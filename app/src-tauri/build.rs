fn main() {
    embed_google_oauth();
    tauri_build::build()
}

/// `google-oauth.json` (git 管理外) の `client_id` と `client_secret` をビルド時に埋め込む。
/// ファイルが無い、または読めない場合は何も出さず、同期は無効になる。
/// 監視は常に出す。ファイルが無いと Cargo は毎回このスクリプトを実行し直すので、
/// 後からファイルを置いても次のビルドで埋め込まれる。
fn embed_google_oauth() {
    println!("cargo:rerun-if-changed=google-oauth.json");
    let Ok(text) = std::fs::read_to_string("google-oauth.json") else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        println!("cargo:warning=google-oauth.json is not valid JSON; sync is disabled");
        return;
    };
    for (key, env) in [
        ("client_id", "INFODESK_GOOGLE_CLIENT_ID"),
        ("client_secret", "INFODESK_GOOGLE_CLIENT_SECRET"),
    ] {
        if let Some(v) = value.get(key).and_then(|v| v.as_str())
            && !v.is_empty()
        {
            println!("cargo:rustc-env={env}={v}");
        }
    }
}
