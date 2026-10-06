use chrono::{DateTime, Utc};
use rusqlite::types::Value;
use rusqlite::{OptionalExtension, Row, params, params_from_iter};

use super::{Store, col_opt_ts, col_parse, col_ts, ts};
use crate::error::{Error, Result};
use crate::model::{Category, Item, ItemFilter, ItemOrder, NewItem, Source};

const ITEM_COLS: &str = "items.id, items.source_id, items.source_name, items.category, \
     items.url, items.title, items.excerpt, items.author, items.published_at, \
     items.fetched_at, items.score, items.ai_summary, items.read, items.starred";

fn row_to_item(r: &Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get(0)?,
        source_id: r.get(1)?,
        source_name: r.get(2)?,
        category: col_parse(r, 3, Category::parse)?,
        url: r.get(4)?,
        title: r.get(5)?,
        excerpt: r.get(6)?,
        author: r.get(7)?,
        published_at: col_opt_ts(r, 8)?,
        fetched_at: col_ts(r, 9)?,
        score: r.get(10)?,
        ai_summary: r.get(11)?,
        read: r.get(12)?,
        starred: r.get(13)?,
    })
}

/// `items_words` (unicode61) に入れる文字列。unicode61 は「Rustの新機能」を1語として
/// 扱うため、CJK以降の文字 (U+2E80 以上) を空白に置き換え、英数字の語を切り出せるようにする。
/// この表は英数字だけのキーワードの照合にしか使わないので、CJKの文字を落としても困らない。
fn word_index_text(s: &str) -> String {
    s.chars()
        .map(|c| if c >= '\u{2E80}' { ' ' } else { c })
        .collect()
}

fn quote_phrase(s: &str) -> Value {
    Value::Text(format!("\"{}\"", s.replace('"', "\"\"")))
}

/// キーワードのいずれかがタイトルか概要に含まれる、というSQL条件を作る。
/// 英数字だけのキーワード (`AI`, `Rust` など) は単語単位で照合する (大文字小文字は区別しない)。
/// 部分一致だと `AI` が "again" に、`Rust` が "trust" に一致してしまうため。
/// それ以外は部分一致: trigramのFTS5は3文字未満の語句に一致しないため、3文字未満はLIKEで照合する。
/// FTS5に渡す語句はフレーズとして引用し、FTS5の演算子 (AND, *, NEAR など) を無効にする。
fn keyword_clause(keywords: &[String], args: &mut Vec<Value>) -> String {
    let mut parts = Vec::new();
    for kw in keywords.iter().map(|k| k.trim()).filter(|k| !k.is_empty()) {
        if kw.chars().all(|c| c.is_ascii_alphanumeric()) {
            args.push(quote_phrase(kw));
            parts.push(
                "items.id IN (SELECT rowid FROM items_words WHERE items_words MATCH ?)".to_string(),
            );
        } else if kw.chars().count() >= 3 {
            args.push(quote_phrase(kw));
            parts.push(
                "items.id IN (SELECT rowid FROM items_fts WHERE items_fts MATCH ?)".to_string(),
            );
        } else {
            let pattern = format!(
                "%{}%",
                kw.replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            args.push(Value::Text(pattern.clone()));
            args.push(Value::Text(pattern));
            parts.push(
                "(items.title LIKE ? ESCAPE '\\' OR items.excerpt LIKE ? ESCAPE '\\')".to_string(),
            );
        }
    }
    if parts.is_empty() {
        "0".to_string()
    } else {
        format!("({})", parts.join(" OR "))
    }
}

impl Store {
    pub fn upsert_items(
        &self,
        source: &Source,
        items: &[NewItem],
        now: &DateTime<Utc>,
    ) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let mut created = 0;
        for it in items {
            let inserted = tx.execute(
                "INSERT INTO items(source_id, source_name, category, url, title, excerpt, \
                 author, published_at, fetched_at, score) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
                 ON CONFLICT(url) DO NOTHING",
                params![
                    source.id,
                    source.name,
                    source.category.as_str(),
                    it.url,
                    it.title,
                    it.excerpt,
                    it.author,
                    it.published_at.as_ref().map(ts),
                    ts(now),
                    it.score,
                ],
            )?;
            if inserted == 1 {
                let id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO items_fts(rowid, title, excerpt) VALUES (?1, ?2, ?3)",
                    params![id, it.title, it.excerpt],
                )?;
                tx.execute(
                    "INSERT INTO items_words(rowid, title, excerpt) VALUES (?1, ?2, ?3)",
                    params![id, word_index_text(&it.title), word_index_text(&it.excerpt)],
                )?;
                created += 1;
            } else if let Some(score) = it.score {
                tx.execute(
                    "UPDATE items SET score = ?1 WHERE url = ?2",
                    params![score, it.url],
                )?;
            }
        }
        tx.commit()?;
        Ok(created)
    }

    pub fn query_items(&self, f: &ItemFilter) -> Result<Vec<Item>> {
        let mut wh: Vec<String> = Vec::new();
        let mut args: Vec<Value> = Vec::new();
        if let Some(c) = f.category {
            wh.push("items.category = ?".into());
            args.push(Value::Text(c.as_str().into()));
        }
        if let Some(id) = f.source_id {
            wh.push("items.source_id = ?".into());
            args.push(Value::Integer(id));
        }
        if let Some(since) = &f.since {
            wh.push("items.fetched_at >= ?".into());
            args.push(Value::Text(ts(since)));
        }
        if let Some(until) = &f.until {
            wh.push("items.fetched_at < ?".into());
            args.push(Value::Text(ts(until)));
        }
        if f.unread_only {
            wh.push("items.read = 0".into());
        }
        if f.starred_only {
            wh.push("items.starred = 1".into());
        }
        if let Some(topic_id) = f.topic_id {
            let topic = self
                .get_topic(topic_id)?
                .ok_or_else(|| Error::Invalid(format!("topic {topic_id} not found")))?;
            wh.push(keyword_clause(&topic.keywords, &mut args));
        }
        if let Some(text) = f.text.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            wh.push(keyword_clause(&[text.to_string()], &mut args));
        }
        let where_sql = if wh.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", wh.join(" AND "))
        };
        let order = match f.order {
            ItemOrder::Recent => {
                "COALESCE(items.published_at, items.fetched_at) DESC, items.id DESC"
            }
            ItemOrder::Score => {
                "items.score IS NULL, items.score DESC, \
                 COALESCE(items.published_at, items.fetched_at) DESC, \
                 items.fetched_at DESC, items.id DESC"
            }
        };
        let limit = if f.limit == 0 { 100 } else { f.limit };
        args.push(Value::Integer(limit as i64));
        args.push(Value::Integer(f.offset as i64));
        let sql =
            format!("SELECT {ITEM_COLS} FROM items {where_sql} ORDER BY {order} LIMIT ? OFFSET ?");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(args), row_to_item)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_item(&self, id: i64) -> Result<Option<Item>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {ITEM_COLS} FROM items WHERE items.id = ?1"),
                params![id],
                row_to_item,
            )
            .optional()?)
    }

    pub fn set_read(&self, id: i64, read: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE items SET read = ?2 WHERE id = ?1",
            params![id, read],
        )?;
        Ok(())
    }

    pub fn set_starred(&self, id: i64, starred: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE items SET starred = ?2 WHERE id = ?1",
            params![id, starred],
        )?;
        Ok(())
    }

    pub fn set_ai_summary(&self, id: i64, summary: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE items SET ai_summary = ?2 WHERE id = ?1",
            params![id, summary],
        )?;
        Ok(())
    }

    /// 絞り込みに一致する未読の記事をすべて既読にする。`limit` と `offset` は無視する。
    pub fn mark_all_read(&self, f: &ItemFilter) -> Result<usize> {
        let all = ItemFilter {
            limit: u32::MAX,
            offset: 0,
            unread_only: true,
            ..f.clone()
        };
        let ids: Vec<i64> = self.query_items(&all)?.into_iter().map(|i| i.id).collect();
        let tx = self.conn.unchecked_transaction()?;
        for id in &ids {
            tx.execute("UPDATE items SET read = 1 WHERE id = ?1", params![id])?;
        }
        tx.commit()?;
        Ok(ids.len())
    }

    /// `since` 以降に取得した記事を指標順に `limit` 件取り、そのうち要約がないものを返す。
    pub fn top_unsummarized(
        &self,
        category: Category,
        since: &DateTime<Utc>,
        limit: u32,
    ) -> Result<Vec<Item>> {
        let sql = format!(
            "SELECT * FROM (SELECT {ITEM_COLS} FROM items \
             WHERE items.category = ?1 AND items.fetched_at >= ?2 \
             ORDER BY items.score IS NULL, items.score DESC, \
               COALESCE(items.published_at, items.fetched_at) DESC, \
               items.fetched_at DESC, items.id DESC \
             LIMIT ?3) WHERE ai_summary IS NULL \
             ORDER BY score IS NULL, score DESC, COALESCE(published_at, fetched_at) DESC, \
               fetched_at DESC, id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![category.as_str(), ts(since), limit], row_to_item)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{at, new_item, store_with_source};

    fn titles(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    fn kws(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn upsert_dedupes_by_url_and_keeps_user_state() {
        let (store, src) = store_with_source(Category::TechJp);
        let t1 = at("2026-10-05T01:00:00Z");
        let n = store
            .upsert_items(
                &src,
                &[new_item("https://e.com/a", "A", "x", Some(10))],
                &t1,
            )
            .unwrap();
        assert_eq!(n, 1);
        let id = store.query_items(&ItemFilter::default()).unwrap()[0].id;
        store.set_read(id, true).unwrap();
        store.set_starred(id, true).unwrap();
        store.set_ai_summary(id, "要約").unwrap();

        let t2 = at("2026-10-06T01:00:00Z");
        let n = store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/a", "A changed", "y", Some(99)),
                    new_item("https://e.com/b", "B", "", None),
                ],
                &t2,
            )
            .unwrap();
        assert_eq!(n, 1);

        let a = store.get_item(id).unwrap().unwrap();
        assert_eq!(a.title, "A");
        assert_eq!(a.score, Some(99));
        assert_eq!(a.fetched_at, t1);
        assert!(a.read && a.starred);
        assert_eq!(a.ai_summary.as_deref(), Some("要約"));
        assert_eq!(a.source_name, "Test Feed");
        assert_eq!(a.category, Category::TechJp);
        assert_eq!(store.query_items(&ItemFilter::default()).unwrap().len(), 2);
    }

    #[test]
    fn refetch_without_score_keeps_old_score() {
        let (store, src) = store_with_source(Category::Github);
        let t = at("2026-10-05T01:00:00Z");
        store
            .upsert_items(&src, &[new_item("https://e.com/a", "A", "", Some(10))], &t)
            .unwrap();
        store
            .upsert_items(&src, &[new_item("https://e.com/a", "A", "", None)], &t)
            .unwrap();
        assert_eq!(
            store.query_items(&ItemFilter::default()).unwrap()[0].score,
            Some(10)
        );
    }

    #[test]
    fn filters_by_category_time_read_star_source() {
        let (store, src) = store_with_source(Category::TechJp);
        let other_id = store
            .add_source(
                crate::model::SourceKind::Hackernews,
                "HN",
                Category::Hackernews,
                "top",
            )
            .unwrap();
        let other = store.get_source(other_id).unwrap().unwrap();
        store
            .upsert_items(
                &src,
                &[new_item("https://e.com/old", "old", "", None)],
                &at("2026-10-01T00:00:00Z"),
            )
            .unwrap();
        store
            .upsert_items(
                &src,
                &[new_item("https://e.com/new", "new", "", None)],
                &at("2026-10-05T00:00:00Z"),
            )
            .unwrap();
        store
            .upsert_items(
                &other,
                &[new_item("https://e.com/hn", "hn", "", Some(5))],
                &at("2026-10-05T00:00:00Z"),
            )
            .unwrap();

        let f = ItemFilter {
            category: Some(Category::TechJp),
            ..Default::default()
        };
        assert_eq!(store.query_items(&f).unwrap().len(), 2);

        let f = ItemFilter {
            source_id: Some(other.id),
            ..Default::default()
        };
        assert_eq!(titles(&store.query_items(&f).unwrap()), vec!["hn"]);

        let f = ItemFilter {
            since: Some(at("2026-10-05T00:00:00Z")),
            until: Some(at("2026-10-06T00:00:00Z")),
            category: Some(Category::TechJp),
            ..Default::default()
        };
        assert_eq!(titles(&store.query_items(&f).unwrap()), vec!["new"]);

        let f = ItemFilter {
            until: Some(at("2026-10-05T00:00:00Z")),
            ..Default::default()
        };
        assert_eq!(titles(&store.query_items(&f).unwrap()), vec!["old"]);

        let new_id = store
            .query_items(&ItemFilter {
                text: Some("new".into()),
                ..Default::default()
            })
            .unwrap()[0]
            .id;
        store.set_read(new_id, true).unwrap();
        store.set_starred(new_id, true).unwrap();
        let f = ItemFilter {
            unread_only: true,
            ..Default::default()
        };
        assert_eq!(store.query_items(&f).unwrap().len(), 2);
        let f = ItemFilter {
            starred_only: true,
            ..Default::default()
        };
        assert_eq!(titles(&store.query_items(&f).unwrap()), vec!["new"]);
    }

    #[test]
    fn orders_and_paginates() {
        let (store, src) = store_with_source(Category::Github);
        let t = at("2026-10-05T00:00:00Z");
        let mut low = new_item("https://e.com/1", "low", "", Some(1));
        low.published_at = Some(at("2026-10-04T00:00:00Z"));
        let mut high = new_item("https://e.com/2", "high", "", Some(100));
        high.published_at = Some(at("2026-10-02T00:00:00Z"));
        let none = new_item("https://e.com/3", "none", "", None);
        store.upsert_items(&src, &[low, high, none], &t).unwrap();

        let recent = store.query_items(&ItemFilter::default()).unwrap();
        assert_eq!(titles(&recent), vec!["none", "low", "high"]);

        let f = ItemFilter {
            order: ItemOrder::Score,
            ..Default::default()
        };
        assert_eq!(
            titles(&store.query_items(&f).unwrap()),
            vec!["high", "low", "none"]
        );

        let f = ItemFilter {
            order: ItemOrder::Score,
            limit: 1,
            offset: 1,
            ..Default::default()
        };
        assert_eq!(titles(&store.query_items(&f).unwrap()), vec!["low"]);
    }

    #[test]
    fn topic_matches_japanese_english_and_short_keywords() {
        let (store, src) = store_with_source(Category::Medcare);
        let t = at("2026-10-05T00:00:00Z");
        store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/1", "認知症ケアの最前線", "", None),
                    new_item("https://e.com/2", "New Dementia drug approved", "", None),
                    new_item("https://e.com/3", "介護報酬の改定", "", None),
                    new_item("https://e.com/4", "無関係", "本文に Alzheimer が出る", None),
                    new_item("https://e.com/5", "Rust 2.0", "", None),
                ],
                &t,
            )
            .unwrap();

        let long = store
            .add_topic("認知症", &kws(&["認知症", "dementia", "alzheimer"]))
            .unwrap();
        let f = ItemFilter {
            topic_id: Some(long),
            ..Default::default()
        };
        let mut got = titles(&store.query_items(&f).unwrap())
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        got.sort();
        assert_eq!(
            got,
            vec!["New Dementia drug approved", "無関係", "認知症ケアの最前線"]
        );

        // Review Focus 1: 2文字のキーワードも一致する。
        let short = store.add_topic("介護", &kws(&["介護"])).unwrap();
        let f = ItemFilter {
            topic_id: Some(short),
            ..Default::default()
        };
        assert_eq!(
            titles(&store.query_items(&f).unwrap()),
            vec!["介護報酬の改定"]
        );

        let empty = store.add_topic("空", &[]).unwrap();
        let f = ItemFilter {
            topic_id: Some(empty),
            ..Default::default()
        };
        assert!(store.query_items(&f).unwrap().is_empty());

        let f = ItemFilter {
            topic_id: Some(9999),
            ..Default::default()
        };
        assert!(matches!(store.query_items(&f), Err(Error::Invalid(_))));
    }

    #[test]
    fn keywords_with_special_characters_match_literally() {
        // Review Focus 2
        let (store, src) = store_with_source(Category::TechIntl);
        let t = at("2026-10-05T00:00:00Z");
        store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/1", "Modern C++ tips", "", None),
                    new_item("https://e.com/2", "He said \"quoted\" words", "", None),
                    new_item("https://e.com/3", "Cats AND dogs", "", None),
                    new_item("https://e.com/4", "100% done", "", None),
                    new_item("https://e.com/5", "plain", "", None),
                ],
                &t,
            )
            .unwrap();
        for (text, want) in [
            ("C++", "Modern C++ tips"),
            ("\"quoted\"", "He said \"quoted\" words"),
            ("AND", "Cats AND dogs"),
            ("0%", "100% done"),
        ] {
            let f = ItemFilter {
                text: Some(text.to_string()),
                ..Default::default()
            };
            assert_eq!(
                titles(&store.query_items(&f).unwrap()),
                vec![want],
                "text={text}"
            );
        }
        for text in ["*", "a*", "NEAR(", "%", "_"] {
            let f = ItemFilter {
                text: Some(text.to_string()),
                ..Default::default()
            };
            assert!(store.query_items(&f).is_ok(), "text={text}");
        }
        let f = ItemFilter {
            text: Some("   ".into()),
            ..Default::default()
        };
        assert_eq!(store.query_items(&f).unwrap().len(), 5);
    }

    #[test]
    fn ascii_keywords_match_whole_words_only() {
        let (store, src) = store_with_source(Category::TechIntl);
        let t = at("2026-10-05T00:00:00Z");
        store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/1", "New AI model", "", None),
                    new_item("https://e.com/2", "Spain wins again", "", None),
                    new_item("https://e.com/3", "x", "maintainers said", None),
                    new_item("https://e.com/4", "OpenAI ships", "", None),
                    new_item("https://e.com/5", "In Rust we trust", "", None),
                    new_item("https://e.com/6", "trust issues", "", None),
                    new_item("https://e.com/7", "Rustの新機能", "", None),
                    new_item("https://e.com/8", "生成AIの活用", "", None),
                ],
                &t,
            )
            .unwrap();
        let search = |text: &str| {
            let f = ItemFilter {
                text: Some(text.to_string()),
                ..Default::default()
            };
            let mut got: Vec<String> = titles(&store.query_items(&f).unwrap())
                .into_iter()
                .map(String::from)
                .collect();
            got.sort();
            got
        };
        // "OpenAI" は単語として別物なので一致しない (単語単位照合の帰結)。
        assert_eq!(search("AI"), vec!["New AI model", "生成AIの活用"]);
        assert_eq!(search("ai"), vec!["New AI model", "生成AIの活用"]);
        assert_eq!(search("Rust"), vec!["In Rust we trust", "Rustの新機能"]);
        assert_eq!(search("trust"), vec!["In Rust we trust", "trust issues"]);

        let topic = store.add_topic("AI", &kws(&["AI"])).unwrap();
        let f = ItemFilter {
            topic_id: Some(topic),
            ..Default::default()
        };
        let items = store.query_items(&f).unwrap();
        let mut got = titles(&items);
        got.sort();
        assert_eq!(got, vec!["New AI model", "生成AIの活用"]);
    }

    #[test]
    fn unscored_items_order_by_published_time_under_score_order() {
        let (store, src) = store_with_source(Category::TechJp);
        let t = at("2026-10-05T00:00:00Z");
        let mut items = Vec::new();
        for (i, published) in ["2026-10-04", "2026-10-03", "2026-10-02"]
            .iter()
            .enumerate()
        {
            let mut it = new_item(&format!("https://e.com/{i}"), published, "", None);
            it.published_at = Some(at(&format!("{published}T00:00:00Z")));
            items.push(it);
        }
        store.upsert_items(&src, &items, &t).unwrap();
        let want = vec!["2026-10-04", "2026-10-03", "2026-10-02"];

        let f = ItemFilter {
            order: ItemOrder::Score,
            ..Default::default()
        };
        assert_eq!(titles(&store.query_items(&f).unwrap()), want);
        let top = store.top_unsummarized(Category::TechJp, &t, 10).unwrap();
        assert_eq!(titles(&top), want);
    }

    #[test]
    fn top_unsummarized_takes_top_n_then_drops_summarized() {
        let (store, src) = store_with_source(Category::Github);
        let t = at("2026-10-05T00:00:00Z");
        store
            .upsert_items(
                &src,
                &[
                    new_item("https://e.com/1", "s30", "", Some(30)),
                    new_item("https://e.com/2", "s20", "", Some(20)),
                    new_item("https://e.com/3", "s10", "", Some(10)),
                ],
                &t,
            )
            .unwrap();
        store
            .upsert_items(
                &src,
                &[new_item("https://e.com/0", "old", "", Some(999))],
                &at("2026-10-01T00:00:00Z"),
            )
            .unwrap();

        let top = store.top_unsummarized(Category::Github, &t, 2).unwrap();
        assert_eq!(titles(&top), vec!["s30", "s20"]);

        store.set_ai_summary(top[0].id, "済").unwrap();
        let top = store.top_unsummarized(Category::Github, &t, 2).unwrap();
        assert_eq!(titles(&top), vec!["s20"]);
        assert!(
            store
                .top_unsummarized(Category::Arxiv, &t, 2)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn mark_all_read_respects_filter_and_ignores_limit() {
        let (store, src) = store_with_source(Category::TechJp);
        let t = at("2026-10-05T00:00:00Z");
        let items: Vec<NewItem> = (0..5)
            .map(|i| new_item(&format!("https://e.com/{i}"), &format!("t{i}"), "", None))
            .collect();
        store.upsert_items(&src, &items, &t).unwrap();
        let other_id = store
            .add_source(
                crate::model::SourceKind::Hackernews,
                "HN",
                Category::Hackernews,
                "top",
            )
            .unwrap();
        let other = store.get_source(other_id).unwrap().unwrap();
        store
            .upsert_items(&other, &[new_item("https://e.com/hn", "hn", "", None)], &t)
            .unwrap();

        let n = store
            .mark_all_read(&ItemFilter {
                category: Some(Category::TechJp),
                limit: 2,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(n, 5);
        let unread = store
            .query_items(&ItemFilter {
                unread_only: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(titles(&unread), vec!["hn"]);
        let n = store
            .mark_all_read(&ItemFilter {
                category: Some(Category::TechJp),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn deleting_source_keeps_items() {
        let (store, src) = store_with_source(Category::TechJp);
        store
            .upsert_items(
                &src,
                &[new_item("https://e.com/a", "A", "", None)],
                &at("2026-10-05T00:00:00Z"),
            )
            .unwrap();
        store.delete_source(src.id).unwrap();
        let items = store.query_items(&ItemFilter::default()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source_id, None);
        assert_eq!(items[0].source_name, "Test Feed");
    }
}
