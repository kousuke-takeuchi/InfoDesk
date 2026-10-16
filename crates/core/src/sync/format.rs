use std::collections::BTreeMap;
use std::io::{Read, Write};

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, Timelike, Utc};
use flate2::{Compression, GzBuilder, read::GzDecoder};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::Result;
use crate::model::Period;

pub const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Manifest {
    pub schema: u32,
    pub db_id: String,
    pub written_at: String,
    pub writer: String,
    pub files: BTreeMap<String, FileEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileEntry {
    pub sha256: String,
    pub size: u64,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CategoryMeta {
    pub id: String,
    pub label: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TopicMeta {
    pub id: i64,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SourceMeta {
    pub id: i64,
    pub name: String,
    pub category: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MetaFile {
    pub categories: Vec<CategoryMeta>,
    pub topics: Vec<TopicMeta>,
    pub sources: Vec<SourceMeta>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ItemRecord {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub excerpt: String,
    pub ai_summary: Option<String>,
    pub image_url: Option<String>,
    pub source_id: Option<i64>,
    pub source_name: String,
    pub category: String,
    pub score: Option<i64>,
    pub published_at: Option<String>,
    pub fetched_at: String,
    pub topic_ids: Vec<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DayFile {
    pub date: String,
    pub items: Vec<ItemRecord>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DigestRecord {
    pub scope: serde_json::Value,
    pub body: String,
    pub status: String,
    pub item_ids: Vec<i64>,
    pub provider: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DigestFile {
    pub period: String,
    pub period_start: String,
    pub stats: Option<serde_json::Value>,
    pub entries: Vec<DigestRecord>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Read,
    Starred,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Op {
    pub item_id: i64,
    pub field: Field,
    pub value: bool,
    pub at: String,
    pub device: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OpsFile {
    pub device: String,
    /// The `db_id` of the manifest the writer saw. A reader whose database
    /// differs skips the file: its item ids name other articles. Absent in
    /// files written before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub db_id: Option<String>,
    pub ops: Vec<Op>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Stamp {
    pub value: bool,
    pub at: String,
    pub device: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ItemState {
    pub read: Option<Stamp>,
    pub starred: Option<Stamp>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StateSnapshot {
    pub covers: Vec<String>,
    pub state: BTreeMap<i64, ItemState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    Manifest,
    Meta,
    Day(NaiveDate),
    Digest(Period, NaiveDate),
    Snapshot,
    Ops { device: String },
}

const DATE_FMT: &str = "%Y-%m-%d";
const OPS_STAMP_LEN: usize = "YYYYMMDDTHHMMSSmmmZ".len();

pub fn day_file_name(d: NaiveDate) -> String {
    format!("day-{}.json.gz", d.format(DATE_FMT))
}

pub fn digest_file_name(p: Period, start: NaiveDate) -> String {
    format!("digest-{}-{}.json.gz", p.as_str(), start.format(DATE_FMT))
}

/// Callers keep `seq` within 1 to 9999 so the name stays 4 digits.
pub fn ops_file_name(device: &str, at: DateTime<Utc>, seq: u32) -> String {
    debug_assert!(seq <= 9999);
    format!(
        "ops-{device}-{:04}{:02}{:02}T{:02}{:02}{:02}{:03}Z-{seq:04}.json",
        at.year(),
        at.month(),
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
        at.timestamp_subsec_millis().min(999),
    )
}

pub fn parse_file_name(name: &str) -> Option<FileKind> {
    match name {
        "manifest.json" => return Some(FileKind::Manifest),
        "meta.json.gz" => return Some(FileKind::Meta),
        "state-snapshot.json.gz" => return Some(FileKind::Snapshot),
        _ => {}
    }
    if let Some(rest) = name
        .strip_prefix("day-")
        .and_then(|r| r.strip_suffix(".json.gz"))
    {
        let d = NaiveDate::parse_from_str(rest, DATE_FMT).ok()?;
        return (day_file_name(d) == name).then_some(FileKind::Day(d));
    }
    if let Some(rest) = name
        .strip_prefix("digest-")
        .and_then(|r| r.strip_suffix(".json.gz"))
    {
        let (p, date) = rest.split_once('-')?;
        let period = Period::parse(p)?;
        let date = NaiveDate::parse_from_str(date, DATE_FMT).ok()?;
        return (digest_file_name(period, date) == name).then_some(FileKind::Digest(period, date));
    }
    if let Some(rest) = name
        .strip_prefix("ops-")
        .and_then(|r| r.strip_suffix(".json"))
    {
        // <device>-<YYYYMMDDTHHMMSSmmmZ>-<NNNN>
        let (head, seq) = rest.rsplit_once('-')?;
        if seq.len() != 4 || !seq.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let (device, stamp) = head.rsplit_once('-')?;
        if device.is_empty() || !is_ops_stamp(stamp) {
            return None;
        }
        return Some(FileKind::Ops {
            device: device.to_string(),
        });
    }
    None
}

fn is_ops_stamp(s: &str) -> bool {
    let b = s.as_bytes();
    NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S%3fZ").is_ok()
        && b.len() == OPS_STAMP_LEN
        && b[8] == b'T'
        && b[OPS_STAMP_LEN - 1] == b'Z'
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[9..OPS_STAMP_LEN - 1].iter().all(u8::is_ascii_digit)
}

pub fn op_time(at: DateTime<Utc>) -> String {
    format!(
        "{}.{:03}Z",
        at.format("%Y-%m-%dT%H:%M:%S"),
        at.timestamp_subsec_millis().min(999)
    )
}

pub fn gzip_json<T: Serialize>(v: &T) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(v)?;
    let mut enc = GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::new(6));
    enc.write_all(&json)?;
    Ok(enc.finish()?)
}

pub fn gunzip_json<T: DeserializeOwned>(b: &[u8]) -> Result<T> {
    let mut out = Vec::new();
    GzDecoder::new(b).read_to_end(&mut out)?;
    Ok(serde_json::from_slice(&out)?)
}

pub fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

/// Normalizes an op from another client: `at` becomes UTC `op_time` text.
/// Returns `None` for an unparsable `at` or an empty `device`.
pub fn normalize_op(op: &Op) -> Option<Op> {
    if op.device.is_empty() {
        return None;
    }
    let at = DateTime::parse_from_rfc3339(&op.at)
        .ok()?
        .with_timezone(&Utc);
    Some(Op {
        at: op_time(at),
        ..op.clone()
    })
}

/// True when `a` beats `b`: newer `at`, then larger `device`, then `value == true`.
/// Callers must pass normalized ops (see `normalize_op`).
pub fn wins(a: &Stamp, b: &Stamp) -> bool {
    (a.at.as_str(), a.device.as_str(), a.value) > (b.at.as_str(), b.device.as_str(), b.value)
}

/// Callers must pass normalized ops (see `normalize_op`).
pub fn apply_op(state: &mut BTreeMap<i64, ItemState>, op: &Op) {
    let entry = state.entry(op.item_id).or_default();
    let slot = match op.field {
        Field::Read => &mut entry.read,
        Field::Starred => &mut entry.starred,
    };
    let stamp = Stamp {
        value: op.value,
        at: op.at.clone(),
        device: op.device.clone(),
    };
    if slot.as_ref().is_none_or(|cur| wins(&stamp, cur)) {
        *slot = Some(stamp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_round_trip() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 13).unwrap();
        assert_eq!(day_file_name(d), "day-2026-10-13.json.gz");
        assert_eq!(
            digest_file_name(Period::Week, NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()),
            "digest-week-2026-10-05.json.gz"
        );
        let at = DateTime::parse_from_rfc3339("2026-10-13T01:02:03.456Z")
            .unwrap()
            .with_timezone(&Utc);
        let ops = ops_file_name("dev-a", at, 7);
        assert_eq!(ops, "ops-dev-a-20261013T010203456Z-0007.json");
        assert_eq!(
            parse_file_name(&ops),
            Some(FileKind::Ops {
                device: "dev-a".into()
            })
        );
        assert_eq!(
            parse_file_name("day-2026-10-13.json.gz"),
            Some(FileKind::Day(d))
        );
        assert_eq!(parse_file_name("manifest.json"), Some(FileKind::Manifest));
        assert_eq!(
            parse_file_name("state-snapshot.json.gz"),
            Some(FileKind::Snapshot)
        );
        assert_eq!(
            parse_file_name("digest-month-2026-10-01.json.gz"),
            Some(FileKind::Digest(
                Period::Month,
                NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
            ))
        );
        assert_eq!(parse_file_name("random.txt"), None);
    }

    #[test]
    fn device_ids_with_hyphens_parse() {
        let at = DateTime::parse_from_rfc3339("2026-10-13T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let name = ops_file_name("6f1c2a9e-1b2c-4d5e-8f90-123456789abc", at, 1);
        assert_eq!(
            parse_file_name(&name),
            Some(FileKind::Ops {
                device: "6f1c2a9e-1b2c-4d5e-8f90-123456789abc".into()
            })
        );
    }

    #[test]
    fn gzip_is_deterministic_and_round_trips() {
        let f = OpsFile {
            device: "d".into(),
            db_id: None,
            ops: vec![],
        };
        let a = gzip_json(&f).unwrap();
        let b = gzip_json(&f).unwrap();
        assert_eq!(a, b);
        assert_eq!(sha256_hex(&a), sha256_hex(&b));
        let back: OpsFile = gunzip_json(&a).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn op_time_has_millis_and_z() {
        let at = DateTime::parse_from_rfc3339("2026-10-13T01:02:03.4567Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(op_time(at), "2026-10-13T01:02:03.456Z");
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

    #[test]
    fn newer_op_wins_and_ties_break_on_device() {
        let mut s = BTreeMap::new();
        apply_op(
            &mut s,
            &op(1, Field::Read, true, "2026-10-13T00:00:01.000Z", "a"),
        );
        apply_op(
            &mut s,
            &op(1, Field::Read, false, "2026-10-13T00:00:00.000Z", "b"),
        );
        assert!(s[&1].read.as_ref().unwrap().value);
        apply_op(
            &mut s,
            &op(1, Field::Read, false, "2026-10-13T00:00:01.000Z", "b"),
        );
        assert!(!s[&1].read.as_ref().unwrap().value);
        apply_op(
            &mut s,
            &op(1, Field::Starred, true, "2026-10-13T00:00:00.000Z", "a"),
        );
        assert!(!s[&1].read.as_ref().unwrap().value);
        assert!(s[&1].starred.as_ref().unwrap().value);
    }

    #[test]
    fn applying_ops_in_any_order_gives_the_same_state() {
        let ops = vec![
            op(1, Field::Read, true, "2026-10-13T00:00:01.000Z", "mac"),
            op(1, Field::Read, false, "2026-10-13T00:00:02.000Z", "phone"),
            op(1, Field::Starred, true, "2026-10-13T00:00:03.000Z", "mac"),
            op(2, Field::Read, true, "2026-10-13T00:00:01.000Z", "phone"),
        ];
        let mut forward = BTreeMap::new();
        for o in &ops {
            apply_op(&mut forward, o);
        }
        let mut backward = BTreeMap::new();
        for o in ops.iter().rev() {
            apply_op(&mut backward, o);
        }
        assert_eq!(forward, backward);
    }

    #[test]
    fn normalize_op_makes_foreign_times_sortable() {
        let mac = op(1, Field::Read, true, "2026-10-13T00:00:01.500Z", "mac");
        let cases = [
            ("2026-10-13T00:00:02Z", true),
            ("2026-10-13T09:00:03+09:00", true),
            ("2026-10-13T00:00:01.400123Z", false),
            ("2026-10-13T09:00:01.400+09:00", false),
        ];
        for (at, beats_mac) in cases {
            let phone = normalize_op(&op(1, Field::Read, false, at, "phone")).unwrap();
            assert!(
                phone.at.ends_with('Z') && phone.at.len() == 24,
                "{}",
                phone.at
            );
            let mut s = BTreeMap::new();
            apply_op(&mut s, &mac);
            apply_op(&mut s, &phone);
            assert_eq!(!s[&1].read.as_ref().unwrap().value, beats_mac, "{at}");
        }
        assert_eq!(
            normalize_op(&op(1, Field::Read, true, "2026-10-13T09:00:03+09:00", "p"))
                .unwrap()
                .at,
            "2026-10-13T00:00:03.000Z"
        );
    }

    #[test]
    fn normalize_op_rejects_garbage() {
        assert!(normalize_op(&op(1, Field::Read, true, "9999", "p")).is_none());
        assert!(normalize_op(&op(1, Field::Read, true, "", "p")).is_none());
        assert!(normalize_op(&op(1, Field::Read, true, "2026-10-13T00:00:00Z", "")).is_none());
    }

    #[test]
    fn full_ties_are_commutative() {
        let t = "2026-10-13T00:00:01.000Z";
        let a = op(1, Field::Read, true, t, "d");
        let b = op(1, Field::Read, false, t, "d");
        let mut ab = BTreeMap::new();
        apply_op(&mut ab, &a);
        apply_op(&mut ab, &b);
        let mut ba = BTreeMap::new();
        apply_op(&mut ba, &b);
        apply_op(&mut ba, &a);
        assert_eq!(ab, ba);
        assert!(ab[&1].read.as_ref().unwrap().value);
    }

    #[test]
    fn non_canonical_names_are_rejected() {
        for n in [
            "day-2026-13-01.json.gz",
            "day-2026-1-5.json.gz",
            "digest-year-2026-10-01.json.gz",
            "digest-week-2026-1-5.json.gz",
            "ops-x-20261013T010203456Z.json",
            "ops-x-20261013T0102034Z-0001.json",
            "ops-x-20261399T999999999Z-0001.json",
        ] {
            assert_eq!(parse_file_name(n), None, "{n}");
        }
    }

    fn fixture(name: &str) -> String {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sync")
            .join(name);
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn shared_file_name_fixtures() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(&fixture("file_names.json")).unwrap();
        for c in cases {
            let name = c["name"].as_str().unwrap();
            let date = c["date"]
                .as_str()
                .map(|d| NaiveDate::parse_from_str(d, DATE_FMT).unwrap());
            let want = match c["kind"].as_str() {
                None => None,
                Some("manifest") => Some(FileKind::Manifest),
                Some("meta") => Some(FileKind::Meta),
                Some("snapshot") => Some(FileKind::Snapshot),
                Some("day") => Some(FileKind::Day(date.unwrap())),
                Some("digest") => Some(FileKind::Digest(
                    Period::parse(c["period"].as_str().unwrap()).unwrap(),
                    date.unwrap(),
                )),
                Some("ops") => Some(FileKind::Ops {
                    device: c["device"].as_str().unwrap().into(),
                }),
                Some(k) => panic!("{k}"),
            };
            assert_eq!(parse_file_name(name), want, "{name}");
        }
    }

    #[test]
    fn shared_ops_merge_fixture() {
        let v: serde_json::Value = serde_json::from_str(&fixture("ops_merge.json")).unwrap();
        let ops: Vec<Op> = serde_json::from_value(v["ops"].clone()).unwrap();
        let want: BTreeMap<i64, ItemState> =
            serde_json::from_value(v["expected_state"].clone()).unwrap();
        let mut state = BTreeMap::new();
        for o in ops.iter().filter_map(normalize_op) {
            apply_op(&mut state, &o);
        }
        assert_eq!(state, want);
    }

    #[test]
    fn shared_snapshot_fixture_round_trips() {
        let snap: StateSnapshot = serde_json::from_str(&fixture("snapshot.json")).unwrap();
        assert_eq!(snap.covers.len(), 2);
        assert!(snap.state[&10].read.as_ref().unwrap().value);
        let back: StateSnapshot =
            serde_json::from_slice(&serde_json::to_vec(&snap).unwrap()).unwrap();
        assert_eq!(back, snap);
    }

    #[test]
    fn committed_gzip_fixtures_decode() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sync");
        let want: serde_json::Value = serde_json::from_str(&fixture("snapshot.json")).unwrap();
        let gz = std::fs::read(dir.join("snapshot.json.gz")).unwrap();
        let got: serde_json::Value = gunzip_json(&gz).unwrap();
        assert_eq!(got, want);
        // Written by the Dart side (mobile/test/sync/format_test.dart).
        let gz = std::fs::read(dir.join("ops_from_dart.json.gz")).unwrap();
        let f: OpsFile = gunzip_json(&gz).unwrap();
        assert_eq!(f.device, "dart-dev");
        assert_eq!(f.ops.len(), 2);
        assert_eq!(f.ops[0].item_id, 1);
        assert_eq!(f.ops[0].field, Field::Read);
        assert!(f.ops[0].value);
        assert_eq!(f.ops[1].at, "2026-10-13T00:00:01.456Z");
    }

    #[test]
    fn ops_file_db_id_is_optional_and_shared_with_dart() {
        // Written by hand, read by the Dart side too (format_test.dart).
        let f: OpsFile = serde_json::from_str(&fixture("ops_with_db_id.json")).unwrap();
        assert_eq!(f.db_id.as_deref(), Some("db-old"));
        assert_eq!(f.device, "phone-1");
        assert_eq!(f.ops.len(), 2);
        let back: serde_json::Value = serde_json::to_value(&f).unwrap();
        assert_eq!(back["db_id"], "db-old");
        // An older file has no db_id; writing it back adds none.
        let old: OpsFile = serde_json::from_str(r#"{"device":"d","ops":[]}"#).unwrap();
        assert_eq!(old.db_id, None);
        let v = serde_json::to_value(&old).unwrap();
        assert!(v.get("db_id").is_none(), "{v}");
    }

    /// Regenerates snapshot.json.gz. Run with `-- --ignored regenerate`.
    #[test]
    #[ignore]
    fn regenerate_snapshot_gzip_fixture() {
        let snap: StateSnapshot = serde_json::from_str(&fixture("snapshot.json")).unwrap();
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sync/snapshot.json.gz");
        std::fs::write(p, gzip_json(&snap).unwrap()).unwrap();
    }
}
