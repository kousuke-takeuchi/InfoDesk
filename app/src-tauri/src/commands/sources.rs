use infodesk_core::collector::feed;
use infodesk_core::model::{Category, Source, SourceKind};
use tauri::State;

use crate::error::AppResult;
use crate::state::AppState;

fn parse_category(s: &str) -> AppResult<Category> {
    Category::parse(s).ok_or_else(|| format!("unknown category '{s}'").into())
}

pub fn list_sources_impl(state: &AppState) -> AppResult<Vec<Source>> {
    state.with_store(|s| s.list_sources())
}

/// フィードを取得して検証してから保存する。解析できなければ保存しない。
pub async fn add_rss(
    state: &AppState,
    url: &str,
    name: Option<String>,
    category: &str,
) -> AppResult<Source> {
    let category = parse_category(category)?;
    let info = feed::validate(&state.http, url).await?;
    let name = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or(info.title);
    state.with_store(|s| {
        let id = s.add_source(SourceKind::Rss, &name, category, url)?;
        Ok(s.get_source(id)?.expect("just inserted"))
    })
}

pub fn update_source_impl(
    state: &AppState,
    id: i64,
    name: &str,
    category: &str,
    config: &str,
) -> AppResult<()> {
    let category = parse_category(category)?;
    state.with_store(|s| s.update_source(id, name, category, config))
}

pub fn set_source_enabled_impl(state: &AppState, id: i64, enabled: bool) -> AppResult<()> {
    state.with_store(|s| s.set_source_enabled(id, enabled))
}

pub fn delete_source_impl(state: &AppState, id: i64) -> AppResult<()> {
    state.with_store(|s| s.delete_source(id))
}

#[tauri::command]
pub fn list_sources(state: State<'_, AppState>) -> AppResult<Vec<Source>> {
    list_sources_impl(&state)
}

#[tauri::command]
pub async fn add_rss_source(
    state: State<'_, AppState>,
    url: String,
    name: Option<String>,
    category: String,
) -> AppResult<Source> {
    add_rss(&state, &url, name, &category).await
}

#[tauri::command]
pub fn update_source(
    state: State<'_, AppState>,
    id: i64,
    name: String,
    category: String,
    config: String,
) -> AppResult<()> {
    update_source_impl(&state, id, &name, &category, &config)
}

#[tauri::command]
pub fn set_source_enabled(state: State<'_, AppState>, id: i64, enabled: bool) -> AppResult<()> {
    set_source_enabled_impl(&state, id, enabled)
}

#[tauri::command]
pub fn delete_source(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    delete_source_impl(&state, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::test_state;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const RSS: &str = r#"<rss version="2.0"><channel><title>テスト</title><item><title>t</title><link>https://e.com/1</link></item></channel></rss>"#;

    #[tokio::test]
    async fn add_rss_validates_then_saves_with_feed_title_as_default_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/feed"))
            .respond_with(ResponseTemplate::new(200).set_body_string(RSS))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/html"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
            .mount(&server)
            .await;
        let (state, _) = test_state();
        let before = list_sources_impl(&state).unwrap().len();

        let s = add_rss(&state, &format!("{}/feed", server.uri()), None, "tech_jp")
            .await
            .unwrap();
        assert_eq!(s.name, "テスト");
        assert_eq!(s.category, Category::TechJp);
        assert_eq!(s.kind, SourceKind::Rss);
        assert_eq!(list_sources_impl(&state).unwrap().len(), before + 1);

        let named = add_rss(
            &state,
            &format!("{}/feed?x=1", server.uri()),
            Some("  名前  ".into()),
            "medcare",
        )
        .await
        .unwrap();
        assert_eq!(named.name, "名前");

        assert!(
            add_rss(&state, &format!("{}/html", server.uri()), None, "tech_jp")
                .await
                .is_err()
        );
        assert!(
            add_rss(&state, &format!("{}/feed", server.uri()), None, "nope")
                .await
                .is_err()
        );
        assert_eq!(list_sources_impl(&state).unwrap().len(), before + 2);
    }

    #[test]
    fn update_toggle_delete() {
        let (state, _) = test_state();
        let first = list_sources_impl(&state).unwrap().remove(0);
        update_source_impl(&state, first.id, "名前2", "tech_intl", &first.config).unwrap();
        set_source_enabled_impl(&state, first.id, false).unwrap();
        let s = list_sources_impl(&state).unwrap().remove(0);
        assert_eq!(s.name, "名前2");
        assert_eq!(s.category, Category::TechIntl);
        assert!(!s.enabled);
        delete_source_impl(&state, first.id).unwrap();
        assert!(
            list_sources_impl(&state)
                .unwrap()
                .iter()
                .all(|x| x.id != first.id)
        );
        assert!(update_source_impl(&state, first.id, "x", "bad", "c").is_err());
    }
}
