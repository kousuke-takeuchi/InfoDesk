//! Reader pass, a port of the phone's `_pass` (mobile/lib/sync/engine.dart): reads the
//! folder another device writes. It never writes the manifest, meta, day, digest or
//! snapshot files and never deletes anything; its only remote writes are its own new
//! ops files. The store lock is never held across an await.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;

use chrono::{DateTime, FixedOffset, Utc};

use super::drive::{DriveStore, RemoteFile};
use super::engine::{
    MANIFEST, MAX_SEQ, MIME_JSON, OPS_PER_FILE, SyncError, local_ops, locked, ops_sort_key, st,
};
use super::format::{
    DayFile, DigestFile, FileKind, Manifest, Op, OpsFile, SCHEMA, StateSnapshot, gunzip_json,
    normalize_op, op_time, ops_file_name, parse_file_name, sha256_hex,
};
use crate::store::{Store, SyncFileRow};

pub const NO_DATA_MESSAGE: &str = "同期フォルダにまだデータがありません";
pub const NEWER_SCHEMA_MESSAGE: &str =
    "同期データの形式が新しくなっています。アプリを更新してください";
pub const MANIFEST_UNREADABLE_MESSAGE: &str = "manifest.json を読めませんでした";
/// The manifest's `writer` and `written_at`, kept for display.
pub const SETTING_READER_WRITER: &str = "sync.reader.writer";
pub const SETTING_READER_WRITTEN_AT: &str = "sync.reader.written_at";

#[derive(Debug, Clone)]
pub struct ReaderInput {
    pub device_id: String,
    pub now: DateTime<Utc>,
    pub tz: FixedOffset,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReaderReport {
    /// Day, digest and snapshot files imported in this pass.
    pub imported_files: usize,
    pub applied_ops: usize,
    pub uploaded_ops: usize,
    /// Japanese messages for the Sync tab. Nothing in them stopped the pass.
    pub warnings: Vec<String>,
}

/// A data file listed in the manifest whose hash differs from the recorded import.
struct Todo {
    name: String,
    kind: FileKind,
    sha256: String,
    id: String,
}

fn hash_mismatch(name: &str) -> String {
    format!("{name} のハッシュが一致しないため取り込みませんでした")
}

fn unreadable(name: &str) -> String {
    format!("{name} を読めませんでした")
}

fn in_row(name: &str, id: &str, sha: &str, now: DateTime<Utc>) -> SyncFileRow {
    SyncFileRow {
        name: name.to_string(),
        kind: "in".to_string(),
        drive_id: Some(id.to_string()),
        sha256: Some(sha.to_string()),
        updated_at: op_time(now),
        fingerprint: None,
    }
}

struct Pass<'a> {
    store: &'a Mutex<Store>,
    drive: &'a dyn DriveStore,
    input: &'a ReaderInput,
    remote: BTreeMap<String, RemoteFile>,
    report: ReaderReport,
}

impl Pass<'_> {
    /// Reads the manifest. `None` (with a warning) when there is none, it is
    /// unreadable, or its schema is not ours; the pass then touches nothing.
    async fn manifest(&mut self) -> Result<Option<Manifest>, SyncError> {
        let Some(f) = self.remote.get(MANIFEST).cloned() else {
            self.report.warnings.push(NO_DATA_MESSAGE.to_string());
            return Ok(None);
        };
        let bytes = self.drive.download(&f.id).await?;
        // The schema is read first, so a newer layout is reported as newer.
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            self.report
                .warnings
                .push(MANIFEST_UNREADABLE_MESSAGE.to_string());
            return Ok(None);
        };
        match value.get("schema").and_then(serde_json::Value::as_u64) {
            Some(s) if s == u64::from(SCHEMA) => {}
            Some(_) => {
                self.report.warnings.push(NEWER_SCHEMA_MESSAGE.to_string());
                return Ok(None);
            }
            None => {
                self.report
                    .warnings
                    .push(MANIFEST_UNREADABLE_MESSAGE.to_string());
                return Ok(None);
            }
        }
        match serde_json::from_value::<Manifest>(value) {
            Ok(m) => Ok(Some(m)),
            Err(_) => {
                self.report
                    .warnings
                    .push(MANIFEST_UNREADABLE_MESSAGE.to_string());
                Ok(None)
            }
        }
    }

    /// Files of the kind `want` accepts that the manifest lists, the folder
    /// holds, and whose hash differs from the recorded import.
    fn changed(
        &self,
        manifest: &Manifest,
        want: fn(&FileKind) -> bool,
    ) -> Result<Vec<Todo>, SyncError> {
        let done: HashMap<String, String> = locked(self.store, |s| s.sync_files("in"))?
            .into_iter()
            .filter_map(|r| r.sha256.map(|h| (r.name, h)))
            .collect();
        let mut out = Vec::new();
        for (name, entry) in &manifest.files {
            let (Some(kind), Some(file)) = (parse_file_name(name), self.remote.get(name)) else {
                continue;
            };
            if !want(&kind) || done.get(name) == Some(&entry.sha256) {
                continue;
            }
            out.push(Todo {
                name: name.clone(),
                kind,
                sha256: entry.sha256.clone(),
                id: file.id.clone(),
            });
        }
        Ok(out)
    }

    /// Downloads a data file and checks it against the manifest's hash.
    /// `None` (with a warning) on a mismatch.
    async fn fetch(&mut self, t: &Todo) -> Result<Option<Vec<u8>>, SyncError> {
        let bytes = self.drive.download(&t.id).await?;
        if sha256_hex(&bytes) != t.sha256 {
            self.report.warnings.push(hash_mismatch(&t.name));
            return Ok(None);
        }
        Ok(Some(bytes))
    }

    fn record(&self, t: &Todo) -> Result<(), SyncError> {
        let row = in_row(&t.name, &t.id, &t.sha256, self.input.now);
        locked(self.store, |s| s.put_sync_file(&row))
    }

    /// Imports every changed day file in one store call. When any of them
    /// failed, nothing is deleted and none is recorded, so all are retried.
    async fn import_days(&mut self, manifest: &Manifest) -> Result<(), SyncError> {
        let todo = self.changed(manifest, |k| matches!(k, FileKind::Day(_)))?;
        let mut parsed: Vec<DayFile> = Vec::new();
        let mut all_ok = true;
        for t in &todo {
            let FileKind::Day(date) = t.kind else {
                continue;
            };
            let Some(bytes) = self.fetch(t).await? else {
                all_ok = false;
                continue;
            };
            match gunzip_json::<DayFile>(&bytes) {
                Ok(f) if f.date == date.format("%Y-%m-%d").to_string() => parsed.push(f),
                _ => {
                    self.report.warnings.push(unreadable(&t.name));
                    all_ok = false;
                }
            }
        }
        if parsed.is_empty() {
            return Ok(());
        }
        let tz = self.input.tz;
        let result = locked(self.store, |s| s.import_days(&parsed, tz, all_ok))?;
        self.report.warnings.extend(result.skipped);
        self.report.imported_files += parsed.len();
        if all_ok {
            for t in &todo {
                self.record(t)?;
            }
        }
        Ok(())
    }

    async fn import_digests(&mut self, manifest: &Manifest) -> Result<(), SyncError> {
        for t in self.changed(manifest, |k| matches!(k, FileKind::Digest(..)))? {
            let FileKind::Digest(period, start) = t.kind else {
                continue;
            };
            let Some(bytes) = self.fetch(&t).await? else {
                continue;
            };
            let file = match gunzip_json::<DigestFile>(&bytes) {
                Ok(f)
                    if f.period == period.as_str()
                        && f.period_start == start.format("%Y-%m-%d").to_string() =>
                {
                    f
                }
                _ => {
                    self.report.warnings.push(unreadable(&t.name));
                    continue;
                }
            };
            let result = locked(self.store, |s| {
                s.import_digests(period, start, &file.entries)
            })?;
            self.report.warnings.extend(result.skipped);
            self.record(&t)?;
            self.report.imported_files += 1;
        }
        Ok(())
    }

    /// Applies the snapshot as ops: each stamp competes with the stored one.
    async fn import_snapshot(&mut self, manifest: &Manifest) -> Result<(), SyncError> {
        for t in self.changed(manifest, |k| matches!(k, FileKind::Snapshot))? {
            let Some(bytes) = self.fetch(&t).await? else {
                continue;
            };
            let Ok(snap) = gunzip_json::<StateSnapshot>(&bytes) else {
                self.report.warnings.push(unreadable(&t.name));
                continue;
            };
            let ops: Vec<Op> = local_ops(snap.state)
                .filter_map(|op| normalize_op(&op))
                .collect();
            let device = self.input.device_id.clone();
            self.report.applied_ops += locked(self.store, |s| s.apply_remote_ops(&ops, &device))?;
            self.record(&t)?;
            self.report.imported_files += 1;
        }
        Ok(())
    }

    /// Applies every ops file not yet imported, oldest first, this device's
    /// own included. A file of another database is recorded, never applied.
    async fn apply_ops_files(&mut self, manifest: &Manifest) -> Result<(), SyncError> {
        let done: HashSet<String> = locked(self.store, |s| s.sync_files("in"))?
            .into_iter()
            .map(|r| r.name)
            .collect();
        let mut names: Vec<String> = self
            .remote
            .keys()
            .filter(|n| {
                matches!(parse_file_name(n), Some(FileKind::Ops { .. })) && !done.contains(*n)
            })
            .cloned()
            .collect();
        names.sort_by_key(|n| ops_sort_key(n));
        for name in names {
            let id = self.remote[&name].id.clone();
            let bytes = self.drive.download(&id).await?;
            match serde_json::from_slice::<OpsFile>(&bytes) {
                Ok(file) if file.db_id.as_ref().is_none_or(|db| *db == manifest.db_id) => {
                    let ops: Vec<Op> = file.ops.iter().filter_map(normalize_op).collect();
                    let device = self.input.device_id.clone();
                    self.report.applied_ops +=
                        locked(self.store, |s| s.apply_remote_ops(&ops, &device))?;
                }
                Ok(_) => {}
                Err(_) => self.report.warnings.push(unreadable(&name)),
            }
            let row = in_row(&name, &id, &sha256_hex(&bytes), self.input.now);
            locked(self.store, |s| s.put_sync_file(&row))?;
        }
        Ok(())
    }

    /// Uploads this device's unsent marks as new ops files tagged with the
    /// manifest's `db_id`, then marks them sent. Each file is recorded as
    /// imported so it is not downloaded back.
    async fn upload_own_ops(&mut self, manifest: &Manifest) -> Result<(), SyncError> {
        let device = self.input.device_id.clone();
        let pending = locked(self.store, |s| s.pending_local_ops(&device))?;
        if pending.is_empty() {
            return Ok(());
        }
        let chunk = OPS_PER_FILE.max(pending.len().div_ceil(MAX_SEQ));
        for (i, part) in pending.chunks(chunk).enumerate() {
            let name = ops_file_name(&device, self.input.now, (i + 1) as u32);
            let file = OpsFile {
                device: device.clone(),
                db_id: Some(manifest.db_id.clone()),
                ops: part.iter().map(|(_, op)| op.clone()).collect(),
            };
            let bytes = st(serde_json::to_vec(&file).map_err(Into::into))?;
            let sha = sha256_hex(&bytes);
            let created = self.drive.create(&name, bytes, MIME_JSON).await?;
            let ids: Vec<i64> = part.iter().map(|(id, _)| *id).collect();
            let row = in_row(&name, &created.id, &sha, self.input.now);
            locked(self.store, |s| {
                s.put_sync_file(&row)?;
                s.mark_ops_uploaded(&ids)
            })?;
            self.report.uploaded_ops += ids.len();
        }
        Ok(())
    }
}

pub async fn run_reader_sync(
    store: &Mutex<Store>,
    drive: &dyn DriveStore,
    input: &ReaderInput,
) -> Result<ReaderReport, SyncError> {
    // Only names of the sync format are ever read (a NAS folder may hold other files).
    let remote = drive
        .list()
        .await?
        .into_iter()
        .filter(|f| parse_file_name(&f.name).is_some())
        .map(|f| (f.name.clone(), f))
        .collect();
    let mut pass = Pass {
        store,
        drive,
        input,
        remote,
        report: ReaderReport::default(),
    };
    let Some(manifest) = pass.manifest().await? else {
        return Ok(pass.report);
    };
    locked(store, |s| {
        s.set_setting(SETTING_READER_WRITER, &manifest.writer)?;
        s.set_setting(SETTING_READER_WRITTEN_AT, &manifest.written_at)?;
        if s.get_setting("sync.db_id")?.as_deref() != Some(manifest.db_id.as_str()) {
            s.reset_for_db_id(&manifest.db_id)?;
        }
        Ok(())
    })?;
    // Meta is never imported: the Mac keeps its own sources and topics.
    pass.import_days(&manifest).await?;
    pass.import_digests(&manifest).await?;
    pass.import_snapshot(&manifest).await?;
    pass.apply_ops_files(&manifest).await?;
    pass.upload_own_ops(&manifest).await?;
    Ok(pass.report)
}
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use chrono::{FixedOffset, NaiveDate};

    use super::*;
    use crate::model::{Category, DigestScope, ItemFilter, NewItem, Period};
    use crate::period::period_start;
    use crate::store::{NewDigest, RedateRange};
    use crate::sync::drive::{DriveError, MemoryDrive, RemoteFile};
    use crate::sync::engine::{SyncInput, run_sync};
    use crate::sync::format::{DayFile, Field, Op, OpsFile, gzip_json, ops_file_name};
    use crate::testutil::{at, new_item, store_with_source};

    pub(super) const NOW: &str = "2026-10-13T12:00:00Z";

    pub(super) fn tz() -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }

    /// The server: the only writer.
    pub(super) fn writer() -> Mutex<Store> {
        let s = store_with_source(Category::TechJp).0;
        s.set_setting("sync.enabled", "true").unwrap();
        s.set_setting("sync.device_id", "server").unwrap();
        Mutex::new(s)
    }

    pub(super) fn writer_input(now: &str) -> SyncInput {
        SyncInput {
            device_id: "server".into(),
            db_id: "db1".into(),
            writer: "infodesk-server".into(),
            now: at(now),
            tz: tz(),
            compact: true,
        }
    }

    /// The Mac: same database id as the server, switched to reader.
    pub(super) fn reader() -> Mutex<Store> {
        let s = store_with_source(Category::TechJp).0;
        s.set_setting("sync.enabled", "true").unwrap();
        s.set_setting("sync.db_id", "db1").unwrap();
        s.switch_to_reader().unwrap();
        Mutex::new(s)
    }

    pub(super) fn reader_input(m: &Mutex<Store>, now: &str) -> ReaderInput {
        let device_id = m
            .lock()
            .unwrap()
            .get_setting("sync.device_id")
            .unwrap()
            .unwrap();
        ReaderInput {
            device_id,
            now: at(now),
            tz: tz(),
        }
    }

    pub(super) fn add(m: &Mutex<Store>, item: NewItem, fetched: &str) -> i64 {
        let s = m.lock().unwrap();
        let src = s.list_sources().unwrap().remove(0);
        let url = item.url.clone();
        s.upsert_items(&src, &[item], &at(fetched)).unwrap();
        s.query_items(&ItemFilter {
            limit: 1000,
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .find(|i| i.url == url)
        .unwrap()
        .id
    }

    pub(super) fn add_item(m: &Mutex<Store>, url: &str, fetched: &str) -> i64 {
        add(m, new_item(url, "t", "", None), fetched)
    }

    /// (id, url, title, fetched_at, ai_summary, read, starred), by id.
    #[allow(clippy::type_complexity)]
    pub(super) fn rows(
        m: &Mutex<Store>,
    ) -> Vec<(i64, String, String, String, Option<String>, bool, bool)> {
        let mut v: Vec<_> = m
            .lock()
            .unwrap()
            .query_items(&ItemFilter {
                limit: 1000,
                ..Default::default()
            })
            .unwrap()
            .into_iter()
            .map(|i| {
                (
                    i.id,
                    i.url,
                    i.title,
                    i.fetched_at.to_rfc3339(),
                    i.ai_summary,
                    i.read,
                    i.starred,
                )
            })
            .collect();
        v.sort();
        v
    }

    pub(super) fn urls(m: &Mutex<Store>) -> Vec<String> {
        rows(m).into_iter().map(|r| r.1).collect()
    }

    fn digests_of(
        m: &Mutex<Store>,
        p: Period,
        start: NaiveDate,
    ) -> Vec<(String, String, Vec<i64>, String, String)> {
        m.lock()
            .unwrap()
            .list_digests(p, start)
            .unwrap()
            .into_iter()
            .map(|d| {
                (
                    d.scope.key(),
                    d.body,
                    d.item_ids,
                    d.provider,
                    d.created_at.to_rfc3339(),
                )
            })
            .collect()
    }

    fn save(m: &Mutex<Store>, p: Period, start: NaiveDate, body: &str, ids: Vec<i64>) {
        m.lock()
            .unwrap()
            .save_digest_done(
                &NewDigest {
                    period: p,
                    period_start: start,
                    scope: DigestScope::Category(Category::TechJp),
                    body: body.into(),
                    item_ids: ids,
                    provider: "claude_api:x".into(),
                },
                &at(NOW),
            )
            .unwrap();
    }

    pub(super) async fn files(drive: &MemoryDrive) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        for f in drive.list().await.unwrap() {
            out.insert(f.name.clone(), drive.download(&f.id).await.unwrap());
        }
        out
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    /// Counts downloads and records every write.
    #[derive(Default)]
    pub(super) struct Spy {
        pub(super) inner: MemoryDrive,
        pub(super) downloads: AtomicUsize,
        pub(super) writes: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl DriveStore for Spy {
        async fn list(&self) -> Result<Vec<RemoteFile>, DriveError> {
            self.inner.list().await
        }
        async fn download(&self, id: &str) -> Result<Vec<u8>, DriveError> {
            self.downloads.fetch_add(1, Ordering::SeqCst);
            self.inner.download(id).await
        }
        async fn create(
            &self,
            name: &str,
            bytes: Vec<u8>,
            mime: &str,
        ) -> Result<RemoteFile, DriveError> {
            self.writes.lock().unwrap().push(format!("create {name}"));
            self.inner.create(name, bytes, mime).await
        }
        async fn update(
            &self,
            id: &str,
            bytes: Vec<u8>,
            mime: &str,
        ) -> Result<RemoteFile, DriveError> {
            self.writes.lock().unwrap().push(format!("update {id}"));
            self.inner.update(id, bytes, mime).await
        }
        async fn delete(&self, id: &str) -> Result<(), DriveError> {
            self.writes.lock().unwrap().push(format!("delete {id}"));
            self.inner.delete(id).await
        }
    }

    #[tokio::test]
    async fn round_trip_matches_the_writer_items_digests_and_marks() {
        let w = writer();
        let a1 = add_item(&w, "https://a/1", "2026-10-12T03:00:00Z");
        let a2 = add_item(&w, "https://a/2", "2026-10-13T03:00:00Z");
        w.lock().unwrap().set_read(a1, true).unwrap();
        w.lock().unwrap().set_starred(a2, true).unwrap();
        w.lock().unwrap().set_ai_summary(a2, "要約").unwrap();
        let week = period_start(Period::Week, day("2026-10-13"));
        save(&w, Period::Day, day("2026-10-13"), "日次", vec![a2]);
        save(&w, Period::Week, week, "週次", vec![a1, a2]);
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();

        let r = reader();
        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();

        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(report.imported_files >= 4, "{report:?}");
        assert_eq!(rows(&r), rows(&w));
        assert_eq!(
            digests_of(&r, Period::Day, day("2026-10-13")),
            digests_of(&w, Period::Day, day("2026-10-13"))
        );
        assert_eq!(
            digests_of(&r, Period::Week, week),
            digests_of(&w, Period::Week, week)
        );
        let s = r.lock().unwrap();
        assert_eq!(
            s.get_setting(SETTING_READER_WRITER).unwrap().as_deref(),
            Some("infodesk-server")
        );
        assert!(s.get_setting(SETTING_READER_WRITTEN_AT).unwrap().is_some());
    }

    #[tokio::test]
    async fn a_copied_database_is_updated_in_place() {
        let w = writer();
        let id = add_item(&w, "https://a/1", "2026-10-13T01:00:00Z");
        w.lock()
            .unwrap()
            .set_ai_summary(id, "サーバーの要約")
            .unwrap();
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();
        // The Mac had the same article (same id) and read it before the switch.
        let r = reader();
        assert_eq!(add_item(&r, "https://a/1", "2026-10-13T01:00:00Z"), id);
        r.lock().unwrap().set_read(id, true).unwrap();

        run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();

        let item = r.lock().unwrap().get_item(id).unwrap().unwrap();
        assert_eq!(item.ai_summary.as_deref(), Some("サーバーの要約"));
        assert!(item.read, "the Mac's own mark stays");
        assert_eq!(urls(&r), vec!["https://a/1"]);
    }

    #[tokio::test]
    async fn redated_articles_move_days_on_the_reader_and_keep_marks() {
        // Review Focus 1
        let w = writer();
        let moved = add(
            &w,
            NewItem {
                published_at: Some(at("2026-10-10T08:00:00Z")),
                ..new_item("https://a/moved", "t", "", None)
            },
            "2026-10-13T03:00:00Z",
        );
        let stays = add_item(&w, "https://a/stays", "2026-10-13T04:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();
        let r = reader();
        run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();
        r.lock().unwrap().set_starred(moved, true).unwrap();

        let range = RedateRange {
            fetched_on: day("2026-10-13"),
            from: day("2026-10-10"),
            to: day("2026-10-12"),
        };
        w.lock().unwrap().redate_apply(&range, tz()).unwrap();
        run_sync(&w, &drive, &writer_input("2026-10-13T12:10:00Z"))
            .await
            .unwrap();
        let report = run_reader_sync(&r, &drive, &reader_input(&r, "2026-10-13T12:11:00Z"))
            .await
            .unwrap();

        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let s = r.lock().unwrap();
        let m = s.get_item(moved).unwrap().unwrap();
        assert_eq!(m.fetched_at, at("2026-10-10T08:00:00Z"));
        assert!(m.starred, "the mark survives the move");
        assert!(s.get_item(stays).unwrap().is_some());
        drop(s);
        assert_eq!(urls(&r).len(), 2);
    }

    #[tokio::test]
    async fn a_new_database_id_wipes_items_and_digests_but_keeps_sources_topics_and_settings() {
        let w = writer();
        add_item(&w, "https://server/1", "2026-10-13T01:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(
            &w,
            &drive,
            &SyncInput {
                db_id: "db2".into(),
                ..writer_input(NOW)
            },
        )
        .await
        .unwrap();
        let r = reader();
        add_item(&r, "https://mac-only/1", "2026-10-13T02:00:00Z");
        save(&r, Period::Day, day("2026-10-12"), "手元だけ", vec![]);
        r.lock()
            .unwrap()
            .add_topic("AI", &["AI".to_string()])
            .unwrap();
        r.lock()
            .unwrap()
            .set_setting("sync.device_name", "Mac")
            .unwrap();

        run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();

        assert_eq!(urls(&r), vec!["https://server/1"]);
        assert!(digests_of(&r, Period::Day, day("2026-10-12")).is_empty());
        let s = r.lock().unwrap();
        assert_eq!(s.get_setting("sync.db_id").unwrap().as_deref(), Some("db2"));
        assert_eq!(
            s.get_setting("sync.device_name").unwrap().as_deref(),
            Some("Mac")
        );
        assert_eq!(s.list_topics().unwrap().len(), 1);
        assert_eq!(s.list_sources().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn no_manifest_or_a_newer_schema_changes_nothing() {
        let r = reader();
        let drive = MemoryDrive::new();
        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();
        assert_eq!(report.warnings, vec![NO_DATA_MESSAGE.to_string()]);
        assert!(drive.names().is_empty());

        drive.put(
            "manifest.json",
            serde_json::to_vec(&serde_json::json!({"schema": 2, "layout": "new"})).unwrap(),
        );
        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();
        assert_eq!(report.warnings, vec![NEWER_SCHEMA_MESSAGE.to_string()]);
        assert_eq!(
            r.lock()
                .unwrap()
                .get_setting("sync.db_id")
                .unwrap()
                .as_deref(),
            Some("db1")
        );
        assert_eq!(drive.names(), vec!["manifest.json"]);

        drive.put("manifest.json", b"{not json".to_vec());
        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();
        assert_eq!(
            report.warnings,
            vec![MANIFEST_UNREADABLE_MESSAGE.to_string()]
        );
    }

    #[tokio::test]
    async fn a_file_whose_hash_differs_is_skipped_and_imported_later() {
        // Review Focus 3
        let w = writer();
        add_item(&w, "https://a/1", "2026-10-12T03:00:00Z");
        add_item(&w, "https://a/2", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();
        let good = files(&drive).await["day-2026-10-13.json.gz"].clone();
        drive.put(
            "day-2026-10-13.json.gz",
            gzip_json(&DayFile {
                date: "2026-10-13".into(),
                items: vec![],
            })
            .unwrap(),
        );
        let r = reader();

        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("day-2026-10-13.json.gz") && w.contains("ハッシュ")),
            "{:?}",
            report.warnings
        );
        assert_eq!(urls(&r), vec!["https://a/1"]);

        drive.put("day-2026-10-13.json.gz", good);
        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(urls(&r), vec!["https://a/1", "https://a/2"]);
    }

    #[tokio::test]
    async fn an_unchanged_folder_downloads_only_the_manifest() {
        let w = writer();
        add_item(&w, "https://a/1", "2026-10-13T03:00:00Z");
        let spy = Spy::default();
        run_sync(&w, &spy.inner, &writer_input(NOW)).await.unwrap();
        let r = reader();
        run_reader_sync(&r, &spy, &reader_input(&r, NOW))
            .await
            .unwrap();
        spy.downloads.store(0, Ordering::SeqCst);
        let report = run_reader_sync(&r, &spy, &reader_input(&r, NOW))
            .await
            .unwrap();
        assert_eq!(spy.downloads.load(Ordering::SeqCst), 1);
        assert_eq!((report.imported_files, report.applied_ops), (0, 0));
    }

    #[tokio::test]
    async fn phone_ops_are_applied_once_and_ops_of_another_database_never() {
        let w = writer();
        let id = add_item(&w, "https://a/1", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();
        let op = |field, device: &str| Op {
            item_id: id,
            field,
            value: true,
            at: "2026-10-13T11:00:00.000Z".into(),
            device: device.into(),
        };
        for (device, db, field) in [
            ("phone", "db1", Field::Starred),
            ("oldphone", "db0", Field::Read),
        ] {
            let file = OpsFile {
                device: device.into(),
                db_id: Some(db.into()),
                ops: vec![op(field, device)],
            };
            drive.put(
                &ops_file_name(device, at("2026-10-13T11:00:00Z"), 1),
                serde_json::to_vec(&file).unwrap(),
            );
        }
        let r = reader();
        let report = run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();
        assert_eq!(report.applied_ops, 1);
        let item = r.lock().unwrap().get_item(id).unwrap().unwrap();
        assert!(item.starred);
        assert!(!item.read, "an op of another database is never applied");
        let again = run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();
        assert_eq!(again.applied_ops, 0);
    }

    #[tokio::test]
    async fn reader_marks_reach_the_writer() {
        let w = writer();
        let id = add_item(&w, "https://a/1", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();
        let r = reader();
        run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();
        r.lock().unwrap().set_starred(id, true).unwrap();

        let input = reader_input(&r, "2026-10-13T12:01:00Z");
        assert_ne!(
            input.device_id, "server",
            "the writer must not skip it as its own"
        );
        let report = run_reader_sync(&r, &drive, &input).await.unwrap();
        assert_eq!(report.uploaded_ops, 1);
        assert!(
            r.lock()
                .unwrap()
                .pending_local_ops(&input.device_id)
                .unwrap()
                .is_empty()
        );
        let mine = format!("ops-{}-", input.device_id);
        assert!(
            drive.names().iter().any(|n| n.starts_with(&mine)),
            "{:?}",
            drive.names()
        );

        let wr = run_sync(&w, &drive, &writer_input("2026-10-13T12:02:00Z"))
            .await
            .unwrap();
        assert_eq!(wr.applied_ops, 1);
        assert!(w.lock().unwrap().get_item(id).unwrap().unwrap().starred);
        // The writer folded the batch into the snapshot and deleted it; the star stays.
        run_reader_sync(&r, &drive, &reader_input(&r, "2026-10-13T12:03:00Z"))
            .await
            .unwrap();
        assert!(r.lock().unwrap().get_item(id).unwrap().unwrap().starred);
    }

    #[tokio::test]
    async fn the_reader_writes_nothing_but_its_own_new_ops_files() {
        let w = writer();
        let id = add_item(&w, "https://a/1", "2026-10-13T03:00:00Z");
        save(&w, Period::Day, day("2026-10-13"), "日次", vec![id]);
        let spy = Spy::default();
        run_sync(&w, &spy.inner, &writer_input(NOW)).await.unwrap();
        let before = files(&spy.inner).await;
        let r = reader();

        run_reader_sync(&r, &spy, &reader_input(&r, NOW))
            .await
            .unwrap();
        r.lock().unwrap().set_read(id, true).unwrap();
        let input = reader_input(&r, "2026-10-13T12:01:00Z");
        run_reader_sync(&r, &spy, &input).await.unwrap();
        run_reader_sync(&r, &spy, &reader_input(&r, "2026-10-13T12:02:00Z"))
            .await
            .unwrap();

        let writes = spy.writes.lock().unwrap().clone();
        assert_eq!(writes.len(), 1, "{writes:?}");
        assert!(
            writes[0].starts_with(&format!("create ops-{}-", input.device_id)),
            "{writes:?}"
        );
        assert!(
            writes
                .iter()
                .all(|w| !w.starts_with("update ") && !w.starts_with("delete ")),
            "{writes:?}"
        );
        let after = files(&spy.inner).await;
        for (name, bytes) in &before {
            assert_eq!(after.get(name), Some(bytes), "{name} unchanged");
        }
        assert_eq!(after.len(), before.len() + 1);
    }

    #[tokio::test]
    async fn a_failed_upload_keeps_the_marks_for_the_next_pass() {
        let w = writer();
        let id = add_item(&w, "https://a/1", "2026-10-13T03:00:00Z");
        let drive = MemoryDrive::new();
        run_sync(&w, &drive, &writer_input(NOW)).await.unwrap();
        let r = reader();
        run_reader_sync(&r, &drive, &reader_input(&r, NOW))
            .await
            .unwrap();
        r.lock().unwrap().set_read(id, true).unwrap();
        *drive.fail_on_name.lock().unwrap() = Some("ops-".into());

        let input = reader_input(&r, "2026-10-13T12:01:00Z");
        let err = run_reader_sync(&r, &drive, &input).await.unwrap_err();
        assert!(
            matches!(err, SyncError::Drive(DriveError::Retryable(_))),
            "{err:?}"
        );
        assert_eq!(
            r.lock()
                .unwrap()
                .pending_local_ops(&input.device_id)
                .unwrap()
                .len(),
            1
        );

        let report = run_reader_sync(&r, &drive, &reader_input(&r, "2026-10-13T12:02:00Z"))
            .await
            .unwrap();
        assert_eq!(report.uploaded_ops, 1);
    }
}
