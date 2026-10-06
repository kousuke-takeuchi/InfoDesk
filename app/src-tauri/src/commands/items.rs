use std::collections::HashMap;

use infodesk_core::model::{Category, Item, ItemFilter};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::error::AppResult;
use crate::state::{AppState, EVENT_ITEMS};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SidebarCounts {
    pub total_unread: i64,
    pub categories: HashMap<String, i64>,
    pub topics: HashMap<i64, i64>,
}

pub fn list_items_impl(state: &AppState, filter: ItemFilter) -> AppResult<Vec<Item>> {
    state.with_store(|s| s.query_items(&filter))
}

pub fn get_item_impl(state: &AppState, id: i64) -> AppResult<Option<Item>> {
    state.with_store(|s| s.get_item(id))
}

pub fn set_read_impl(state: &AppState, id: i64, read: bool) -> AppResult<()> {
    state.with_store(|s| s.set_read(id, read))?;
    state.notify(EVENT_ITEMS);
    Ok(())
}

pub fn set_starred_impl(state: &AppState, id: i64, starred: bool) -> AppResult<()> {
    state.with_store(|s| s.set_starred(id, starred))?;
    state.notify(EVENT_ITEMS);
    Ok(())
}

pub fn mark_all_read_impl(state: &AppState, filter: ItemFilter) -> AppResult<usize> {
    let n = state.with_store(|s| s.mark_all_read(&filter))?;
    state.notify(EVENT_ITEMS);
    Ok(n)
}

/// 記事を既読にし、開くURLを返す。http/https 以外のスキームは拒否する。
pub fn open_item_url(state: &AppState, id: i64) -> AppResult<String> {
    let item = get_item_impl(state, id)?.ok_or_else(|| format!("item {id} not found"))?;
    let scheme = item.url.split_once(':').map_or("", |(s, _)| s);
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(format!("unsupported URL scheme {scheme:?} for item {id}").into());
    }
    set_read_impl(state, id, true)?;
    Ok(item.url)
}

pub fn sidebar_counts_impl(state: &AppState) -> AppResult<SidebarCounts> {
    state.with_store(|s| {
        let unread = ItemFilter {
            unread_only: true,
            ..Default::default()
        };
        let total_unread = s.count_items(&unread)?;
        let mut categories = HashMap::new();
        for c in Category::ALL {
            let f = ItemFilter {
                category: Some(c),
                ..unread.clone()
            };
            categories.insert(c.as_str().to_string(), s.count_items(&f)?);
        }
        let mut topics = HashMap::new();
        for t in s.list_topics()? {
            let f = ItemFilter {
                topic_id: Some(t.id),
                ..unread.clone()
            };
            topics.insert(t.id, s.count_items(&f)?);
        }
        Ok(SidebarCounts {
            total_unread,
            categories,
            topics,
        })
    })
}

#[tauri::command]
pub fn list_items(state: State<'_, AppState>, filter: ItemFilter) -> AppResult<Vec<Item>> {
    list_items_impl(&state, filter)
}

#[tauri::command]
pub fn get_item(state: State<'_, AppState>, id: i64) -> AppResult<Option<Item>> {
    get_item_impl(&state, id)
}

#[tauri::command]
pub fn set_read(state: State<'_, AppState>, id: i64, read: bool) -> AppResult<()> {
    set_read_impl(&state, id, read)
}

#[tauri::command]
pub fn set_starred(state: State<'_, AppState>, id: i64, starred: bool) -> AppResult<()> {
    set_starred_impl(&state, id, starred)
}

#[tauri::command]
pub fn mark_all_read(state: State<'_, AppState>, filter: ItemFilter) -> AppResult<usize> {
    mark_all_read_impl(&state, filter)
}

#[tauri::command]
pub fn open_item(app: AppHandle, state: State<'_, AppState>, id: i64) -> AppResult<()> {
    let url = open_item_url(&state, id)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn sidebar_counts(state: State<'_, AppState>) -> AppResult<SidebarCounts> {
    sidebar_counts_impl(&state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;
    use infodesk_core::model::NewItem;

    fn seed_items(state: &AppState) -> Vec<i64> {
        state
            .with_store(|s| {
                let src = s
                    .list_sources()?
                    .into_iter()
                    .find(|x| x.category == Category::Medcare)
                    .unwrap();
                let now = chrono::Utc::now();
                let items = ["認知症の新薬", "介護報酬", "Rust 2.0"]
                    .iter()
                    .enumerate()
                    .map(|(i, t)| NewItem {
                        url: format!("https://e.com/{i}"),
                        title: t.to_string(),
                        excerpt: String::new(),
                        author: None,
                        published_at: None,
                        score: None,
                    })
                    .collect::<Vec<_>>();
                s.upsert_items(&src, &items, &now)?;
                let mut ids: Vec<i64> = s
                    .query_items(&ItemFilter::default())?
                    .into_iter()
                    .map(|i| i.id)
                    .collect();
                // query_items returns newest first; tests index by insertion order
                ids.sort_unstable();
                Ok(ids)
            })
            .unwrap()
    }

    #[test]
    fn read_and_star_notify_items_changed() {
        let (state, events) = test_state();
        let ids = seed_items(&state);
        set_read_impl(&state, ids[0], true).unwrap();
        set_starred_impl(&state, ids[1], true).unwrap();
        let item = get_item_impl(&state, ids[0]).unwrap().unwrap();
        assert!(item.read);
        assert!(get_item_impl(&state, ids[1]).unwrap().unwrap().starred);
        assert_eq!(
            events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| *e == EVENT_ITEMS)
                .count(),
            2
        );
    }

    #[test]
    fn sidebar_counts_unread_per_category_and_topic() {
        let (state, _) = test_state();
        let ids = seed_items(&state);
        let topic = state
            .with_store(|s| s.add_topic("認知症", &["認知症".to_string()]))
            .unwrap();
        set_read_impl(&state, ids[2], true).unwrap();
        let c = sidebar_counts_impl(&state).unwrap();
        assert_eq!(c.total_unread, 2);
        assert_eq!(c.categories["medcare"], 2);
        assert_eq!(c.categories["github"], 0);
        assert_eq!(c.topics[&topic], 1);
    }

    #[test]
    fn mark_all_read_uses_filter() {
        let (state, _) = test_state();
        seed_items(&state);
        let n = mark_all_read_impl(
            &state,
            ItemFilter {
                category: Some(Category::Medcare),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(n, 3);
        assert_eq!(sidebar_counts_impl(&state).unwrap().total_unread, 0);
    }

    #[test]
    fn open_item_marks_read_and_returns_url() {
        let (state, _) = test_state();
        let ids = seed_items(&state);
        let url = open_item_url(&state, ids[0]).unwrap();
        assert_eq!(url, "https://e.com/0");
        assert!(get_item_impl(&state, ids[0]).unwrap().unwrap().read);
        assert!(open_item_url(&state, 9999).is_err());
    }

    #[test]
    fn open_item_rejects_non_http_scheme() {
        let (state, _) = test_state();
        let id = state
            .with_store(|s| {
                let src = s
                    .list_sources()?
                    .into_iter()
                    .find(|x| x.category == Category::Medcare)
                    .unwrap();
                let item = NewItem {
                    url: "file:///etc/hosts".to_string(),
                    title: "local".to_string(),
                    excerpt: String::new(),
                    author: None,
                    published_at: None,
                    score: None,
                };
                s.upsert_items(&src, &[item], &chrono::Utc::now())?;
                Ok(s.query_items(&ItemFilter::default())?[0].id)
            })
            .unwrap();
        let err = open_item_url(&state, id).unwrap_err();
        assert!(format!("{err:?}").contains("file"), "{err:?}");
        assert!(!get_item_impl(&state, id).unwrap().unwrap().read);
    }
}
