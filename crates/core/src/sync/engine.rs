//! One sync pass: import remote ops, upload local ops, write changed files,
//! compact, apply retention, and write the manifest last.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Mutex;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};

use super::drive::{DriveError, DriveStore, RemoteFile};
use super::format::{
    CategoryMeta, DayFile, DigestFile, DigestRecord, FileEntry, FileKind, ItemState, Manifest,
    MetaFile, Op, OpsFile, SCHEMA, SourceMeta, StateSnapshot, TopicMeta, apply_op, day_file_name,
    digest_file_name, gunzip_json, gzip_json, normalize_op, op_time, ops_file_name,
    parse_file_name, sha256_hex,
};
use crate::model::{Category, DigestStatus, Period};
use crate::period::period_start;
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

struct Output {
    name: String,
    bytes: Vec<u8>,
}

struct Built {
    outputs: Vec<Output>,
    /// Ids of items fetched inside the day retention window.
    retained_ids: HashSet<i64>,
}

fn date_str(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn build_outputs(store: &Store, input: &SyncInput) -> crate::Result<Built> {
    let today = input.now.with_timezone(&input.tz).date_naive();
    let day_cut = today - Duration::days(DAY_RETENTION - 1);
    let long_cut = today - Duration::days(LONG_RETENTION - 1);
    let mut outputs = Vec::new();

    let meta = MetaFile {
        categories: Category::ALL
            .iter()
            .map(|c| CategoryMeta {
                id: c.as_str().to_string(),
                label: c.label_ja().to_string(),
            })
            .collect(),
        topics: store
            .list_topics()?
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
    outputs.push(Output {
        name: META.to_string(),
        bytes: super::format::gzip_json(&meta)?,
    });

    let mut retained_ids = HashSet::new();
    for date in store.days_with_items(day_cut, input.tz)? {
        let items = store.items_fetched_on(date, input.tz)?;
        retained_ids.extend(items.iter().map(|i| i.id));
        let file = DayFile {
            date: date_str(date),
            items,
        };
        outputs.push(Output {
            name: day_file_name(date),
            bytes: gzip_json(&file)?,
        });
    }

    let mut starts: Vec<(Period, NaiveDate)> = Vec::new();
    for i in 0..DAY_RETENTION {
        starts.push((Period::Day, day_cut + Duration::days(i)));
    }
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
    for (period, start) in starts {
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
            continue;
        }
        let stats = store.digest_stats(period, start, input.tz, None)?;
        let file = DigestFile {
            period: period.as_str().to_string(),
            period_start: date_str(start),
            stats: Some(serde_json::to_value(stats)?),
            entries,
        };
        outputs.push(Output {
            name: digest_file_name(period, start),
            bytes: gzip_json(&file)?,
        });
    }
    Ok(Built {
        outputs,
        retained_ids,
    })
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

struct Pass<'a> {
    store: &'a Mutex<Store>,
    drive: &'a dyn DriveStore,
    now_s: String,
    remote: BTreeMap<String, RemoteFile>,
    hashes: BTreeMap<String, String>,
    report: SyncReport,
}

impl Pass<'_> {
    /// Writes `bytes` unless the recorded hash already matches the Drive copy.
    async fn write_out(
        &mut self,
        name: &str,
        bytes: Vec<u8>,
        mime: &str,
    ) -> Result<bool, SyncError> {
        let sha = sha256_hex(&bytes);
        let existing = self.remote.get(name).cloned();
        let row = locked(self.store, |s| s.sync_file(name))?;
        if let (Some(f), Some(r)) = (&existing, &row)
            && r.kind == "out"
            && r.sha256.as_deref() == Some(sha.as_str())
            && r.drive_id.as_deref() == Some(f.id.as_str())
        {
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
        };
        locked(self.store, |s| s.put_sync_file(&record))?;
        self.remote.insert(name.to_string(), file);
        self.hashes.insert(name.to_string(), sha);
        self.report.written_files.push(name.to_string());
        Ok(true)
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

    async fn download_ops(&self, name: &str) -> Result<(Vec<u8>, Option<OpsFile>), SyncError> {
        let id = self.remote[name].id.clone();
        let bytes = self.drive.download(&id).await?;
        let parsed = serde_json::from_slice::<OpsFile>(&bytes).ok();
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
                applied = locked(self.store, |s| {
                    let mut n = 0;
                    for op in &ops {
                        if s.apply_remote_op(op, &input.device_id)? {
                            n += 1;
                        }
                    }
                    Ok(n)
                })?;
            }
            let row = SyncFileRow {
                name: name.clone(),
                kind: "in".to_string(),
                drive_id: Some(self.remote[&name].id.clone()),
                sha256: Some(sha.clone()),
                updated_at: self.now_s.clone(),
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
            self.write_out(&name, bytes, MIME_JSON).await?;
            let ids: Vec<i64> = part.iter().map(|(id, _)| *id).collect();
            locked(self.store, |s| s.mark_ops_uploaded(&ids))?;
            self.report.uploaded_ops += ids.len();
        }
        Ok(())
    }

    async fn write_outputs(&mut self, input: &SyncInput) -> Result<HashSet<i64>, SyncError> {
        let built = locked(self.store, |s| build_outputs(s, input))?;
        for out in built.outputs {
            self.write_out(&out.name, out.bytes, MIME_GZ).await?;
        }
        Ok(built.retained_ids)
    }

    async fn compact(
        &mut self,
        input: &SyncInput,
        retained: &HashSet<i64>,
    ) -> Result<(), SyncError> {
        let cutoff = op_time(input.now - Duration::days(OPS_RETENTION_DAYS));
        locked(self.store, |s| s.prune_uploaded_ops(&cutoff))?;

        let names = self.ops_names();
        if names.is_empty() {
            return Ok(());
        }
        let mut state: BTreeMap<i64, ItemState> = match self.remote.get(SNAPSHOT).cloned() {
            Some(f) => {
                let bytes = self.drive.download(&f.id).await?;
                let snap: StateSnapshot = gunzip_json(&bytes)
                    .map_err(|e| SyncError::Store(format!("unreadable snapshot: {e}")))?;
                snap.state
            }
            None => BTreeMap::new(),
        };
        let mut covers = Vec::new();
        for name in &names {
            let (_, parsed) = self.download_ops(name).await?;
            let Some(file) = parsed else { continue };
            for op in file.ops.iter().filter_map(normalize_op) {
                apply_op(&mut state, &op);
            }
            covers.push(name.clone());
        }
        let ids: Vec<i64> = retained.iter().copied().collect();
        let local = locked(self.store, |s| {
            s.local_state_stamps(Some(&ids), &input.device_id)
        })?;
        for (item_id, item) in local {
            for (field, stamp) in [
                (super::format::Field::Read, item.read),
                (super::format::Field::Starred, item.starred),
            ] {
                if let Some(stamp) = stamp {
                    apply_op(
                        &mut state,
                        &Op {
                            item_id,
                            field,
                            value: stamp.value,
                            at: stamp.at,
                            device: stamp.device,
                        },
                    );
                }
            }
        }
        state.retain(|id, _| retained.contains(id));
        let snapshot = StateSnapshot {
            covers: covers.clone(),
            state,
        };
        self.write_out(SNAPSHOT, st(gzip_json(&snapshot))?, MIME_GZ)
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
        let bytes = st(serde_json::to_vec(&manifest).map_err(Into::into))?;
        let file = match self.remote.get(MANIFEST).cloned() {
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
    let listing = drive.list().await?;
    let mut pass = Pass {
        store,
        drive,
        now_s: op_time(input.now),
        remote: listing.into_iter().map(|f| (f.name.clone(), f)).collect(),
        hashes: BTreeMap::new(),
        report: SyncReport::default(),
    };
    pass.import_remote_ops(input).await?;
    pass.upload_local_ops(input).await?;
    let retained = pass.write_outputs(input).await?;
    if input.compact {
        pass.compact(input, &retained).await?;
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
        Mutex::new(store_with_source(Category::TechJp).0)
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
    async fn second_sync_without_changes_writes_only_the_manifest() {
        let m = setup();
        add_item(&m, "https://a/1", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        let again = run_sync(&m, &drive, &input(NOW, false)).await.unwrap();
        assert_eq!(again.written_files, vec!["manifest.json"]);
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
            "manifest.json",
        ] {
            assert!(names.contains(&n.to_string()), "{n} in {names:?}");
        }
        assert_eq!(names.len(), 4, "no duplicates: {names:?}");
        assert_eq!(report.written_files.len(), 3 - partial + 1);
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
