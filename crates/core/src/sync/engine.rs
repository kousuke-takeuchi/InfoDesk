//! One sync pass: check the database id, import remote ops, upload local ops, write changed files,
//! compact, apply retention, and write the manifest last.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Mutex;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};

use super::drive::{DriveError, DriveStore, RemoteFile};
use super::format::{
    CategoryMeta, DayFile, DigestFile, DigestRecord, Field, FileEntry, FileKind, ItemState,
    Manifest, MetaFile, Op, OpsFile, SCHEMA, SourceMeta, StateSnapshot, TopicMeta, apply_op,
    day_file_name, digest_file_name, gunzip_json, gzip_json, normalize_op, op_time, ops_file_name,
    parse_file_name, sha256_hex,
};
use crate::model::{Category, DigestStatus, Period};
use crate::period::{period_end, period_start, utc_range};
use crate::store::Store;
use crate::store::SyncFileRow;

const DAY_RETENTION: i64 = 60;
const LONG_RETENTION: i64 = 400;
const OPS_RETENTION_DAYS: i64 = 30;
const MAX_SEQ: usize = 9999;
const OPS_PER_FILE: usize = 1000;
const MIME_JSON: &str = "application/json";
const MIME_GZ: &str = "application/gzip";
const SNAPSHOT: &str = "state-snapshot.json.gz";
const MANIFEST: &str = "manifest.json";
const META: &str = "meta.json.gz";

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Drive(#[from] DriveError),
    #[error("同期データのエラー: {0}")]
    Store(String),
}

fn st<T>(r: crate::Result<T>) -> Result<T, SyncError> {
    r.map_err(|e| SyncError::Store(e.to_string()))
}

#[derive(Debug, Clone)]
pub struct SyncInput {
    pub device_id: String,
    pub db_id: String,
    pub writer: String,
    pub now: DateTime<Utc>,
    pub tz: FixedOffset,
    pub compact: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub imported_ops: usize,
    pub applied_ops: usize,
    pub uploaded_ops: usize,
    pub written_files: Vec<String>,
    pub deleted_files: Vec<String>,
    /// Meta, day and digest files serialized and gzipped in this pass.
    pub built_files: usize,
    /// Problems that did not stop the pass, such as unreadable remote files.
    pub warnings: Vec<String>,
    /// True when compaction ran in this pass, asked for or forced because
    /// Drive had no snapshot.
    pub compacted: bool,
}

/// True when today's `daily_time` has passed and the last compaction was before it.
pub fn compaction_due(
    last: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    tz: FixedOffset,
    daily_time: NaiveTime,
) -> bool {
    let today = now.with_timezone(&tz).date_naive();
    let Some(threshold) = tz
        .from_local_datetime(&today.and_time(daily_time))
        .single()
        .map(|t| t.with_timezone(&Utc))
    else {
        return false;
    };
    now >= threshold && last.is_none_or(|l| l < threshold)
}

/// Locks, runs `f`, and releases before returning, so callers never hold the guard across an await.
fn locked<T>(
    store: &Mutex<Store>,
    f: impl FnOnce(&Store) -> crate::Result<T>,
) -> Result<T, SyncError> {
    let guard = store
        .lock()
        .map_err(|_| SyncError::Store("store lock poisoned".into()))?;
    st(f(&guard))
}

fn date_str(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

enum Job {
    Day(NaiveDate),
    Digest(Period, NaiveDate),
}

/// A day or digest file this pass may write, with the fingerprint of its inputs.
struct Planned {
    name: String,
    fingerprint: String,
    job: Job,
}

/// Everything decided under one short lock before any file is built.
struct Plan {
    meta: MetaFile,
    jobs: Vec<Planned>,
    /// Ids of items fetched inside the day retention window.
    retained_ids: HashSet<i64>,
    out_rows: HashMap<String, SyncFileRow>,
}

/// Digest periods kept on Drive: the last 60 days, and weeks and months
/// starting inside the last 400 days.
fn digest_starts(today: NaiveDate) -> Vec<(Period, NaiveDate)> {
    let day_cut = today - Duration::days(DAY_RETENTION - 1);
    let long_cut = today - Duration::days(LONG_RETENTION - 1);
    let mut starts: Vec<(Period, NaiveDate)> = (0..DAY_RETENTION)
        .map(|i| (Period::Day, day_cut + Duration::days(i)))
        .collect();
    for period in [Period::Week, Period::Month] {
        let mut set = BTreeSet::new();
        let mut d = long_cut;
        while d <= today {
            let s = period_start(period, d);
            if s >= long_cut {
                set.insert(s);
            }
            d += Duration::days(1);
        }
        starts.extend(set.into_iter().map(|s| (period, s)));
    }
    starts
}

/// Reads the cheap inputs for every output file. A handful of queries; no
/// file is built here.
///
/// Fingerprints:
/// - meta: the hash of its JSON (two small queries; only the gzip is skipped).
/// - day: `Store::day_fingerprints`, salted with the topics version because
///   `topic_ids` come from topic keywords.
/// - digest: the topics version, each finished digest's scope, id,
///   `created_at` and body length (`created_at` moves on every regeneration),
///   and the stats inputs. `digest_stats` counts items in the period and the
///   one before it. Items are only ever added with `fetched_at` = now, so a
///   closed period's counts are fixed; for the open period (the one holding
///   today) we add the item count and largest id over [previous start, end).
///   The "open"/"closed" marker differs, so a period is rebuilt once when it
///   closes and picks up items added since the last pass.
fn plan(store: &Store, input: &SyncInput) -> crate::Result<Plan> {
    let today = input.now.with_timezone(&input.tz).date_naive();
    let day_cut = today - Duration::days(DAY_RETENTION - 1);
    let long_cut = today - Duration::days(LONG_RETENTION - 1);
    let topics = store.list_topics()?;
    let topics_version = store.topics_version()?;
    let meta = MetaFile {
        categories: Category::ALL
            .iter()
            .map(|c| CategoryMeta {
                id: c.as_str().to_string(),
                label: c.label_ja().to_string(),
            })
            .collect(),
        topics: topics
            .into_iter()
            .map(|t| TopicMeta {
                id: t.id,
                name: t.name,
            })
            .collect(),
        sources: store
            .list_sources()?
            .into_iter()
            .map(|s| SourceMeta {
                id: s.id,
                name: s.name,
                category: s.category.as_str().to_string(),
            })
            .collect(),
    };

    let mut jobs = Vec::new();
    let mut retained_ids = HashSet::new();
    for (date, day) in store.day_fingerprints(day_cut, input.tz, &topics_version)? {
        retained_ids.extend(day.ids);
        jobs.push(Planned {
            name: day_file_name(date),
            fingerprint: day.fingerprint,
            job: Job::Day(date),
        });
    }

    let digests = store.done_digest_keys(long_cut)?;
    for (period, start) in digest_starts(today) {
        let Some(rows) = digests.get(&(period.as_str(), start)) else {
            continue;
        };
        let end = period_end(period, start);
        let window = if start <= today && today < end {
            let prev = period_start(period, start - Duration::days(1));
            let (since, _) = utc_range(period, prev, input.tz);
            let (_, until) = utc_range(period, start, input.tz);
            let (count, max_id) = store.items_count_and_max_id(&since, &until)?;
            format!("open:{count}:{max_id}")
        } else {
            "closed".to_string()
        };
        let text = format!("{topics_version}\u{1e}{window}\u{1e}{rows}");
        jobs.push(Planned {
            name: digest_file_name(period, start),
            fingerprint: sha256_hex(text.as_bytes()),
            job: Job::Digest(period, start),
        });
    }

    let out_rows = store
        .sync_files("out")?
        .into_iter()
        .map(|r| (r.name.clone(), r))
        .collect();
    Ok(Plan {
        meta,
        jobs,
        retained_ids,
        out_rows,
    })
}

fn build_day(store: &Store, date: NaiveDate, tz: FixedOffset) -> crate::Result<DayFile> {
    Ok(DayFile {
        date: date_str(date),
        items: store.items_fetched_on(date, tz)?,
    })
}

fn build_digest(
    store: &Store,
    period: Period,
    start: NaiveDate,
    tz: FixedOffset,
) -> crate::Result<Option<DigestFile>> {
    let entries: Vec<DigestRecord> = store
        .list_digests(period, start)?
        .into_iter()
        .filter(|d| d.status == DigestStatus::Done)
        .map(|d| {
            Ok(DigestRecord {
                scope: serde_json::to_value(&d.scope)?,
                body: d.body,
                status: d.status.as_str().to_string(),
                item_ids: d.item_ids,
                provider: d.provider,
                created_at: op_time(d.created_at),
            })
        })
        .collect::<crate::Result<_>>()?;
    if entries.is_empty() {
        return Ok(None);
    }
    let stats = store.digest_stats(period, start, tz, None)?;
    Ok(Some(DigestFile {
        period: period.as_str().to_string(),
        period_start: date_str(start),
        stats: Some(serde_json::to_value(stats)?),
        entries,
    }))
}

/// Sort key for an ops file name: the UTC stamp, then the whole name.
fn ops_sort_key(name: &str) -> (String, String) {
    let stamp = name
        .strip_suffix(".json")
        .and_then(|r| r.rsplit('-').nth(1))
        .unwrap_or("")
        .to_string();
    (stamp, name.to_string())
}

fn local_ops(local: BTreeMap<i64, ItemState>) -> impl Iterator<Item = Op> {
    local.into_iter().flat_map(|(item_id, item)| {
        [(Field::Read, item.read), (Field::Starred, item.starred)]
            .into_iter()
            .filter_map(move |(field, stamp)| {
                stamp.map(|stamp| Op {
                    item_id,
                    field,
                    value: stamp.value,
                    at: stamp.at,
                    device: stamp.device,
                })
            })
    })
}

struct Pass<'a> {
    store: &'a Mutex<Store>,
    drive: &'a dyn DriveStore,
    now_s: String,
    remote: BTreeMap<String, RemoteFile>,
    hashes: BTreeMap<String, String>,
    /// The manifest on Drive when the pass started, downloaded once.
    old_manifest: Option<Manifest>,
    report: SyncReport,
}

impl Pass<'_> {
    /// Downloads and parses the manifest. An unparsable one counts as missing
    /// and is rewritten at the end of the pass.
    async fn read_manifest(&mut self) -> Result<(), SyncError> {
        if let Some(f) = self.remote.get(MANIFEST).cloned() {
            let bytes = self.drive.download(&f.id).await?;
            self.old_manifest = serde_json::from_slice::<Manifest>(&bytes).ok();
        }
        Ok(())
    }

    /// Drive holds files written for another database, whose item ids mean
    /// other articles. Deletes every file except the manifest without reading
    /// them, and forgets all `sync_files` rows. The pass then publishes this
    /// database as if Drive were empty.
    async fn reset_for_new_database(&mut self, old_db: &str) -> Result<(), SyncError> {
        let on_drive: Vec<String> = self
            .remote
            .keys()
            .filter(|n| n.as_str() != MANIFEST)
            .cloned()
            .collect();
        for name in &on_drive {
            self.delete_remote(name).await?;
        }
        locked(self.store, |s| {
            for kind in ["in", "out"] {
                for row in s.sync_files(kind)? {
                    s.delete_sync_file(&row.name)?;
                }
            }
            Ok(())
        })?;
        self.report.warnings.push(format!(
            "Drive held sync data of another database ({old_db}); deleted {} files without applying them",
            on_drive.len()
        ));
        Ok(())
    }

    /// Writes `bytes` unless the recorded hash already matches the Drive copy.
    /// `fingerprint` is stored with the 'out' row.
    async fn write_out(
        &mut self,
        name: &str,
        bytes: Vec<u8>,
        mime: &str,
        fingerprint: Option<&str>,
    ) -> Result<bool, SyncError> {
        let sha = sha256_hex(&bytes);
        let existing = self.remote.get(name).cloned();
        let row = locked(self.store, |s| s.sync_file(name))?;
        if let (Some(f), Some(r)) = (&existing, &row)
            && r.kind == "out"
            && r.sha256.as_deref() == Some(sha.as_str())
            && r.drive_id.as_deref() == Some(f.id.as_str())
        {
            if r.fingerprint.as_deref() != fingerprint {
                let updated = SyncFileRow {
                    fingerprint: fingerprint.map(str::to_string),
                    ..r.clone()
                };
                locked(self.store, |s| s.put_sync_file(&updated))?;
            }
            self.hashes.insert(name.to_string(), sha);
            return Ok(false);
        }
        let file = match existing {
            Some(f) => self.drive.update(&f.id, bytes, mime).await?,
            None => self.drive.create(name, bytes, mime).await?,
        };
        let record = SyncFileRow {
            name: name.to_string(),
            kind: "out".to_string(),
            drive_id: Some(file.id.clone()),
            sha256: Some(sha.clone()),
            updated_at: self.now_s.clone(),
            fingerprint: fingerprint.map(str::to_string),
        };
        locked(self.store, |s| s.put_sync_file(&record))?;
        self.remote.insert(name.to_string(), file);
        self.hashes.insert(name.to_string(), sha);
        self.report.written_files.push(name.to_string());
        Ok(true)
    }

    /// True when the 'out' row's fingerprint matches and the file it names is
    /// still on Drive. Its recorded hash then feeds the manifest.
    fn unchanged(&mut self, rows: &HashMap<String, SyncFileRow>, name: &str, fp: &str) -> bool {
        let (Some(row), Some(file)) = (rows.get(name), self.remote.get(name)) else {
            return false;
        };
        let fresh = row.kind == "out"
            && row.fingerprint.as_deref() == Some(fp)
            && row.drive_id.as_deref() == Some(file.id.as_str());
        match (&row.sha256, fresh) {
            (Some(sha), true) => {
                self.hashes.insert(name.to_string(), sha.clone());
                true
            }
            _ => false,
        }
    }

    async fn delete_remote(&mut self, name: &str) -> Result<(), SyncError> {
        if let Some(f) = self.remote.get(name).cloned() {
            self.drive.delete(&f.id).await?;
            self.remote.remove(name);
            self.hashes.remove(name);
            self.report.deleted_files.push(name.to_string());
        }
        locked(self.store, |s| s.delete_sync_file(name))
    }

    /// Downloads an ops file. An unparsable one is reported as a warning and
    /// returned as `None`.
    async fn download_ops(&mut self, name: &str) -> Result<(Vec<u8>, Option<OpsFile>), SyncError> {
        let id = self.remote[name].id.clone();
        let bytes = self.drive.download(&id).await?;
        let parsed = match serde_json::from_slice::<OpsFile>(&bytes) {
            Ok(f) => Some(f),
            Err(e) => {
                self.report
                    .warnings
                    .push(format!("unreadable ops file {name}: {e}"));
                None
            }
        };
        Ok((bytes, parsed))
    }

    fn ops_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .remote
            .keys()
            .filter(|n| matches!(parse_file_name(n), Some(FileKind::Ops { .. })))
            .cloned()
            .collect();
        names.sort_by_key(|n| ops_sort_key(n));
        names
    }

    async fn import_remote_ops(&mut self, input: &SyncInput) -> Result<(), SyncError> {
        let done: HashSet<String> = locked(self.store, |s| s.sync_files("in"))?
            .into_iter()
            .map(|r| r.name)
            .collect();
        let todo: Vec<String> = self
            .ops_names()
            .into_iter()
            .filter(|n| {
                !done.contains(n)
                    && matches!(parse_file_name(n), Some(FileKind::Ops { device }) if device != input.device_id)
            })
            .collect();
        for name in todo {
            let (bytes, parsed) = self.download_ops(&name).await?;
            let sha = sha256_hex(&bytes);
            let mut imported = 0;
            let mut applied = 0;
            if let Some(file) = parsed {
                let ops: Vec<Op> = file.ops.iter().filter_map(normalize_op).collect();
                imported = ops.len();
                applied = locked(self.store, |s| s.apply_remote_ops(&ops, &input.device_id))?;
            }
            let row = SyncFileRow {
                name: name.clone(),
                kind: "in".to_string(),
                drive_id: Some(self.remote[&name].id.clone()),
                sha256: Some(sha.clone()),
                updated_at: self.now_s.clone(),
                fingerprint: None,
            };
            locked(self.store, |s| s.put_sync_file(&row))?;
            self.hashes.insert(name, sha);
            self.report.imported_ops += imported;
            self.report.applied_ops += applied;
        }
        Ok(())
    }

    async fn upload_local_ops(&mut self, input: &SyncInput) -> Result<(), SyncError> {
        let pending = locked(self.store, |s| s.pending_local_ops(&input.device_id))?;
        if pending.is_empty() {
            return Ok(());
        }
        let chunk = OPS_PER_FILE.max(pending.len().div_ceil(MAX_SEQ));
        for (i, part) in pending.chunks(chunk).enumerate() {
            let name = ops_file_name(&input.device_id, input.now, (i + 1) as u32);
            let file = OpsFile {
                device: input.device_id.clone(),
                ops: part.iter().map(|(_, op)| op.clone()).collect(),
            };
            let bytes = st(serde_json::to_vec(&file).map_err(Into::into))?;
            self.write_out(&name, bytes, MIME_JSON, None).await?;
            let ids: Vec<i64> = part.iter().map(|(id, _)| *id).collect();
            locked(self.store, |s| s.mark_ops_uploaded(&ids))?;
            self.report.uploaded_ops += ids.len();
        }
        Ok(())
    }

    /// Writes meta, day and digest files whose inputs changed. The store lock
    /// is taken once for the plan and once per file built, never for the whole
    /// build and never across an await.
    async fn write_outputs(&mut self, input: &SyncInput) -> Result<HashSet<i64>, SyncError> {
        let plan = locked(self.store, |s| plan(s, input))?;

        let meta_json = st(serde_json::to_vec(&plan.meta).map_err(Into::into))?;
        let meta_fp = sha256_hex(&meta_json);
        if !self.unchanged(&plan.out_rows, META, &meta_fp) {
            let bytes = st(gzip_json(&plan.meta))?;
            self.report.built_files += 1;
            self.write_out(META, bytes, MIME_GZ, Some(&meta_fp)).await?;
        }

        for job in &plan.jobs {
            if self.unchanged(&plan.out_rows, &job.name, &job.fingerprint) {
                continue;
            }
            let bytes = match job.job {
                Job::Day(date) => {
                    let file = locked(self.store, |s| build_day(s, date, input.tz))?;
                    st(gzip_json(&file))?
                }
                Job::Digest(period, start) => {
                    let file = locked(self.store, |s| build_digest(s, period, start, input.tz))?;
                    let Some(file) = file else { continue };
                    st(gzip_json(&file))?
                }
            };
            self.report.built_files += 1;
            self.write_out(&job.name, bytes, MIME_GZ, Some(&job.fingerprint))
                .await?;
        }
        Ok(plan.retained_ids)
    }

    /// Reads the snapshot. `Ok(None)` when there is none; an unreadable one is
    /// reported and treated as empty (the Mac DB and its stamps still hold the
    /// state), with `true` meaning it must be rewritten.
    async fn read_snapshot(&mut self) -> Result<Option<(StateSnapshot, bool)>, SyncError> {
        let Some(f) = self.remote.get(SNAPSHOT).cloned() else {
            return Ok(None);
        };
        let bytes = self.drive.download(&f.id).await?;
        match gunzip_json::<StateSnapshot>(&bytes) {
            Ok(snap) => Ok(Some((snap, false))),
            Err(e) => {
                self.report.warnings.push(format!(
                    "unreadable {SNAPSHOT}, rebuilt from local state: {e}"
                ));
                Ok(Some((
                    StateSnapshot {
                        covers: Vec::new(),
                        state: BTreeMap::new(),
                    },
                    true,
                )))
            }
        }
    }

    /// Applies the Mac's stamps (including ones whose `sync_ops` row was pruned
    /// or never written) and keeps only retained items.
    fn fold_local_stamps(
        &self,
        state: &mut BTreeMap<i64, ItemState>,
        input: &SyncInput,
        retained: &HashSet<i64>,
    ) -> Result<(), SyncError> {
        let ids: Vec<i64> = retained.iter().copied().collect();
        let local = locked(self.store, |s| {
            s.local_state_stamps(Some(&ids), &input.device_id)
        })?;
        for op in local_ops(local) {
            apply_op(state, &op);
        }
        state.retain(|id, _| retained.contains(id));
        Ok(())
    }

    async fn compact(
        &mut self,
        input: &SyncInput,
        retained: &HashSet<i64>,
    ) -> Result<(), SyncError> {
        let cutoff = op_time(input.now - Duration::days(OPS_RETENTION_DAYS));
        locked(self.store, |s| s.prune_uploaded_ops(&cutoff))?;

        let names = self.ops_names();
        let existing = self.read_snapshot().await?;
        if names.is_empty()
            && let Some((mut snap, false)) = existing
        {
            // No new ops: fold in the local stamps (marks that never had an
            // ops file, such as ones set while sync was off) and re-apply
            // retention. Rewrite only if that changed the state.
            let before = snap.state.clone();
            self.fold_local_stamps(&mut snap.state, input, retained)?;
            if snap.state != before {
                self.write_out(SNAPSHOT, st(gzip_json(&snap))?, MIME_GZ, None)
                    .await?;
            }
            return Ok(());
        }
        // With no snapshot yet, the first one is built from the local stamps
        // even when there are no ops files.
        let mut state = existing.map(|(s, _)| s.state).unwrap_or_default();
        let mut covers = Vec::new();
        for name in &names {
            // This Mac's own ops are already in its stamps, read below.
            if matches!(parse_file_name(name), Some(FileKind::Ops { device }) if device == input.device_id)
            {
                covers.push(name.clone());
                continue;
            }
            let (_, parsed) = self.download_ops(name).await?;
            let Some(file) = parsed else { continue };
            for op in file.ops.iter().filter_map(normalize_op) {
                apply_op(&mut state, &op);
            }
            covers.push(name.clone());
        }
        self.fold_local_stamps(&mut state, input, retained)?;
        let snapshot = StateSnapshot {
            covers: covers.clone(),
            state,
        };
        self.write_out(SNAPSHOT, st(gzip_json(&snapshot))?, MIME_GZ, None)
            .await?;
        for name in covers {
            self.delete_remote(&name).await?;
        }
        Ok(())
    }

    async fn apply_retention(&mut self, input: &SyncInput) -> Result<(), SyncError> {
        let today = input.now.with_timezone(&input.tz).date_naive();
        let day_cut = today - Duration::days(DAY_RETENTION - 1);
        let long_cut = today - Duration::days(LONG_RETENTION - 1);
        let old: Vec<String> = self
            .remote
            .keys()
            .filter(|n| match parse_file_name(n) {
                Some(FileKind::Day(d)) => d < day_cut,
                Some(FileKind::Digest(Period::Day, d)) => d < day_cut,
                Some(FileKind::Digest(_, d)) => d < long_cut,
                _ => false,
            })
            .cloned()
            .collect();
        for name in old {
            self.delete_remote(&name).await?;
        }
        Ok(())
    }

    async fn write_manifest(&mut self, input: &SyncInput) -> Result<(), SyncError> {
        let mut files = BTreeMap::new();
        let listing: Vec<(String, RemoteFile)> = self
            .remote
            .iter()
            .filter(|(n, _)| n.as_str() != MANIFEST)
            .map(|(n, f)| (n.clone(), f.clone()))
            .collect();
        for (name, f) in listing {
            let mut size = f.size;
            let sha = match self.hashes.get(&name) {
                Some(h) => h.clone(),
                None => {
                    let row = locked(self.store, |s| s.sync_file(&name))?;
                    match row {
                        Some(r)
                            if r.drive_id.as_deref() == Some(f.id.as_str())
                                && r.sha256.is_some() =>
                        {
                            r.sha256.unwrap_or_default()
                        }
                        _ => {
                            let bytes = self.drive.download(&f.id).await?;
                            size = Some(bytes.len() as u64);
                            sha256_hex(&bytes)
                        }
                    }
                }
            };
            files.insert(
                name,
                FileEntry {
                    sha256: sha,
                    size: size.unwrap_or(0),
                    updated_at: f.modified_time.clone(),
                },
            );
        }
        let manifest = Manifest {
            schema: SCHEMA,
            db_id: input.db_id.clone(),
            written_at: self.now_s.clone(),
            writer: input.writer.clone(),
            files,
        };
        let existing = self.remote.get(MANIFEST).cloned();
        // `written_at` means "when the content last changed": an identical
        // manifest (ignoring `written_at`) is not rewritten.
        if existing.is_some()
            && let Some(old) = self.old_manifest.take()
            && (Manifest {
                written_at: manifest.written_at.clone(),
                ..old
            }) == manifest
        {
            return Ok(());
        }
        let bytes = st(serde_json::to_vec(&manifest).map_err(Into::into))?;
        let file = match existing {
            Some(f) => self.drive.update(&f.id, bytes, MIME_JSON).await?,
            None => self.drive.create(MANIFEST, bytes, MIME_JSON).await?,
        };
        self.remote.insert(MANIFEST.to_string(), file);
        self.report.written_files.push(MANIFEST.to_string());
        Ok(())
    }
}

pub async fn run_sync(
    store: &Mutex<Store>,
    drive: &dyn DriveStore,
    input: &SyncInput,
) -> Result<SyncReport, SyncError> {
    // The folder may hold files the pass did not write (a NAS share holds
    // `.DS_Store`, leftovers of a connection check, the user's own files).
    // Only names of the sync format are ever read, listed or deleted.
    let listing: Vec<RemoteFile> = drive
        .list()
        .await?
        .into_iter()
        .filter(|f| parse_file_name(&f.name).is_some())
        .collect();
    let mut pass = Pass {
        store,
        drive,
        now_s: op_time(input.now),
        remote: listing.into_iter().map(|f| (f.name.clone(), f)).collect(),
        hashes: BTreeMap::new(),
        old_manifest: None,
        report: SyncReport::default(),
    };
    pass.read_manifest().await?;
    if let Some(old_db) = pass
        .old_manifest
        .as_ref()
        .map(|m| m.db_id.clone())
        .filter(|id| *id != input.db_id)
    {
        pass.reset_for_new_database(&old_db).await?;
    }
    pass.import_remote_ops(input).await?;
    pass.upload_local_ops(input).await?;
    let retained = pass.write_outputs(input).await?;
    // Without a snapshot the phone cannot see marks that have no ops file,
    // such as ones set before sync existed, so the first pass always compacts.
    if input.compact || !pass.remote.contains_key(SNAPSHOT) {
        pass.compact(input, &retained).await?;
        pass.report.compacted = true;
    }
    pass.apply_retention(input).await?;
    pass.write_manifest(input).await?;
    Ok(pass.report)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use super::*;
    use crate::model::{DigestScope, ItemFilter};
    use crate::store::NewDigest;
    use crate::sync::drive::MemoryDrive;
    use crate::sync::format::Field;
    use crate::testutil::{at, new_item, store_with_source};

    fn tz() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    fn input(now: &str, compact: bool) -> SyncInput {
        SyncInput {
            device_id: "mac".into(),
            db_id: "db1".into(),
            writer: "Mac".into(),
            now: at(now),
            tz: tz(),
            compact,
        }
    }

    const NOW: &str = "2026-10-13T12:00:00Z";

    fn setup() -> Mutex<Store> {
        let s = store_with_source(Category::TechJp).0;
        s.set_setting("sync.enabled", "true").unwrap();
        s.set_setting("sync.device_id", "mac").unwrap();
        Mutex::new(s)
    }

    /// An empty snapshot, so a pass compacts only when asked to.
    fn put_empty_snapshot(drive: &MemoryDrive) {
        let snap = StateSnapshot {
            covers: Vec::new(),
            state: BTreeMap::new(),
        };
        drive.put(SNAPSHOT, gzip_json(&snap).unwrap());
    }

    fn add_item(m: &Mutex<Store>, url: &str, fetched: &str) -> i64 {
        let s = m.lock().unwrap();
        let src = s.list_sources().unwrap().remove(0);
        s.upsert_items(&src, &[new_item(url, "t", "", None)], &at(fetched))
            .unwrap();
        s.query_items(&ItemFilter::default())
            .unwrap()
            .into_iter()
            .find(|i| i.url == url)
            .unwrap()
            .id
    }

    fn put_ops(drive: &MemoryDrive, device: &str, stamp: &str, ops: Vec<Op>) -> String {
        let name = ops_file_name(device, at(stamp), 1);
        let file = OpsFile {
            device: device.into(),
            ops,
        };
        drive.put(&name, serde_json::to_vec(&file).unwrap());
        name
    }

    fn op(item: i64, field: Field, value: bool, at: &str, device: &str) -> Op {
        Op {
            item_id: item,
            field,
            value,
            at: at.into(),
            device: device.into(),
        }
    }

    async fn read_file(drive: &MemoryDrive, name: &str) -> Option<Vec<u8>> {
        let f = drive
            .list()
            .await
            .unwrap()
            .into_iter()
            .find(|f| f.name == name)?;
        Some(drive.download(&f.id).await.unwrap())
    }

    #[tokio::test]
    async fn first_sync_writes_meta_days_digests_and_manifest_last() {
        let m = setup();
        add_item(&m, "https://a/1", "2026-10-12T03:00:00Z");
        let id2 = add_item(&m, "https://a/2", "2026-10-13T03:00:00Z");
        m.lock()
            .unwrap()
            .save_digest_done(
                &NewDigest {
                    period: Period::Day,
                    period_start: NaiveDate::from_ymd_opt(2026, 10, 13).unwrap(),
                    scope: DigestScope::Category(Category::TechJp),
                    body: "body".into(),
                    item_ids: vec![id2],
                    provider: "p".into(),
                },
                &at(NOW),
            )
            .unwrap();
        let drive = MemoryDrive::new();
        let report = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert_eq!(report.written_files.last().unwrap(), "manifest.json");
        let names = drive.names();
        for n in [
            "meta.json.gz",
            "day-2026-10-12.json.gz",
            "day-2026-10-13.json.gz",
            "digest-day-2026-10-13.json.gz",
            "manifest.json",
        ] {
            assert!(names.contains(&n.to_string()), "{n} in {names:?}");
        }
        let manifest: Manifest =
            serde_json::from_slice(&read_file(&drive, "manifest.json").await.unwrap()).unwrap();
        assert_eq!(manifest.db_id, "db1");
        assert_eq!(manifest.files.len(), names.len() - 1);
        for (name, entry) in &manifest.files {
            let bytes = read_file(&drive, name).await.unwrap();
            assert_eq!(entry.sha256, sha256_hex(&bytes), "{name}");
            assert_eq!(entry.size, bytes.len() as u64);
        }
        let digest: DigestFile = gunzip_json(
            &read_file(&drive, "digest-day-2026-10-13.json.gz")
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(digest.entries.len(), 1);
        assert!(digest.stats.is_some());
    }

    #[tokio::test]
    async fn second_sync_without_changes_writes_nothing() {
        let m = setup();
        add_item(&m, "https://a/1", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        let again = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert!(again.written_files.is_empty(), "{:?}", again.written_files);
        assert!(again.deleted_files.is_empty());
    }

    #[tokio::test]
    async fn phone_ops_are_applied_once() {
        let m = setup();
        let id = add_item(&m, "https://a/1", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        put_ops(
            &drive,
            "phone",
            "2026-10-13T11:00:00.000Z",
            vec![op(
                id,
                Field::Read,
                true,
                "2026-10-13T11:00:00.000Z",
                "phone",
            )],
        );
        let first = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert_eq!((first.imported_ops, first.applied_ops), (1, 1));
        assert!(m.lock().unwrap().get_item(id).unwrap().unwrap().read);
        let second = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert_eq!(second.imported_ops, 0);
    }

    #[tokio::test]
    async fn local_ops_are_uploaded_once() {
        let m = setup();
        let id = add_item(&m, "https://a/1", "2026-10-13T03:00:00Z");
        m.lock().unwrap().set_read(id, true).unwrap();
        let drive = MemoryDrive::new();
        put_empty_snapshot(&drive);
        let first = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert_eq!(first.uploaded_ops, 1);
        let ops_files: Vec<String> = drive
            .names()
            .into_iter()
            .filter(|n| n.starts_with("ops-mac-"))
            .collect();
        assert_eq!(ops_files.len(), 1);
        let file: OpsFile =
            serde_json::from_slice(&read_file(&drive, &ops_files[0]).await.unwrap()).unwrap();
        assert_eq!(file.ops.len(), 1);
        assert_eq!(file.ops[0].device, "mac");
        let second = run_sync(&m, &drive, &input("2026-10-13T12:05:00Z", false))
            .await
            .unwrap();
        assert_eq!(second.uploaded_ops, 0);
        assert_eq!(second.imported_ops, 0, "own ops files are not imported");
        assert_eq!(
            drive
                .names()
                .iter()
                .filter(|n| n.starts_with("ops-"))
                .count(),
            1
        );
    }

    /// Fails the Nth call (1-based) of any kind with a retryable error.
    struct FailAt {
        inner: MemoryDrive,
        calls: AtomicUsize,
        at: AtomicUsize,
    }

    impl FailAt {
        fn tick(&self) {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if n == self.at.load(Ordering::SeqCst) {
                *self.inner.fail_next.lock().unwrap() = Some(DriveError::Retryable("boom".into()));
            }
        }
    }

    #[async_trait]
    impl DriveStore for FailAt {
        async fn list(&self) -> Result<Vec<RemoteFile>, DriveError> {
            self.tick();
            self.inner.list().await
        }
        async fn download(&self, id: &str) -> Result<Vec<u8>, DriveError> {
            self.tick();
            self.inner.download(id).await
        }
        async fn create(
            &self,
            name: &str,
            bytes: Vec<u8>,
            mime: &str,
        ) -> Result<RemoteFile, DriveError> {
            self.tick();
            self.inner.create(name, bytes, mime).await
        }
        async fn update(
            &self,
            id: &str,
            bytes: Vec<u8>,
            mime: &str,
        ) -> Result<RemoteFile, DriveError> {
            self.tick();
            self.inner.update(id, bytes, mime).await
        }
        async fn delete(&self, id: &str) -> Result<(), DriveError> {
            self.tick();
            self.inner.delete(id).await
        }
    }

    #[tokio::test]
    async fn failure_midway_skips_the_manifest_and_retries_next_time() {
        let m = setup();
        add_item(&m, "https://a/1", "2026-10-12T03:00:00Z");
        add_item(&m, "https://a/2", "2026-10-13T03:00:00Z");
        let drive = FailAt {
            inner: MemoryDrive::new(),
            calls: AtomicUsize::new(0),
            at: AtomicUsize::new(3),
        };
        let err = run_sync(&m, &drive, &input(NOW, false)).await.unwrap_err();
        assert!(matches!(err, SyncError::Drive(DriveError::Retryable(_))));
        assert!(!drive.inner.names().contains(&"manifest.json".to_string()));
        let partial = drive.inner.names().len();
        assert!(partial >= 1);

        drive.at.store(0, Ordering::SeqCst);
        let report = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        let names = drive.inner.names();
        for n in [
            "meta.json.gz",
            "day-2026-10-12.json.gz",
            "day-2026-10-13.json.gz",
            "state-snapshot.json.gz",
            "manifest.json",
        ] {
            assert!(names.contains(&n.to_string()), "{n} in {names:?}");
        }
        assert_eq!(names.len(), 5, "no duplicates: {names:?}");
        assert_eq!(report.written_files.len(), 4 - partial + 1);
    }

    #[tokio::test]
    async fn failed_import_leaves_the_file_unrecorded() {
        let m = setup();
        let id = add_item(&m, "https://a/1", "2026-10-13T03:00:00Z");
        let drive = FailAt {
            inner: MemoryDrive::new(),
            calls: AtomicUsize::new(0),
            at: AtomicUsize::new(2),
        };
        put_ops(
            &drive.inner,
            "phone",
            "2026-10-13T11:00:00.000Z",
            vec![op(
                id,
                Field::Read,
                true,
                "2026-10-13T11:00:00.000Z",
                "phone",
            )],
        );
        assert!(run_sync(&m, &drive, &input(NOW, false)).await.is_err());
        assert!(m.lock().unwrap().sync_files("in").unwrap().is_empty());
        drive.at.store(0, Ordering::SeqCst);
        let ok = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert_eq!(ok.imported_ops, 1);
    }

    fn compose(ops: &[Op]) -> BTreeMap<i64, ItemState> {
        let mut s = BTreeMap::new();
        for o in ops {
            apply_op(&mut s, &normalize_op(o).unwrap());
        }
        s
    }

    #[tokio::test]
    async fn compaction_merges_ops_into_the_snapshot_and_deletes_them() {
        let m = setup();
        let id1 = add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        let id2 = add_item(&m, "https://a/2", "2026-10-13T02:00:00Z");
        let id3 = add_item(&m, "https://a/3", "2026-10-13T03:00:00Z");
        {
            let s = m.lock().unwrap();
            s.set_read(id1, true).unwrap();
            s.set_starred(id2, true).unwrap();
        }
        let drive = MemoryDrive::new();
        let phone_a = vec![
            op(id1, Field::Read, false, "2999-01-01T00:00:00.000Z", "phone"),
            op(id3, Field::Starred, true, "2999-01-01T00:00:01Z", "phone"),
        ];
        let first_name = put_ops(&drive, "phone", "2026-10-13T11:00:00.000Z", phone_a.clone());
        let report = run_sync(&m, &drive, &input(NOW, true)).await.unwrap();
        assert!(report.deleted_files.contains(&first_name));
        assert!(!drive.names().iter().any(|n| n.starts_with("ops-")));
        assert!(
            drive
                .names()
                .contains(&"state-snapshot.json.gz".to_string())
        );

        let snap: StateSnapshot =
            gunzip_json(&read_file(&drive, "state-snapshot.json.gz").await.unwrap()).unwrap();
        assert!(snap.covers.iter().any(|c| c.starts_with("ops-mac-")));
        assert!(snap.covers.contains(&first_name));
        let r1 = snap.state[&id1].read.as_ref().unwrap();
        assert!(!r1.value);
        assert_eq!(r1.device, "phone");
        let s2 = snap.state[&id2].starred.as_ref().unwrap();
        assert!(s2.value);
        assert_eq!(s2.device, "mac");
        assert!(snap.state[&id3].starred.as_ref().unwrap().value);

        // A mobile client: snapshot plus the ops that arrive later equals the
        // composition of every op ever written.
        let later = vec![
            op(
                id2,
                Field::Starred,
                false,
                "2999-01-02T00:00:00.000Z",
                "phone",
            ),
            op(id1, Field::Read, true, "2999-01-03T00:00:00.000Z", "phone"),
        ];
        let mut mobile = snap.state.clone();
        for o in &later {
            apply_op(&mut mobile, &normalize_op(o).unwrap());
        }
        let mac_ops: Vec<Op> = {
            let s = m.lock().unwrap();
            s.local_state_stamps(None, "mac")
                .unwrap()
                .into_iter()
                .flat_map(|(id, st)| {
                    [(Field::Read, st.read), (Field::Starred, st.starred)]
                        .into_iter()
                        .filter_map(move |(f, stamp)| {
                            stamp.filter(|s| s.device == "mac").map(|s| Op {
                                item_id: id,
                                field: f,
                                value: s.value,
                                at: s.at,
                                device: s.device,
                            })
                        })
                })
                .collect()
        };
        let mut all = mac_ops;
        all.extend(phone_a);
        all.extend(later);
        assert_eq!(mobile, compose(&all));
    }

    #[tokio::test]
    async fn retention_deletes_old_days() {
        let m = setup();
        let drive = MemoryDrive::new();
        drive.put("day-2026-08-14.json.gz", b"x".to_vec());
        drive.put("day-2026-08-15.json.gz", b"x".to_vec());
        drive.put("digest-day-2026-08-14.json.gz", b"x".to_vec());
        drive.put("digest-week-2025-09-01.json.gz", b"x".to_vec());
        drive.put("digest-week-2025-09-15.json.gz", b"x".to_vec());
        let report = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        let names = drive.names();
        assert!(!names.contains(&"day-2026-08-14.json.gz".to_string()));
        assert!(names.contains(&"day-2026-08-15.json.gz".to_string()));
        assert!(!names.contains(&"digest-day-2026-08-14.json.gz".to_string()));
        assert!(!names.contains(&"digest-week-2025-09-01.json.gz".to_string()));
        assert!(names.contains(&"digest-week-2025-09-15.json.gz".to_string()));
        assert_eq!(report.deleted_files.len(), 3);
        let manifest: Manifest =
            serde_json::from_slice(&read_file(&drive, "manifest.json").await.unwrap()).unwrap();
        assert!(manifest.files.contains_key("day-2026-08-15.json.gz"));
        assert!(!manifest.files.contains_key("day-2026-08-14.json.gz"));
    }

    async fn snapshot_of(drive: &MemoryDrive) -> StateSnapshot {
        gunzip_json(&read_file(drive, SNAPSHOT).await.unwrap()).unwrap()
    }

    async fn manifest_of(drive: &MemoryDrive) -> Manifest {
        serde_json::from_slice(&read_file(drive, MANIFEST).await.unwrap()).unwrap()
    }

    fn ids_of(state: &BTreeMap<i64, ItemState>) -> Vec<i64> {
        state.keys().copied().collect()
    }

    /// Marks every pending local op uploaded and prunes it, so the stamp lives
    /// only on the item row.
    fn drop_journal(m: &Mutex<Store>) {
        let s = m.lock().unwrap();
        let ids: Vec<i64> = s
            .pending_local_ops("mac")
            .unwrap()
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        s.mark_ops_uploaded(&ids).unwrap();
        s.prune_uploaded_ops("2999-01-01T00:00:00.000Z").unwrap();
        assert!(s.pending_local_ops("mac").unwrap().is_empty());
    }

    #[tokio::test]
    async fn unreadable_snapshot_is_treated_as_empty_and_reported() {
        let m = setup();
        let id1 = add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        m.lock().unwrap().set_read(id1, true).unwrap();
        let drive = MemoryDrive::new();
        drive.put(SNAPSHOT, b"not gzip".to_vec());
        put_ops(
            &drive,
            "phone",
            "2026-10-13T11:00:00.000Z",
            vec![op(
                id1,
                Field::Starred,
                true,
                "2999-01-01T00:00:00.000Z",
                "phone",
            )],
        );
        let report = run_sync(&m, &drive, &input(NOW, true)).await.unwrap();
        assert!(
            report.warnings.iter().any(|w| w.contains(SNAPSHOT)),
            "{:?}",
            report.warnings
        );
        assert!(!drive.names().iter().any(|n| n.starts_with("ops-")));
        let snap = snapshot_of(&drive).await;
        assert!(snap.state[&id1].read.as_ref().unwrap().value);
        assert!(snap.state[&id1].starred.as_ref().unwrap().value);
        assert!(drive.names().contains(&MANIFEST.to_string()));
    }

    #[tokio::test]
    async fn compaction_keeps_mac_stamps_without_a_journal_row() {
        let m = setup();
        let id1 = add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        let id2 = add_item(&m, "https://a/2", "2026-10-13T02:00:00Z");
        m.lock().unwrap().set_read(id1, true).unwrap();
        drop_journal(&m);
        let at1 = m.lock().unwrap().local_state_stamps(None, "mac").unwrap()[&id1]
            .read
            .clone()
            .unwrap()
            .at;
        let drive = MemoryDrive::new();
        put_ops(
            &drive,
            "phone",
            "2026-10-13T11:00:00.000Z",
            vec![op(
                id2,
                Field::Read,
                true,
                "2026-10-13T11:00:00.000Z",
                "phone",
            )],
        );
        let report = run_sync(&m, &drive, &input(NOW, true)).await.unwrap();
        assert_eq!(report.uploaded_ops, 0);
        let snap = snapshot_of(&drive).await;
        let r1 = snap.state[&id1].read.as_ref().expect("mac stamp kept");
        assert!(r1.value);
        assert_eq!(r1.at, at1);
        assert!(snap.state[&id2].read.as_ref().unwrap().value);
    }

    #[tokio::test]
    async fn failed_snapshot_write_keeps_ops_files_and_skips_the_manifest() {
        let m = setup();
        let id1 = add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        m.lock().unwrap().set_read(id1, true).unwrap();
        let drive = MemoryDrive::new();
        let phone = put_ops(
            &drive,
            "phone",
            "2026-10-13T11:00:00.000Z",
            vec![op(
                id1,
                Field::Starred,
                true,
                "2026-10-13T11:00:00.000Z",
                "phone",
            )],
        );
        *drive.fail_on_name.lock().unwrap() = Some(SNAPSHOT.into());
        let err = run_sync(&m, &drive, &input(NOW, true)).await.unwrap_err();
        assert!(matches!(err, SyncError::Drive(DriveError::Retryable(_))));
        let names = drive.names();
        let mac: Vec<&String> = names.iter().filter(|n| n.starts_with("ops-mac-")).collect();
        assert_eq!(mac.len(), 1, "{names:?}");
        assert!(names.contains(&phone));
        assert!(!names.contains(&SNAPSHOT.to_string()));
        assert!(!names.contains(&MANIFEST.to_string()));
        let s = m.lock().unwrap();
        assert_eq!(s.sync_file(&phone).unwrap().unwrap().kind, "in");
        assert_eq!(s.sync_file(mac[0]).unwrap().unwrap().kind, "out");
    }

    #[tokio::test]
    async fn failed_ops_upload_leaves_local_ops_pending() {
        let m = setup();
        let id1 = add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        m.lock().unwrap().set_read(id1, true).unwrap();
        let drive = MemoryDrive::new();
        *drive.fail_on_name.lock().unwrap() = Some("ops-mac-".into());
        assert!(run_sync(&m, &drive, &input(NOW, false)).await.is_err());
        assert_eq!(m.lock().unwrap().pending_local_ops("mac").unwrap().len(), 1);
        assert!(!drive.names().iter().any(|n| n.starts_with("ops-")));
        let ok = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert_eq!(ok.uploaded_ops, 1);
    }

    #[tokio::test]
    async fn compaction_drops_items_outside_retention_and_unknown_ids() {
        let m = setup();
        let old = add_item(&m, "https://a/old", "2026-08-01T01:00:00Z");
        let recent = add_item(&m, "https://a/new", "2026-10-13T01:00:00Z");
        let drive = MemoryDrive::new();
        put_ops(
            &drive,
            "phone",
            "2026-10-13T11:00:00.000Z",
            vec![
                op(old, Field::Read, true, "2026-10-13T11:00:00.000Z", "phone"),
                op(
                    99_999,
                    Field::Read,
                    true,
                    "2026-10-13T11:00:00.000Z",
                    "phone",
                ),
                op(
                    recent,
                    Field::Read,
                    true,
                    "2026-10-13T11:00:00.000Z",
                    "phone",
                ),
            ],
        );
        run_sync(&m, &drive, &input(NOW, true)).await.unwrap();
        assert_eq!(ids_of(&snapshot_of(&drive).await.state), vec![recent]);
    }

    #[tokio::test]
    async fn manifest_lists_ops_files_and_the_snapshot_with_their_hashes() {
        let m = setup();
        let id1 = add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        m.lock().unwrap().set_read(id1, true).unwrap();
        let drive = MemoryDrive::new();
        let phone = put_ops(
            &drive,
            "phone",
            "2026-10-13T11:00:00.000Z",
            vec![op(
                id1,
                Field::Starred,
                true,
                "2026-10-13T11:00:00.000Z",
                "phone",
            )],
        );
        async fn check(drive: &MemoryDrive) -> Manifest {
            let manifest = manifest_of(drive).await;
            assert_eq!(manifest.files.len(), drive.names().len() - 1);
            for (name, entry) in &manifest.files {
                let bytes = read_file(drive, name).await.unwrap();
                assert_eq!(entry.sha256, sha256_hex(&bytes), "{name}");
            }
            manifest
        }
        put_empty_snapshot(&drive);
        run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        let first = check(&drive).await;
        assert!(first.files.contains_key(&phone));
        assert!(first.files.keys().any(|n| n.starts_with("ops-mac-")));

        run_sync(&m, &drive, &input("2026-10-13T12:01:00Z", true))
            .await
            .unwrap();
        let second = check(&drive).await;
        assert!(second.files.contains_key(SNAPSHOT));
        assert!(!second.files.keys().any(|n| n.starts_with("ops-")));
    }

    #[tokio::test]
    async fn retention_keeps_the_60th_digest_day_and_drops_old_months() {
        let m = setup();
        let drive = MemoryDrive::new();
        drive.put("digest-day-2026-08-15.json.gz", b"x".to_vec());
        drive.put("digest-month-2025-08-01.json.gz", b"x".to_vec());
        drive.put("digest-month-2025-10-01.json.gz", b"x".to_vec());
        run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        let names = drive.names();
        assert!(names.contains(&"digest-day-2026-08-15.json.gz".to_string()));
        assert!(!names.contains(&"digest-month-2025-08-01.json.gz".to_string()));
        assert!(names.contains(&"digest-month-2025-10-01.json.gz".to_string()));
    }

    fn save_day_digest(m: &Mutex<Store>, day: u32, ids: Vec<i64>, now: &str) {
        m.lock()
            .unwrap()
            .save_digest_done(
                &NewDigest {
                    period: Period::Day,
                    period_start: NaiveDate::from_ymd_opt(2026, 10, day).unwrap(),
                    scope: DigestScope::Category(Category::TechJp),
                    body: "body".into(),
                    item_ids: ids,
                    provider: "p".into(),
                },
                &at(now),
            )
            .unwrap();
    }

    #[tokio::test]
    async fn unchanged_second_pass_builds_nothing() {
        let m = setup();
        let id1 = add_item(&m, "https://a/1", "2026-10-12T03:00:00Z");
        let id2 = add_item(&m, "https://a/2", "2026-10-13T03:00:00Z");
        save_day_digest(&m, 12, vec![id1], NOW);
        save_day_digest(&m, 13, vec![id2], NOW);
        let drive = MemoryDrive::new();
        let first = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert_eq!(first.built_files, 5);
        let again = run_sync(&m, &drive, &input("2026-10-13T12:01:00Z", false))
            .await
            .unwrap();
        assert_eq!(again.built_files, 0);
        assert!(again.written_files.is_empty(), "{:?}", again.written_files);
        check_manifest_hashes(&drive).await;
    }

    async fn check_manifest_hashes(drive: &MemoryDrive) {
        let manifest = manifest_of(drive).await;
        assert_eq!(manifest.files.len(), drive.names().len() - 1);
        for (name, entry) in &manifest.files {
            let bytes = read_file(drive, name).await.unwrap();
            assert_eq!(entry.sha256, sha256_hex(&bytes), "{name}");
        }
    }

    #[tokio::test]
    async fn changed_summary_rebuilds_only_that_day() {
        let m = setup();
        let id1 = add_item(&m, "https://a/1", "2026-10-12T03:00:00Z");
        add_item(&m, "https://a/2", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        m.lock().unwrap().set_ai_summary(id1, "要約").unwrap();
        let again = run_sync(&m, &drive, &input("2026-10-13T12:01:00Z", false))
            .await
            .unwrap();
        assert_eq!(again.built_files, 1);
        assert_eq!(
            again.written_files,
            vec!["day-2026-10-12.json.gz", MANIFEST]
        );
        let day: DayFile =
            gunzip_json(&read_file(&drive, "day-2026-10-12.json.gz").await.unwrap()).unwrap();
        assert_eq!(day.items[0].ai_summary.as_deref(), Some("要約"));
        check_manifest_hashes(&drive).await;
    }

    #[tokio::test]
    async fn new_item_rebuilds_its_day_and_the_open_digest_only() {
        let m = setup();
        let id1 = add_item(&m, "https://a/1", "2026-10-12T03:00:00Z");
        let id2 = add_item(&m, "https://a/2", "2026-10-13T03:00:00Z");
        save_day_digest(&m, 12, vec![id1], NOW);
        save_day_digest(&m, 13, vec![id2], NOW);
        let drive = MemoryDrive::new();
        run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        add_item(&m, "https://a/3", "2026-10-13T12:00:30Z");
        let again = run_sync(&m, &drive, &input("2026-10-13T12:01:00Z", false))
            .await
            .unwrap();
        assert_eq!(
            again.written_files,
            vec![
                "day-2026-10-13.json.gz",
                "digest-day-2026-10-13.json.gz",
                MANIFEST
            ]
        );
        assert_eq!(again.built_files, 2);
    }

    #[tokio::test]
    async fn topic_change_rebuilds_meta_and_every_day() {
        let m = setup();
        add_item(&m, "https://a/1", "2026-10-12T03:00:00Z");
        add_item(&m, "https://a/2", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        m.lock()
            .unwrap()
            .add_topic("Rust", &["Rust".to_string()])
            .unwrap();
        let again = run_sync(&m, &drive, &input("2026-10-13T12:01:00Z", false))
            .await
            .unwrap();
        assert_eq!(again.built_files, 3);
    }

    #[tokio::test]
    async fn unparsable_ops_files_are_reported() {
        let m = setup();
        add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        let drive = MemoryDrive::new();
        let bad = ops_file_name("phone", at("2026-10-13T11:00:00Z"), 1);
        drive.put(&bad, b"{not json".to_vec());
        let first = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert!(
            first.warnings.iter().any(|w| w.contains(&bad)),
            "{:?}",
            first.warnings
        );
        let second = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert!(second.warnings.is_empty(), "{:?}", second.warnings);
        let compact = run_sync(&m, &drive, &input(NOW, true)).await.unwrap();
        assert!(
            compact.warnings.iter().any(|w| w.contains(&bad)),
            "{:?}",
            compact.warnings
        );
        assert!(
            drive.names().contains(&bad),
            "unreadable ops are not deleted"
        );
    }

    #[tokio::test]
    async fn snapshot_is_refiltered_for_retention_without_new_ops() {
        let m = setup();
        let old = add_item(&m, "https://a/old", "2026-08-01T01:00:00Z");
        let recent = add_item(&m, "https://a/new", "2026-10-13T01:00:00Z");
        let mut state = BTreeMap::new();
        for id in [old, recent, 99_999] {
            apply_op(
                &mut state,
                &op(id, Field::Read, true, "2026-10-01T00:00:00.000Z", "phone"),
            );
        }
        let drive = MemoryDrive::new();
        drive.put(
            SNAPSHOT,
            gzip_json(&StateSnapshot {
                covers: vec!["ops-phone-x.json".into()],
                state,
            })
            .unwrap(),
        );
        let first = run_sync(&m, &drive, &input(NOW, true)).await.unwrap();
        assert!(first.written_files.contains(&SNAPSHOT.to_string()));
        let snap = snapshot_of(&drive).await;
        assert_eq!(ids_of(&snap.state), vec![recent]);
        assert_eq!(snap.covers, vec!["ops-phone-x.json"]);
        let second = run_sync(&m, &drive, &input(NOW, true)).await.unwrap();
        assert!(!second.written_files.contains(&SNAPSHOT.to_string()));
    }

    const EPOCH: &str = "1970-01-01T00:00:00.000Z";

    /// A read mark set before schema v5, as the v5 migration leaves it.
    fn pre_v5_read(m: &Mutex<Store>, id: i64) {
        m.lock()
            .unwrap()
            .conn
            .execute(
                "UPDATE items SET read = 1, read_changed_at = ?2 WHERE id = ?1",
                rusqlite::params![id, EPOCH],
            )
            .unwrap();
    }

    /// Records the name of every file downloaded.
    #[derive(Default)]
    struct Recording {
        inner: MemoryDrive,
        downloads: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait]
    impl DriveStore for Recording {
        async fn list(&self) -> Result<Vec<RemoteFile>, DriveError> {
            self.inner.list().await
        }
        async fn download(&self, id: &str) -> Result<Vec<u8>, DriveError> {
            let name = self
                .inner
                .list()
                .await?
                .into_iter()
                .find(|f| f.id == id)
                .map(|f| f.name)
                .unwrap_or_default();
            self.downloads.lock().unwrap().push(name);
            self.inner.download(id).await
        }
        async fn create(
            &self,
            name: &str,
            bytes: Vec<u8>,
            mime: &str,
        ) -> Result<RemoteFile, DriveError> {
            self.inner.create(name, bytes, mime).await
        }
        async fn update(
            &self,
            id: &str,
            bytes: Vec<u8>,
            mime: &str,
        ) -> Result<RemoteFile, DriveError> {
            self.inner.update(id, bytes, mime).await
        }
        async fn delete(&self, id: &str) -> Result<(), DriveError> {
            self.inner.delete(id).await
        }
    }

    impl Recording {
        fn take(&self) -> Vec<String> {
            std::mem::take(&mut *self.downloads.lock().unwrap())
        }
    }

    #[tokio::test]
    async fn pre_v5_marks_reach_the_snapshot_on_the_first_pass() {
        let m = setup();
        let id = add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        pre_v5_read(&m, id);
        let drive = MemoryDrive::new();
        let report = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert!(report.compacted);
        let snap = snapshot_of(&drive).await;
        let r = snap.state[&id]
            .read
            .as_ref()
            .expect("pre-v5 mark published");
        assert!(r.value);
        assert_eq!(r.at, EPOCH);
        assert_eq!(r.device, "mac");
        assert!(manifest_of(&drive).await.files.contains_key(SNAPSHOT));
        // Once the snapshot exists, a pass without `compact` leaves it alone.
        let again = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert!(!again.compacted);
    }

    #[tokio::test]
    async fn compaction_without_ops_folds_local_stamps_into_the_snapshot() {
        let m = setup();
        let id = add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        pre_v5_read(&m, id);
        let drive = MemoryDrive::new();
        put_empty_snapshot(&drive);
        run_sync(&m, &drive, &input(NOW, true)).await.unwrap();
        let snap = snapshot_of(&drive).await;
        assert!(snap.state[&id].read.as_ref().expect("folded").value);
        let again = run_sync(&m, &drive, &input(NOW, true)).await.unwrap();
        assert!(!again.written_files.contains(&SNAPSHOT.to_string()));
    }

    #[tokio::test]
    async fn a_phone_op_beats_the_epoch_stamp() {
        let m = setup();
        let id = add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        pre_v5_read(&m, id);
        let drive = MemoryDrive::new();
        put_ops(
            &drive,
            "phone",
            "2026-10-13T11:00:00.000Z",
            vec![op(
                id,
                Field::Read,
                false,
                "2026-10-13T11:00:00.000Z",
                "phone",
            )],
        );
        let report = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert_eq!(report.applied_ops, 1);
        assert!(!m.lock().unwrap().get_item(id).unwrap().unwrap().read);
        let r = snapshot_of(&drive).await.state[&id].read.clone().unwrap();
        assert!(!r.value);
        assert_eq!(r.device, "phone");
    }

    fn put_manifest(drive: &MemoryDrive, db_id: &str) {
        let manifest = Manifest {
            schema: SCHEMA,
            db_id: db_id.into(),
            written_at: "2026-10-01T00:00:00.000Z".into(),
            writer: "Old Mac".into(),
            files: BTreeMap::new(),
        };
        drive.put(MANIFEST, serde_json::to_vec(&manifest).unwrap());
    }

    #[tokio::test]
    async fn a_new_database_id_clears_old_sync_files_without_applying_them() {
        let m = setup();
        let id = add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        m.lock().unwrap().set_starred(id, true).unwrap();
        let drive = MemoryDrive::new();
        put_manifest(&drive, "old-db");
        let phone = put_ops(
            &drive,
            "phone",
            "2026-10-13T11:00:00.000Z",
            vec![op(
                id,
                Field::Read,
                true,
                "2999-01-01T00:00:00.000Z",
                "phone",
            )],
        );
        let old_mac = put_ops(
            &drive,
            "oldmac",
            "2026-10-12T11:00:00.000Z",
            vec![op(
                id,
                Field::Starred,
                false,
                "2999-01-01T00:00:00.000Z",
                "oldmac",
            )],
        );
        let mut stale = BTreeMap::new();
        apply_op(
            &mut stale,
            &op(id, Field::Read, true, "2999-01-01T00:00:00.000Z", "phone"),
        );
        drive.put(
            SNAPSHOT,
            gzip_json(&StateSnapshot {
                covers: vec!["ops-phone-x.json".into()],
                state: stale,
            })
            .unwrap(),
        );
        drive.put("day-2026-01-01.json.gz", b"old".to_vec());
        drive.put("digest-day-2026-01-01.json.gz", b"old".to_vec());
        drive.put("unknown-file.bin", b"old".to_vec());
        // Rows left from the old database's files.
        m.lock()
            .unwrap()
            .put_sync_file(&SyncFileRow {
                name: phone.clone(),
                kind: "in".into(),
                drive_id: Some("gone".into()),
                sha256: None,
                updated_at: NOW.into(),
                fingerprint: None,
            })
            .unwrap();

        let report = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert_eq!((report.imported_ops, report.applied_ops), (0, 0));
        assert!(!report.warnings.is_empty());
        let item = m.lock().unwrap().get_item(id).unwrap().unwrap();
        assert!(!item.read, "stale phone op not applied");
        assert!(item.starred, "stale old-Mac op not applied");
        let names = drive.names();
        assert!(!names.contains(&phone), "{names:?}");
        assert!(!names.contains(&old_mac), "{names:?}");
        assert!(m.lock().unwrap().sync_file(&phone).unwrap().is_none());
        assert!(m.lock().unwrap().sync_file(&old_mac).unwrap().is_none());
        let snap = snapshot_of(&drive).await;
        assert!(!snap.covers.contains(&"ops-phone-x.json".to_string()));
        assert!(snap.covers.iter().all(|c| c.starts_with("ops-mac-")));
        assert!(snap.state[&id].read.is_none());
        assert!(snap.state[&id].starred.as_ref().unwrap().value);
        assert_eq!(manifest_of(&drive).await.db_id, "db1");
        for gone in ["day-2026-01-01.json.gz", "digest-day-2026-01-01.json.gz"] {
            assert!(!names.contains(&gone.to_string()), "{gone} {names:?}");
        }
        // Not a sync file: the reset leaves it alone.
        assert!(names.contains(&"unknown-file.bin".to_string()), "{names:?}");
        let manifest = manifest_of(&drive).await;
        assert!(!manifest.files.contains_key("day-2026-01-01.json.gz"));
        assert!(!manifest.files.contains_key("digest-day-2026-01-01.json.gz"));
        assert!(
            names.iter().any(|n| n.starts_with("day-2026-10-")),
            "{names:?}"
        );
        assert!(names.iter().any(|n| n.starts_with("meta")), "{names:?}");
    }

    /// Files a person or the OS left in a NAS folder, and the leftovers of a check.
    const FOREIGN: [&str; 4] = [
        ".DS_Store",
        "infodesk-check-0a1b.txt",
        "notes.txt",
        "day-2026-10-13.json.gz.bak",
    ];

    fn put_foreign(drive: &MemoryDrive) {
        for name in FOREIGN {
            drive.put(name, format!("user {name}").into_bytes());
        }
    }

    async fn assert_foreign_untouched(drive: &Recording, downloads: &[String]) {
        for name in FOREIGN {
            assert_eq!(
                read_file(&drive.inner, name).await,
                Some(format!("user {name}").into_bytes()),
                "{name} kept as it was"
            );
            assert!(!downloads.contains(&name.to_string()), "{name} downloaded");
        }
        let manifest = manifest_of(&drive.inner).await;
        for name in FOREIGN {
            assert!(!manifest.files.contains_key(name), "{name} in manifest");
        }
    }

    #[tokio::test]
    async fn foreign_files_in_the_folder_are_never_touched() {
        // A normal first pass, then an unchanged one.
        let m = setup();
        add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        let drive = Recording::default();
        put_foreign(&drive.inner);
        run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert_foreign_untouched(&drive, &drive.take()).await;
        run_sync(&m, &drive, &input(NOW, true)).await.unwrap();
        assert_foreign_untouched(&drive, &drive.take()).await;

        // A folder that held another database: the reset deletes only sync files.
        let m = setup();
        add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        let drive = Recording::default();
        put_manifest(&drive.inner, "old-db");
        drive.inner.put("day-2026-01-01.json.gz", b"old".to_vec());
        put_foreign(&drive.inner);
        let report = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert!(!report.warnings.is_empty());
        assert!(
            !drive
                .inner
                .names()
                .contains(&"day-2026-01-01.json.gz".into())
        );
        assert_foreign_untouched(&drive, &drive.take()).await;
    }

    #[tokio::test]
    async fn the_manifest_is_downloaded_once_per_pass() {
        let m = setup();
        add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        let drive = Recording::default();
        run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        drive.take();
        let again = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert!(again.written_files.is_empty(), "{:?}", again.written_files);
        assert_eq!(drive.take(), vec![MANIFEST.to_string()]);
    }

    #[tokio::test]
    async fn compaction_skips_downloading_own_ops_but_deletes_them() {
        let m = setup();
        let id = add_item(&m, "https://a/1", "2026-10-13T01:00:00Z");
        let drive = Recording::default();
        run_sync(&m, &drive, &input(NOW, true)).await.unwrap();
        m.lock().unwrap().set_read(id, true).unwrap();
        run_sync(&m, &drive, &input("2026-10-13T12:01:00Z", false))
            .await
            .unwrap();
        let own: Vec<String> = drive
            .inner
            .names()
            .into_iter()
            .filter(|n| n.starts_with("ops-mac-"))
            .collect();
        assert_eq!(own.len(), 1);
        drive.take();
        let report = run_sync(&m, &drive, &input("2026-10-13T12:02:00Z", true))
            .await
            .unwrap();
        let downloads = drive.take();
        assert!(!downloads.contains(&own[0]), "{downloads:?}");
        assert!(report.deleted_files.contains(&own[0]));
        let snap = snapshot_of(&drive.inner).await;
        assert!(snap.covers.contains(&own[0]));
        assert!(snap.state[&id].read.as_ref().unwrap().value);
    }

    #[test]
    fn compaction_due_boundaries() {
        let tz = FixedOffset::east_opt(9 * 3600).unwrap();
        let t = NaiveTime::from_hms_opt(3, 0, 0).unwrap();
        // 03:00 JST on 2026-10-13 is 2026-10-12T18:00:00Z.
        let before = at("2026-10-12T17:59:59Z");
        let after = at("2026-10-12T18:00:01Z");
        assert!(!compaction_due(None, before, tz, t));
        assert!(compaction_due(None, after, tz, t));
        assert!(compaction_due(
            Some(at("2026-10-11T20:00:00Z")),
            after,
            tz,
            t
        ));
        assert!(!compaction_due(
            Some(at("2026-10-12T18:00:30Z")),
            at("2026-10-13T05:00:00Z"),
            tz,
            t
        ));
        assert!(compaction_due(
            Some(at("2026-10-12T18:00:30Z")),
            at("2026-10-13T18:00:01Z"),
            tz,
            t
        ));
    }
}
