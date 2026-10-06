use chrono::{DateTime, Utc};

use crate::model::{Category, NewItem, Source, SourceKind};
use crate::store::Store;

pub fn at(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

pub fn new_item(url: &str, title: &str, excerpt: &str, score: Option<i64>) -> NewItem {
    NewItem {
        url: url.to_string(),
        title: title.to_string(),
        excerpt: excerpt.to_string(),
        author: None,
        published_at: None,
        score,
    }
}

/// メモリ上のStoreと、指定カテゴリーのRSSソース1件を返す。
pub fn store_with_source(category: Category) -> (Store, Source) {
    let store = Store::open_in_memory().unwrap();
    let id = store
        .add_source(SourceKind::Rss, "Test Feed", category, "https://example.com/feed")
        .unwrap();
    let source = store.get_source(id).unwrap().unwrap();
    (store, source)
}
