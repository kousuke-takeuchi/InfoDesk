//! サイドカーのツールが読む JSON 出力。形は計画 (Plan 3 Task 1) の契約どおりにする。

use chrono::{Duration, FixedOffset, NaiveDate, TimeZone, Utc};
use infodesk_core::collector::text::truncate_chars;
use infodesk_core::model::{Category, DigestScope, Item, ItemFilter, ItemOrder, Period};
use infodesk_core::period::period_start;
use infodesk_core::{Error, Result, Store};
use serde_json::{Value, json};

const DEFAULT_LIMIT: u32 = 30;
const MAX_LIMIT: u32 = 100;
const SUMMARY_CHARS: usize = 300;

#[derive(Debug, Default, Clone)]
pub struct ItemsArgs {
    pub category: Option<Category>,
    pub topic_id: Option<i64>,
    pub days: Option<i64>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub text: Option<String>,
    pub limit: u32,
}

pub fn clamp_limit(limit: u32) -> u32 {
    match limit {
        0 => DEFAULT_LIMIT,
        n => n.min(MAX_LIMIT),
    }
}

fn local_midnight_utc(date: NaiveDate, tz: FixedOffset) -> chrono::DateTime<Utc> {
    tz.from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight"))
        .single()
        .expect("fixed offset is unambiguous")
        .with_timezone(&Utc)
}

fn item_value(item: &Item) -> Value {
    let summary = item
        .ai_summary
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            let e = item.excerpt.trim();
            (!e.is_empty()).then(|| truncate_chars(e, SUMMARY_CHARS))
        });
    json!({
        "id": item.id,
        "title": item.title,
        "url": item.url,
        "source": item.source_name,
        "category": item.category.as_str(),
        "category_label": item.category.label_ja(),
        "fetched_at": item.fetched_at,
        "published_at": item.published_at,
        "score": item.score,
        "summary": summary,
    })
}

pub fn items_json(store: &Store, args: &ItemsArgs, tz: FixedOffset) -> Result<Value> {
    let mut filter = ItemFilter {
        category: args.category,
        topic_id: args.topic_id,
        text: args.text.clone(),
        order: ItemOrder::Recent,
        limit: clamp_limit(args.limit),
        ..Default::default()
    };
    if let Some(d) = args.days {
        filter.since = Some(Utc::now() - Duration::days(d));
    }
    if let Some(from) = args.from {
        filter.since = Some(local_midnight_utc(from, tz));
    }
    if let Some(to) = args.to {
        filter.until = Some(local_midnight_utc(to + Duration::days(1), tz));
    }
    let total = store.count_items(&filter)?;
    let items: Vec<Value> = store.query_items(&filter)?.iter().map(item_value).collect();
    Ok(json!({ "items": items, "total": total }))
}

pub fn item_json(store: &Store, id: i64) -> Result<Value> {
    let item = store
        .get_item(id)?
        .ok_or_else(|| Error::Invalid(format!("item {id} not found")))?;
    let mut v = item_value(&item);
    let obj = v.as_object_mut().expect("object");
    obj.insert("excerpt".into(), json!(item.excerpt));
    obj.insert("author".into(), json!(item.author));
    obj.insert("read".into(), json!(item.read));
    obj.insert("starred".into(), json!(item.starred));
    Ok(json!({ "item": v }))
}

pub fn digests_json(
    store: &Store,
    period: Period,
    date: NaiveDate,
    category: Option<Category>,
    topic_id: Option<i64>,
) -> Result<Value> {
    let start = period_start(period, date);
    let scopes: Vec<(DigestScope, String)> = match (category, topic_id) {
        (_, Some(id)) => {
            let topic = store
                .get_topic(id)?
                .ok_or_else(|| Error::Invalid(format!("topic {id} not found")))?;
            vec![(DigestScope::Topic(id), topic.name)]
        }
        (Some(c), None) => vec![(DigestScope::Category(c), c.label_ja().to_string())],
        (None, None) => Category::ALL
            .into_iter()
            .map(|c| (DigestScope::Category(c), c.label_ja().to_string()))
            .collect(),
    };
    let mut out = Vec::new();
    for (scope, label) in scopes {
        let v = match store.get_digest(period, start, &scope)? {
            Some(d) => json!({
                "scope": scope.key(),
                "label": label,
                "status": d.status.as_str(),
                "body": d.body,
                "item_ids": d.item_ids,
                "provider": d.provider,
                "created_at": d.created_at,
                "last_error": d.last_error,
            }),
            None => json!({
                "scope": scope.key(),
                "label": label,
                "status": "none",
                "body": "",
                "item_ids": [],
                "provider": null,
                "created_at": null,
                "last_error": null,
            }),
        };
        out.push(v);
    }
    Ok(json!({
        "period": period.as_str(),
        "period_start": start.format("%Y-%m-%d").to_string(),
        "digests": out,
    }))
}

pub fn config_json(store: &Store) -> Result<Value> {
    let map: serde_json::Map<String, Value> = store
        .list_settings()?
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    Ok(json!({ "settings": map }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone, Utc};
    use infodesk_core::Store;
    use infodesk_core::model::{Category, DigestScope, NewItem, Period, SourceKind};
    use infodesk_core::store::NewDigest;

    fn tz() -> chrono::FixedOffset {
        chrono::FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn item(url: &str, title: &str, excerpt: &str) -> NewItem {
        NewItem {
            url: url.into(),
            title: title.into(),
            excerpt: excerpt.into(),
            author: None,
            published_at: None,
            score: Some(10),
        }
    }

    fn seeded() -> Store {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_source(
                SourceKind::Rss,
                "介護ニュース",
                Category::Medcare,
                "https://example.com/feed",
            )
            .unwrap();
        let source = store.get_source(id).unwrap().unwrap();
        // 2026-10-07 10:00 JST と 2026-10-08 10:00 JST に取得した記事
        let d7 = Utc.with_ymd_and_hms(2026, 10, 7, 1, 0, 0).unwrap();
        let d8 = Utc.with_ymd_and_hms(2026, 10, 8, 1, 0, 0).unwrap();
        store
            .upsert_items(
                &source,
                &[item(
                    "https://a/1",
                    "認知症の新薬",
                    &"長い概要。".repeat(100),
                )],
                &d7,
            )
            .unwrap();
        store
            .upsert_items(&source, &[item("https://a/2", "介護報酬の改定", "")], &d8)
            .unwrap();
        store
    }

    #[test]
    fn items_json_filters_by_local_dates_and_shapes_fields() {
        let store = seeded();
        let args = ItemsArgs {
            from: Some(NaiveDate::from_ymd_opt(2026, 10, 8).unwrap()),
            to: Some(NaiveDate::from_ymd_opt(2026, 10, 8).unwrap()),
            limit: 30,
            ..Default::default()
        };
        let v = items_json(&store, &args, tz()).unwrap();
        assert_eq!(v["total"], 1);
        let first = &v["items"][0];
        assert_eq!(first["title"], "介護報酬の改定");
        assert_eq!(first["source"], "介護ニュース");
        assert_eq!(first["category"], "medcare");
        assert_eq!(first["category_label"], "医療介護");
        assert!(first["summary"].is_null(), "空の概要は null");
        assert!(first["id"].is_i64());
    }

    #[test]
    fn items_json_truncates_excerpt_summary_and_caps_limit() {
        let store = seeded();
        let args = ItemsArgs {
            limit: 500,
            ..Default::default()
        };
        let v = items_json(&store, &args, tz()).unwrap();
        assert_eq!(v["items"].as_array().unwrap().len(), 2);
        let long = v["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["title"] == "認知症の新薬")
            .unwrap();
        assert_eq!(
            long["summary"].as_str().unwrap().chars().count(),
            301,
            "300文字 + …"
        );
        assert_eq!(clamp_limit(500), 100);
        assert_eq!(clamp_limit(0), 30);
    }

    #[test]
    fn item_json_includes_detail_and_rejects_unknown_id() {
        let store = seeded();
        let id = store.query_items(&Default::default()).unwrap()[0].id;
        let v = item_json(&store, id).unwrap();
        assert_eq!(v["item"]["id"], id);
        assert!(v["item"]["excerpt"].is_string());
        assert_eq!(v["item"]["read"], false);
        let err = item_json(&store, 9999).unwrap_err().to_string();
        assert!(err.contains("item 9999 not found"), "{err}");
    }

    #[test]
    fn digests_json_reports_saved_and_missing_scopes_without_generating() {
        let store = seeded();
        let now = Utc.with_ymd_and_hms(2026, 10, 8, 1, 0, 0).unwrap();
        store
            .save_digest_done(
                &NewDigest {
                    period: Period::Day,
                    period_start: NaiveDate::from_ymd_opt(2026, 10, 8).unwrap(),
                    scope: DigestScope::Category(Category::Medcare),
                    body: "- 介護報酬の議論が始まった".into(),
                    item_ids: vec![2],
                    provider: "ollama:gemma4:26b".into(),
                },
                &now,
            )
            .unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let v = digests_json(&store, Period::Day, date, None, None).unwrap();
        assert_eq!(v["period"], "day");
        assert_eq!(v["period_start"], "2026-10-08");
        let all = v["digests"].as_array().unwrap();
        assert_eq!(all.len(), Category::ALL.len());
        let med = all
            .iter()
            .find(|d| d["scope"] == "category:medcare")
            .unwrap();
        assert_eq!(med["status"], "done");
        assert_eq!(med["label"], "医療介護");
        let gh = all
            .iter()
            .find(|d| d["scope"] == "category:github")
            .unwrap();
        assert_eq!(gh["status"], "none");
        assert!(gh["provider"].is_null());
        // 1カテゴリーだけ
        let one = digests_json(&store, Period::Day, date, Some(Category::Medcare), None).unwrap();
        assert_eq!(one["digests"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn digests_json_for_a_topic_uses_the_topic_name_as_label() {
        let store = seeded();
        let tid = store.add_topic("認知症", &["認知症".to_string()]).unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let v = digests_json(&store, Period::Week, date, None, Some(tid)).unwrap();
        assert_eq!(v["period_start"], "2026-10-05", "週は月曜始まり");
        let d = &v["digests"][0];
        assert_eq!(d["scope"], format!("topic:{tid}"));
        assert_eq!(d["label"], "認知症");
    }

    #[test]
    fn config_json_lists_settings() {
        let store = seeded();
        store.set_setting("ollama.model", "gemma4:26b").unwrap();
        let v = config_json(&store).unwrap();
        assert_eq!(v["settings"]["ollama.model"], "gemma4:26b");
    }
}
