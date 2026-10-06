pub mod prompts;

use std::collections::BTreeSet;

use chrono::{DateTime, Days, FixedOffset, NaiveDate, Utc};

use crate::collector::text::truncate_chars;
use crate::error::{Error, Result};
use crate::llm::{LlmProvider, clean_output};
use crate::model::{Category, DigestScope, DigestStatus, Item, ItemFilter, ItemOrder, Period};
use crate::period::{period_end, period_start, utc_range};
use crate::store::{NewDigest, Store};

/// 日次のカテゴリー要約に入れる記事の上限。
const CATEGORY_ITEM_LIMIT: u32 = 60;
/// トピック別要約に入れる記事の上限。
const TOPIC_ITEM_LIMIT: u32 = 80;
const LINE_DETAIL_MAX_CHARS: usize = 200;

#[derive(Debug, Clone)]
pub struct DigestJob {
    pub period: Period,
    pub period_start: NaiveDate,
    pub scope: DigestScope,
    pub system: String,
    pub prompt: String,
    pub item_ids: Vec<i64>,
}

fn item_line(item: &Item) -> String {
    let detail = item
        .ai_summary
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&item.excerpt);
    if detail.trim().is_empty() {
        format!("- {}", item.title)
    } else {
        format!(
            "- {} | {}",
            item.title,
            truncate_chars(detail, LINE_DETAIL_MAX_CHARS)
        )
    }
}

fn items_input(items: &[Item]) -> (Vec<String>, Vec<i64>) {
    (
        items.iter().map(item_line).collect(),
        items.iter().map(|i| i.id).collect(),
    )
}

/// 期間要約のジョブを組み立てる。LLMは呼ばない。
/// 入力がない場合と、生成済みで `force` が偽の場合は `None`。
pub fn plan_digest(
    store: &Store,
    period: Period,
    date: NaiveDate,
    scope: &DigestScope,
    tz: FixedOffset,
    force: bool,
) -> Result<Option<DigestJob>> {
    let start = period_start(period, date);
    if !force
        && let Some(existing) = store.get_digest(period, start, scope)?
        && existing.status == DigestStatus::Done
    {
        return Ok(None);
    }
    let (from, to) = utc_range(period, start, tz);

    let (label, lines, item_ids) = match scope {
        // トピック別は、期間によらず該当記事から直接作る。
        DigestScope::Topic(topic_id) => {
            let topic = store
                .get_topic(*topic_id)?
                .ok_or_else(|| Error::Invalid(format!("topic {topic_id} not found")))?;
            let items = store.query_items(&ItemFilter {
                topic_id: Some(*topic_id),
                since: Some(from),
                until: Some(to),
                order: ItemOrder::Score,
                limit: TOPIC_ITEM_LIMIT,
                ..Default::default()
            })?;
            let (lines, ids) = items_input(&items);
            (topic.name, lines, ids)
        }
        DigestScope::Category(category) => match period {
            Period::Day => {
                let items = store.query_items(&ItemFilter {
                    category: Some(*category),
                    since: Some(from),
                    until: Some(to),
                    order: ItemOrder::Score,
                    limit: CATEGORY_ITEM_LIMIT,
                    ..Default::default()
                })?;
                let (lines, ids) = items_input(&items);
                (category.label_ja().to_string(), lines, ids)
            }
            // 週次は日次要約から、月次は週次要約から作る。
            // 月次には、月の初日より前に始まり月にまたがる週も入れる (6日前までに始まった週)。
            Period::Week | Period::Month => {
                let (child, from) = if period == Period::Week {
                    (Period::Day, start)
                } else {
                    (Period::Week, start - Days::new(6))
                };
                let children =
                    store.digests_in_range(child, scope, from, period_end(period, start))?;
                let lines = children
                    .iter()
                    .map(|c| format!("## {}\n{}", c.period_start, c.body))
                    .collect();
                let ids: BTreeSet<i64> = children
                    .iter()
                    .flat_map(|c| c.item_ids.iter().copied())
                    .collect();
                (
                    category.label_ja().to_string(),
                    lines,
                    ids.into_iter().collect(),
                )
            }
        },
    };

    if lines.is_empty() {
        return Ok(None);
    }
    Ok(Some(DigestJob {
        period,
        period_start: start,
        scope: scope.clone(),
        system: prompts::DIGEST_SYSTEM.to_string(),
        prompt: prompts::digest_prompt(&label, period, &lines),
        item_ids,
    }))
}

/// LLMの結果を保存する。失敗と空の出力は、保留 (3回目で失敗) として記録する。
pub fn finish_digest(
    store: &Store,
    job: &DigestJob,
    provider: &str,
    output: Result<String>,
    now: &DateTime<Utc>,
) -> Result<DigestStatus> {
    match output.and_then(|raw| clean_output(&raw)) {
        Ok(body) => {
            store.save_digest_done(
                &NewDigest {
                    period: job.period,
                    period_start: job.period_start,
                    scope: job.scope.clone(),
                    body,
                    item_ids: job.item_ids.clone(),
                    provider: provider.to_string(),
                },
                now,
            )?;
            Ok(DigestStatus::Done)
        }
        Err(_) => store.mark_digest_attempt_failed(job.period, job.period_start, &job.scope, now),
    }
}

/// 計画、LLM呼び出し、保存をまとめて行う。`None` は生成不要だったことを表す。
#[allow(clippy::too_many_arguments)]
pub async fn run_digest(
    store: &Store,
    llm: &dyn LlmProvider,
    period: Period,
    date: NaiveDate,
    scope: &DigestScope,
    tz: FixedOffset,
    force: bool,
    now: &DateTime<Utc>,
) -> Result<Option<DigestStatus>> {
    let Some(job) = plan_digest(store, period, date, scope, tz, force)? else {
        return Ok(None);
    };
    let output = llm.complete(&job.system, &job.prompt).await;
    Ok(Some(finish_digest(store, &job, &llm.name(), output, now)?))
}

/// `summarize_top_items` の結果。
#[derive(Debug, Clone, PartialEq)]
pub struct ItemSummaryReport {
    pub ok: usize,
    pub failed: usize,
    /// 最初に失敗したLLM呼び出しのエラー。失敗がなければ None。
    pub error: Option<String>,
}

/// カテゴリーの上位 `limit` 件のうち、要約がない記事に要約を付ける。
/// 1件失敗したら残りを中止する (LLMが落ちている場合に全件で待たないため)。
/// 失敗した記事は要約なしのまま残り、次回の実行で再び対象になる。
pub async fn summarize_top_items(
    store: &Store,
    llm: &dyn LlmProvider,
    category: Category,
    since: &DateTime<Utc>,
    limit: u32,
) -> Result<ItemSummaryReport> {
    let items = store.top_unsummarized(category, since, limit)?;
    let mut ok = 0;
    let mut error = None;
    for item in &items {
        let result = llm
            .complete(prompts::ITEM_SYSTEM, &prompts::item_prompt(item))
            .await
            .and_then(|raw| clean_output(&raw));
        match result {
            Ok(summary) => {
                store.set_ai_summary(item.id, &summary)?;
                ok += 1;
            }
            Err(e) => {
                error = Some(e.to_string());
                break;
            }
        }
    }
    Ok(ItemSummaryReport {
        ok,
        failed: items.len() - ok,
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::mock::MockProvider;
    use crate::model::Source;
    use crate::testutil::{at, new_item, store_with_source};

    fn jst() -> FixedOffset {
        FixedOffset::east_opt(9 * 3600).unwrap()
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    const NOW: &str = "2026-10-06T22:00:00Z";

    /// JSTの10-06に2件 (指標 5 と 50)、10-05に1件を入れる。
    fn seeded() -> (Store, Source) {
        let (store, src) = store_with_source(Category::Github);
        store
            .upsert_items(
                &src,
                &[new_item("https://e.com/prev", "前日の記事", "", Some(999))],
                &at("2026-10-05T14:00:00Z"),
            )
            .unwrap();
        store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/low", "低スコア記事", "概要A", Some(5)),
                    new_item("https://e.com/high", "高スコア記事", "概要B", Some(50)),
                ],
                &at("2026-10-05T16:00:00Z"),
            )
            .unwrap();
        (store, src)
    }

    #[tokio::test]
    async fn item_summaries_are_saved_for_top_items() {
        let (store, _) = seeded();
        let llm = MockProvider::ok(" 一文要約。 ");
        let report = summarize_top_items(
            &store,
            &llm,
            Category::Github,
            &at("2026-10-05T15:00:00Z"),
            1,
        )
        .await
        .unwrap();
        assert_eq!(
            report,
            ItemSummaryReport {
                ok: 1,
                failed: 0,
                error: None
            }
        );
        let calls = llm.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, prompts::ITEM_SYSTEM);
        assert!(calls[0].1.contains("高スコア記事"));
        let items = store
            .query_items(&ItemFilter {
                text: Some("高スコア記事".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(items[0].ai_summary.as_deref(), Some("一文要約。"));
    }

    #[tokio::test]
    async fn item_summary_failure_stops_early_and_leaves_items_for_retry() {
        let (store, _) = seeded();
        let llm = MockProvider::failing("connection refused");
        let report = summarize_top_items(
            &store,
            &llm,
            Category::Github,
            &at("2026-10-05T15:00:00Z"),
            10,
        )
        .await
        .unwrap();
        assert_eq!((report.ok, report.failed), (0, 2));
        let err = report.error.expect("LLM error must be surfaced");
        assert!(err.contains("connection refused"), "error was: {err}");
        assert_eq!(llm.calls().len(), 1);
        assert_eq!(
            store
                .top_unsummarized(Category::Github, &at("2026-10-05T15:00:00Z"), 10)
                .unwrap()
                .len(),
            2
        );

        // Review Focus 5: 空の出力は保存しない。
        let llm = MockProvider::ok("   ");
        let report = summarize_top_items(
            &store,
            &llm,
            Category::Github,
            &at("2026-10-05T15:00:00Z"),
            10,
        )
        .await
        .unwrap();
        assert_eq!((report.ok, report.failed), (0, 2));
        assert!(report.error.is_some());
        let items = store
            .query_items(&ItemFilter {
                category: Some(Category::Github),
                ..Default::default()
            })
            .unwrap();
        assert!(!items.is_empty());
        assert!(items.iter().all(|i| i.ai_summary.is_none()));
    }

    #[tokio::test]
    async fn day_digest_uses_items_of_that_local_day_in_score_order() {
        let (store, _) = seeded();
        let high = store
            .query_items(&ItemFilter {
                text: Some("高スコア記事".into()),
                ..Default::default()
            })
            .unwrap()[0]
            .id;
        store.set_ai_summary(high, "AI要約B").unwrap();

        let llm = MockProvider::ok("日次まとめ");
        let scope = DigestScope::Category(Category::Github);
        let st = run_digest(
            &store,
            &llm,
            Period::Day,
            d("2026-10-06"),
            &scope,
            jst(),
            false,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));

        let (system, prompt) = llm.calls().remove(0);
        assert_eq!(system, prompts::DIGEST_SYSTEM);
        assert!(prompt.contains("対象: GitHub"));
        assert!(prompt.contains("期間: 今日"));
        assert!(!prompt.contains("前日の記事"));
        assert!(prompt.contains("AI要約B"));
        assert!(!prompt.contains("概要B"));
        assert!(prompt.contains("概要A"));
        assert!(prompt.find("高スコア記事").unwrap() < prompt.find("低スコア記事").unwrap());

        let saved = store
            .get_digest(Period::Day, d("2026-10-06"), &scope)
            .unwrap()
            .unwrap();
        assert_eq!(saved.body, "日次まとめ");
        assert_eq!(saved.provider, "mock");
        assert_eq!(saved.item_ids.len(), 2);
        assert_eq!(saved.item_ids[0], high);
    }

    #[tokio::test]
    async fn no_input_means_no_llm_call_and_no_digest() {
        let (store, _) = seeded();
        let llm = MockProvider::ok("x");
        let scope = DigestScope::Category(Category::Arxiv);
        let st = run_digest(
            &store,
            &llm,
            Period::Day,
            d("2026-10-06"),
            &scope,
            jst(),
            false,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, None);
        // 日次要約がまだない週次も同じ。
        let scope = DigestScope::Category(Category::Github);
        let st = run_digest(
            &store,
            &llm,
            Period::Week,
            d("2026-10-06"),
            &scope,
            jst(),
            false,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, None);
        assert!(llm.calls().is_empty());
        assert!(
            store
                .get_digest(Period::Week, d("2026-10-05"), &scope)
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn week_uses_day_digests_and_month_uses_week_digests() {
        let (store, _) = seeded();
        let scope = DigestScope::Category(Category::Github);
        for (day, body) in [
            ("2026-10-05", "月曜のまとめ"),
            ("2026-10-06", "火曜のまとめ"),
        ] {
            let llm = MockProvider::ok(body);
            run_digest(
                &store,
                &llm,
                Period::Day,
                d(day),
                &scope,
                jst(),
                false,
                &at(NOW),
            )
            .await
            .unwrap();
        }

        let llm = MockProvider::ok("週次まとめ");
        let st = run_digest(
            &store,
            &llm,
            Period::Week,
            d("2026-10-08"),
            &scope,
            jst(),
            false,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));
        let prompt = llm.calls().remove(0).1;
        assert!(prompt.contains("期間: 今週"));
        assert!(prompt.contains("月曜のまとめ") && prompt.contains("火曜のまとめ"));
        assert!(prompt.find("月曜のまとめ").unwrap() < prompt.find("火曜のまとめ").unwrap());
        assert!(!prompt.contains("高スコア記事"));
        let week = store
            .get_digest(Period::Week, d("2026-10-05"), &scope)
            .unwrap()
            .unwrap();
        assert_eq!(week.item_ids.len(), 3);

        let llm = MockProvider::ok("月次まとめ");
        let st = run_digest(
            &store,
            &llm,
            Period::Month,
            d("2026-10-20"),
            &scope,
            jst(),
            false,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));
        let prompt = llm.calls().remove(0).1;
        assert!(prompt.contains("期間: 今月"));
        assert!(prompt.contains("週次まとめ"));
        assert!(!prompt.contains("月曜のまとめ"));
        let month = store
            .get_digest(Period::Month, d("2026-10-01"), &scope)
            .unwrap()
            .unwrap();
        assert_eq!(month.body, "月次まとめ");
        assert_eq!(month.item_ids.len(), 3);
    }

    #[tokio::test]
    async fn month_includes_weeks_overlapping_the_month() {
        let store = Store::open_in_memory().unwrap();
        let scope = DigestScope::Category(Category::Github);
        for (start, body, ids) in [
            ("2026-09-21", "9月21日の週", vec![1]),
            ("2026-09-28", "9月28日の週", vec![2]),
            ("2026-10-26", "10月26日の週", vec![3]),
            ("2026-11-02", "11月2日の週", vec![4]),
        ] {
            store
                .save_digest_done(
                    &NewDigest {
                        period: Period::Week,
                        period_start: d(start),
                        scope: scope.clone(),
                        body: body.to_string(),
                        item_ids: ids,
                        provider: "mock".to_string(),
                    },
                    &at(NOW),
                )
                .unwrap();
        }
        let job = plan_digest(&store, Period::Month, d("2026-10-15"), &scope, jst(), false)
            .unwrap()
            .unwrap();
        // 9/28 の週は 10/1-10/4 を含むので10月に入れる。9/21 の週と 11/2 の週は入れない。
        assert!(job.prompt.contains("9月28日の週"));
        assert!(job.prompt.contains("10月26日の週"));
        assert!(!job.prompt.contains("9月21日の週"));
        assert!(!job.prompt.contains("11月2日の週"));
        assert_eq!(job.item_ids, vec![2, 3]);
    }

    #[tokio::test]
    async fn done_digest_is_reused_unless_forced() {
        let (store, _) = seeded();
        let scope = DigestScope::Category(Category::Github);
        let day = d("2026-10-06");
        run_digest(
            &store,
            &MockProvider::ok("v1"),
            Period::Day,
            day,
            &scope,
            jst(),
            false,
            &at(NOW),
        )
        .await
        .unwrap();

        let llm = MockProvider::ok("v2");
        let st = run_digest(
            &store,
            &llm,
            Period::Day,
            day,
            &scope,
            jst(),
            false,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, None);
        assert!(llm.calls().is_empty());
        assert_eq!(
            store
                .get_digest(Period::Day, day, &scope)
                .unwrap()
                .unwrap()
                .body,
            "v1"
        );

        let st = run_digest(
            &store,
            &llm,
            Period::Day,
            day,
            &scope,
            jst(),
            true,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));
        assert_eq!(
            store
                .get_digest(Period::Day, day, &scope)
                .unwrap()
                .unwrap()
                .body,
            "v2"
        );

        // 強制再生成が失敗しても、生成済みの本文は残る。
        let st = run_digest(
            &store,
            &MockProvider::failing("down"),
            Period::Day,
            day,
            &scope,
            jst(),
            true,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));
        assert_eq!(
            store
                .get_digest(Period::Day, day, &scope)
                .unwrap()
                .unwrap()
                .body,
            "v2"
        );
    }

    #[tokio::test]
    async fn llm_failure_and_empty_output_leave_digest_pending_then_failed() {
        // Review Focus 5
        let (store, _) = seeded();
        let scope = DigestScope::Category(Category::Github);
        let day = d("2026-10-06");
        let st = run_digest(
            &store,
            &MockProvider::failing("down"),
            Period::Day,
            day,
            &scope,
            jst(),
            false,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, Some(DigestStatus::Pending));
        let st = run_digest(
            &store,
            &MockProvider::ok("  \n "),
            Period::Day,
            day,
            &scope,
            jst(),
            false,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, Some(DigestStatus::Pending));
        let saved = store.get_digest(Period::Day, day, &scope).unwrap().unwrap();
        assert_eq!(saved.body, "");
        assert_eq!(saved.attempts, 2);
        let st = run_digest(
            &store,
            &MockProvider::failing("down"),
            Period::Day,
            day,
            &scope,
            jst(),
            false,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, Some(DigestStatus::Failed));

        // 保留または失敗の後でも、成功すれば生成済みになる。
        let st = run_digest(
            &store,
            &MockProvider::ok("復旧"),
            Period::Day,
            day,
            &scope,
            jst(),
            false,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));
    }

    #[tokio::test]
    async fn topic_digest_reads_matching_items_directly_for_any_period() {
        let (store, src) = store_with_source(Category::Medcare);
        store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/1", "介護報酬の改定", "", None),
                    new_item("https://e.com/2", "無関係な記事", "", None),
                ],
                &at("2026-10-05T16:00:00Z"),
            )
            .unwrap();
        let topic = store.add_topic("介護", &["介護".to_string()]).unwrap();
        let scope = DigestScope::Topic(topic);

        let llm = MockProvider::ok("トピック週次");
        let st = run_digest(
            &store,
            &llm,
            Period::Week,
            d("2026-10-06"),
            &scope,
            jst(),
            false,
            &at(NOW),
        )
        .await
        .unwrap();
        assert_eq!(st, Some(DigestStatus::Done));
        let prompt = llm.calls().remove(0).1;
        assert!(prompt.contains("対象: 介護"));
        assert!(prompt.contains("介護報酬の改定"));
        assert!(!prompt.contains("無関係な記事"));
        assert_eq!(
            store
                .get_digest(Period::Week, d("2026-10-05"), &scope)
                .unwrap()
                .unwrap()
                .item_ids
                .len(),
            1
        );

        let missing = DigestScope::Topic(9999);
        let err = run_digest(
            &store,
            &llm,
            Period::Day,
            d("2026-10-06"),
            &missing,
            jst(),
            false,
            &at(NOW),
        )
        .await;
        assert!(matches!(err, Err(Error::Invalid(_))));
    }
}
