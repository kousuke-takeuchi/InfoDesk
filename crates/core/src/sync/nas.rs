//! NAS 同期の設定キーと文。アプリとサーバー (`infodesk tick` / `infodesk sync`) で共有する。

use std::net::Ipv4Addr;

pub const SETTING_ENABLED: &str = "sync.enabled";
pub const SETTING_DEVICE_NAME: &str = "sync.device_name";
pub const SETTING_LAST_SYNC_AT: &str = "sync.last_sync_at";
pub const SETTING_LAST_COMPACTION_AT: &str = "sync.last_compaction_at";
pub const SETTING_BACKEND: &str = "sync.backend";
pub const SETTING_NAS_URL: &str = "sync.nas.url";
pub const SETTING_NAS_USER: &str = "sync.nas.user";
/// 保存先が変わった。次の同期の始めに、向こうのファイルの記録を捨てて最初から書き出す。
pub const SETTING_RESET_PENDING: &str = "sync.reset_pending";
/// 既定。`nas` 以外の値 (古い `google` も) はすべてこれとして読む。
pub const BACKEND_GDRIVE: &str = "gdrive";
pub const BACKEND_NAS: &str = "nas";
/// WebDAV クライアントを作れなかった。詳細 (プロキシの資格情報を含みうる) は出さない。
pub const NAS_PREPARE_MESSAGE: &str = "NAS への接続を準備できません";
pub const NAS_INCOMPLETE_MESSAGE: &str = "NAS の URL、ユーザー名、パスワードを入力してください";
/// 通信できなかった失敗 (`DriveError::Retryable`) に出す文。詳細は画面に出さない。
pub const NETWORK_MESSAGE: &str = "ネットワークに接続できません。次の同期で再試行します。";
/// LAN の NAS の URL が使えないとき。名前は Pod から引けず、LAN の外に出るおそれもあるので IP に限る。
pub const NAS_LAN_URL_MESSAGE: &str =
    "NAS の URL は LAN の IP アドレスで書いてください (例: http://192.168.0.151:5005/infodesk/)";

/// クラスターから LAN の NAS へ直接つなぐときの URL を確かめる。http で、ホストが
/// プライベートな IPv4 (10/8、172.16/12、192.168/16) のときだけ通す。それ以外には何も送らない。
pub fn lan_nas_url(raw: &str) -> Result<String, &'static str> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|_| NAS_LAN_URL_MESSAGE)?;
    // 資格情報は URL に書かせない (ログや設定に残る)。パスワードは環境変数から渡す。
    if url.scheme() != "http" || !url.username().is_empty() || url.password().is_some() {
        return Err(NAS_LAN_URL_MESSAGE);
    }
    let ip: Ipv4Addr = url
        .host_str()
        .and_then(|h| h.parse().ok())
        .ok_or(NAS_LAN_URL_MESSAGE)?;
    if !ip.is_private() {
        return Err(NAS_LAN_URL_MESSAGE);
    }
    Ok(url.to_string())
}

/// 保存先の切り替えの予約があれば、向こうのファイルの記録を捨て、まとめ直しを求める。捨てたら `true`。
/// 途中で失敗しても予約は最後に消すので、次の同期でやり直せる。アプリとサーバーで共有する。
pub fn take_reset(s: &crate::store::Store) -> crate::Result<bool> {
    if s.get_setting(SETTING_RESET_PENDING)?.as_deref() != Some("true") {
        return Ok(false);
    }
    s.clear_sync_files()?;
    s.set_setting(SETTING_LAST_COMPACTION_AT, "")?;
    s.set_setting(SETTING_RESET_PENDING, "")?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ipv4_urls_are_accepted_as_they_are() {
        for raw in [
            "http://192.168.0.151:5005/infodesk/",
            "http://10.0.0.5:5005/x/",
            "http://172.16.3.4:5005/x/",
            "http://172.31.255.255:5005/x/",
            "http://192.168.255.255:5005/x/",
        ] {
            assert_eq!(lan_nas_url(raw).unwrap(), raw, "{raw}");
        }
    }

    #[test]
    fn names_public_addresses_https_and_ipv6_are_refused() {
        // Review Focus 4
        for raw in [
            "http://takeuchi-nas:5005/infodesk/",
            "http://nas.example.com:5005/x/",
            "http://8.8.8.8:5005/x/",
            "http://172.32.0.1:5005/x/",
            "http://172.15.255.255:5005/x/",
            "http://127.0.0.1:5005/x/",
            "http://169.254.1.1:5005/x/",
            "ftp://192.168.0.151/x/",
            "http://u:secret@192.168.0.151:5005/x/",
            "http://u@192.168.0.151:5005/x/",
            "http://:secret@192.168.0.151:5005/x/",
            "https://192.168.0.151:5006/infodesk/",
            "http://[fd00::5]:5005/x/",
            "not a url",
            "",
        ] {
            assert_eq!(lan_nas_url(raw), Err(NAS_LAN_URL_MESSAGE), "{raw:?}");
        }
    }
}
