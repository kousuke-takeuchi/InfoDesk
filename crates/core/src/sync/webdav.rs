//! WebDAV storage (Synology NAS) for cloud sync. Files live directly in the
//! configured folder, and a file's name is its id.

use std::time::Duration;

use async_trait::async_trait;

use super::drive::{DriveError, DriveStore, RemoteFile};

#[derive(Clone)]
pub struct WebDavConfig {
    pub url: String,
    pub user: String,
    pub password: String,
    /// `socks5h://user:pass@127.0.0.1:port`. `None` connects directly (tests).
    pub proxy: Option<String>,
}

impl std::fmt::Debug for WebDavConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebDavConfig")
            .field("url", &self.url)
            .field("user", &self.user)
            .field("password", &"<redacted>")
            .field("proxy", &self.proxy.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

pub struct WebDavStore {
    http: reqwest::Client,
    base: reqwest::Url,
    user: String,
    password: String,
}

impl WebDavStore {
    pub fn new(cfg: WebDavConfig) -> Result<Self, DriveError> {
        Self::with_timeouts(cfg, Duration::from_secs(10), Duration::from_secs(60))
    }

    fn with_timeouts(
        cfg: WebDavConfig,
        connect: Duration,
        read: Duration,
    ) -> Result<Self, DriveError> {
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(connect)
            .read_timeout(read);
        if let Some(proxy) = &cfg.proxy {
            let proxy = reqwest::Proxy::all(proxy)
                .map_err(|e| DriveError::Other(format!("invalid proxy: {e}")))?;
            builder = builder.proxy(proxy);
        } else {
            builder = builder.no_proxy();
        }
        let http = builder
            .build()
            .map_err(|e| DriveError::Other(format!("http client: {e}")))?;
        let mut base = reqwest::Url::parse(cfg.url.trim())
            .map_err(|e| DriveError::Other(format!("invalid NAS URL: {e}")))?;
        if base.cannot_be_a_base() {
            return Err(DriveError::Other("invalid NAS URL".into()));
        }
        // The folder always ends with a slash, so both spellings resolve alike.
        let path = format!("{}/", base.path().trim_end_matches('/'));
        base.set_path(&path);
        Ok(Self {
            http,
            base,
            user: cfg.user,
            password: cfg.password,
        })
    }

    fn file_url(&self, name: &str) -> Result<reqwest::Url, DriveError> {
        if !safe_name(name) {
            return Err(DriveError::Other(format!("unsafe file name {name:?}")));
        }
        let mut url = self.base.clone();
        if let Ok(mut segs) = url.path_segments_mut() {
            segs.pop_if_empty().push(name);
        }
        Ok(url)
    }

    async fn send(
        &self,
        method: &str,
        url: reqwest::Url,
        depth: Option<&str>,
        body: Option<(Vec<u8>, &str)>,
        allow_404: bool,
    ) -> Result<reqwest::Response, DriveError> {
        let method = reqwest::Method::from_bytes(method.as_bytes()).expect("valid method");
        let mut req = self
            .http
            .request(method, url)
            .basic_auth(&self.user, Some(&self.password));
        if let Some(d) = depth {
            req = req.header("Depth", d);
        }
        if let Some((bytes, content_type)) = body {
            req = req.header("Content-Type", content_type).body(bytes);
        }
        let resp = req.send().await.map_err(transport)?;
        let status = resp.status();
        if status.is_success() || (allow_404 && status.as_u16() == 404) {
            return Ok(resp);
        }
        let code = status.as_u16();
        if code == 401 {
            return Err(DriveError::BadCredentials);
        }
        if status.is_redirection() {
            return Err(DriveError::Redirected);
        }
        if code == 507 || code == 413 {
            return Err(DriveError::StorageFull);
        }
        if code == 403 {
            return Err(DriveError::Forbidden);
        }
        let text = resp.text().await.unwrap_or_default();
        Err(if code == 408 || code == 429 || status.is_server_error() {
            DriveError::Retryable(format!("HTTP {code}"))
        } else {
            DriveError::Other(format!("HTTP {code}: {}", error_text(&text)))
        })
    }

    async fn propfind(&self, url: reqwest::Url, depth: &str) -> Result<Vec<Entry>, DriveError> {
        let resp = self
            .send(
                "PROPFIND",
                url,
                Some(depth),
                Some((PROPFIND_BODY.as_bytes().to_vec(), "application/xml")),
                false,
            )
            .await?;
        if resp.status().as_u16() != 207 {
            return Err(unexpected_propfind());
        }
        let text = resp.text().await.map_err(transport)?;
        parse_multistatus(&text)
    }

    async fn stat(&self, name: &str) -> Result<RemoteFile, DriveError> {
        let entries = self.propfind(self.file_url(name)?, "0").await?;
        let entry = entries
            .into_iter()
            .next()
            .ok_or_else(|| DriveError::Other(format!("no properties for {name}")))?;
        to_remote(name.to_string(), entry)
    }

    async fn put(&self, name: &str, bytes: Vec<u8>, mime: &str) -> Result<RemoteFile, DriveError> {
        self.send(
            "PUT",
            self.file_url(name)?,
            None,
            Some((bytes, mime)),
            false,
        )
        .await?;
        self.stat(name).await
    }
}

const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?><D:propfind xmlns:D="DAV:"><D:prop><D:getlastmodified/><D:getcontentlength/><D:resourcetype/></D:prop></D:propfind>"#;

fn unexpected_propfind() -> DriveError {
    DriveError::Other("unexpected PROPFIND response".into())
}

/// A name is one plain path segment: no separators, no `.` or `..`.
fn safe_name(name: &str) -> bool {
    !(name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\'))
}

fn transport(e: reqwest::Error) -> DriveError {
    DriveError::Retryable(e.to_string())
}

fn error_text(body: &str) -> String {
    body.chars().take(200).collect()
}

#[derive(Debug, Default)]
struct Entry {
    href: String,
    last_modified: Option<String>,
    length: Option<u64>,
    is_collection: bool,
}

fn to_remote(name: String, e: Entry) -> Result<RemoteFile, DriveError> {
    let raw = e
        .last_modified
        .ok_or_else(|| DriveError::Other(format!("file {name} has no getlastmodified")))?;
    let modified_time = chrono::DateTime::parse_from_rfc2822(raw.trim())
        .map_err(|err| DriveError::Other(format!("bad getlastmodified {raw:?}: {err}")))?
        .with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    Ok(RemoteFile {
        id: name.clone(),
        name,
        modified_time,
        size: e.length,
    })
}

/// Percent-decode a URL path component. Invalid escapes are kept as written.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

/// The path part of an `href`, which may be absolute (`http://host/path`).
fn href_path(href: &str) -> &str {
    let href = href.trim();
    match href.find("://") {
        Some(i) => match href[i + 3..].find('/') {
            Some(j) => &href[i + 3 + j..],
            None => "/",
        },
        None => href,
    }
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Parse a `207 Multi-Status` body. Matches elements by local name, so any
/// prefix for the `DAV:` namespace (`D:`, `d:`, none) works.
fn parse_multistatus(xml: &str) -> Result<Vec<Entry>, DriveError> {
    use quick_xml::XmlVersion;
    use quick_xml::events::Event;
    let bad = |e: &dyn std::fmt::Display| DriveError::Other(format!("bad PROPFIND response: {e}"));
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut entries = Vec::new();
    let mut current: Option<Entry> = None;
    let mut seen_root = false;
    let mut prop: Option<PropStat> = None;
    let mut field: Option<String> = None;
    let mut text = String::new();
    loop {
        match reader.read_event().map_err(|e| bad(&e))? {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_string();
                match name.as_str() {
                    "multistatus" => seen_root = true,
                    "response" => current = Some(Entry::default()),
                    "propstat" => prop = Some(PropStat::default()),
                    "href" | "getlastmodified" | "getcontentlength" | "status" => {
                        field = Some(name);
                        text.clear();
                    }
                    "collection" => {
                        if let Some(p) = prop.as_mut() {
                            p.is_collection = true;
                        }
                    }
                    _ => {}
                }
            }
            Event::Empty(e) => {
                let name = e.local_name().as_ref().to_string();
                if name == "multistatus" {
                    seen_root = true;
                } else if name == "collection"
                    && let Some(p) = prop.as_mut()
                {
                    p.is_collection = true;
                }
            }
            Event::Text(t) => {
                if field.is_some() {
                    text.push_str(&t.xml_content(XmlVersion::Implicit1_0));
                }
            }
            Event::GeneralRef(r) => {
                if field.is_some() {
                    if let Ok(Some(c)) = r.resolve_char_ref() {
                        text.push(c);
                    } else if let Some(s) = quick_xml::escape::resolve_xml_entity(
                        &r.xml_content(XmlVersion::Implicit1_0),
                    ) {
                        text.push_str(s);
                    }
                }
            }
            Event::End(e) => {
                let name = e.local_name().as_ref().to_string();
                if name == "response" {
                    if let Some(c) = current.take() {
                        entries.push(c);
                    }
                } else if name == "propstat" {
                    if let (Some(p), Some(c)) = (prop.take(), current.as_mut())
                        && p.ok()
                    {
                        c.is_collection |= p.is_collection;
                        c.last_modified = c.last_modified.take().or(p.last_modified);
                        c.length = c.length.or(p.length);
                    }
                } else if field.as_deref() == Some(name.as_str()) {
                    let value = text.trim().to_string();
                    match name.as_str() {
                        "href" => {
                            if let Some(c) = current.as_mut() {
                                c.href = value;
                            }
                        }
                        "getlastmodified" => {
                            if let Some(p) = prop.as_mut() {
                                p.last_modified = Some(value);
                            }
                        }
                        "getcontentlength" => {
                            if let Some(p) = prop.as_mut() {
                                p.length = value.parse().ok();
                            }
                        }
                        _ => {
                            if let Some(p) = prop.as_mut() {
                                p.status = Some(value);
                            }
                        }
                    }
                    field = None;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !seen_root {
        return Err(unexpected_propfind());
    }
    Ok(entries)
}

/// One `propstat`: only one whose status is 200 (or absent) is trusted.
#[derive(Default)]
struct PropStat {
    last_modified: Option<String>,
    length: Option<u64>,
    is_collection: bool,
    status: Option<String>,
}

impl PropStat {
    fn ok(&self) -> bool {
        match &self.status {
            None => true,
            Some(s) => s.split_whitespace().nth(1) == Some("200"),
        }
    }
}

#[async_trait]
impl DriveStore for WebDavStore {
    async fn list(&self) -> Result<Vec<RemoteFile>, DriveError> {
        let entries = self.propfind(self.base.clone(), "1").await?;
        let folder = percent_decode(self.base.path())
            .trim_end_matches('/')
            .to_string();
        let mut files = Vec::new();
        for e in entries {
            if e.is_collection {
                continue;
            }
            let path = percent_decode(href_path(&e.href));
            let trimmed = path.trim_end_matches('/');
            // Only the part that comes from the base URL is case-insensitive
            // (Synology share names are).
            let Some(rest) = strip_prefix_ci(trimmed, &folder) else {
                continue;
            };
            let Some(name) = rest.strip_prefix('/') else {
                continue; // the folder itself, or an unrelated sibling
            };
            if !safe_name(name) {
                continue;
            }
            // A file without a usable timestamp is skipped, not fatal.
            if let Ok(f) = to_remote(name.to_string(), e) {
                files.push(f);
            }
        }
        Ok(files)
    }

    async fn download(&self, id: &str) -> Result<Vec<u8>, DriveError> {
        let resp = self
            .send("GET", self.file_url(id)?, None, None, false)
            .await?;
        Ok(resp.bytes().await.map_err(transport)?.to_vec())
    }

    async fn create(
        &self,
        name: &str,
        bytes: Vec<u8>,
        mime: &str,
    ) -> Result<RemoteFile, DriveError> {
        self.put(name, bytes, mime).await
    }

    async fn update(&self, id: &str, bytes: Vec<u8>, mime: &str) -> Result<RemoteFile, DriveError> {
        self.put(id, bytes, mime).await
    }

    async fn delete(&self, id: &str) -> Result<(), DriveError> {
        self.send("DELETE", self.file_url(id)?, None, None, true)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_bytes, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cfg(url: String) -> WebDavConfig {
        WebDavConfig {
            url,
            user: "kou".into(),
            password: "pw".into(),
            proxy: None,
        }
    }

    fn store(url: String) -> WebDavStore {
        WebDavStore::new(cfg(url)).unwrap()
    }

    const SYNOLOGY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
<D:response>
<D:href>/InfoDesk/</D:href>
<D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:00:00 GMT</D:getlastmodified><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
</D:response>
<D:response>
<D:href>/InfoDesk/day-2026-10-14.json.gz</D:href>
<D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified><D:getcontentlength>2048</D:getcontentlength><D:resourcetype></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
</D:response>
<D:response>
<D:href>/InfoDesk/snapshot.json.gz</D:href>
<D:propstat><D:prop><D:getlastmodified>Tue, 13 Oct 2026 23:59:59 GMT</D:getlastmodified><D:getcontentlength>10</D:getcontentlength><D:resourcetype/></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
</D:response>
</D:multistatus>"#;

    fn multistatus(prefix: &str, ns_decl: &str) -> String {
        let p = |t: &str| {
            if prefix.is_empty() {
                t.to_string()
            } else {
                format!("{prefix}:{t}")
            }
        };
        format!(
            r#"<?xml version="1.0"?><{ms} {ns}><{r}><{h}>/InfoDesk/</{h}><{ps}><{pr}><{rt}><{c}/></{rt}></{pr}></{ps}></{r}><{r}><{h}>/InfoDesk/a.json</{h}><{ps}><{pr}><{lm}>Wed, 14 Oct 2026 03:21:09 GMT</{lm}><{cl}>5</{cl}></{pr}></{ps}></{r}></{ms}>"#,
            ms = p("multistatus"),
            r = p("response"),
            h = p("href"),
            ps = p("propstat"),
            pr = p("prop"),
            rt = p("resourcetype"),
            c = p("collection"),
            lm = p("getlastmodified"),
            cl = p("getcontentlength"),
            ns = ns_decl,
        )
    }

    async fn propfind_mock(s: &MockServer, p: &str, body: String) {
        Mock::given(method("PROPFIND"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(207).set_body_string(body))
            .mount(s)
            .await;
    }

    #[tokio::test]
    async fn list_parses_propfind_and_skips_the_folder_itself() {
        let s = MockServer::start().await;
        Mock::given(method("PROPFIND"))
            .and(path("/InfoDesk/"))
            .and(header("depth", "1"))
            .and(header("authorization", "Basic a291OnB3"))
            .respond_with(ResponseTemplate::new(207).set_body_string(SYNOLOGY))
            .mount(&s)
            .await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        let files = d.list().await.unwrap();
        assert_eq!(
            files,
            vec![
                RemoteFile {
                    id: "day-2026-10-14.json.gz".into(),
                    name: "day-2026-10-14.json.gz".into(),
                    modified_time: "2026-10-14T03:21:09Z".into(),
                    size: Some(2048),
                },
                RemoteFile {
                    id: "snapshot.json.gz".into(),
                    name: "snapshot.json.gz".into(),
                    modified_time: "2026-10-13T23:59:59Z".into(),
                    size: Some(10),
                },
            ]
        );
    }

    #[tokio::test]
    async fn list_accepts_other_dav_prefixes() {
        for (prefix, ns) in [
            ("d", r#"xmlns:d="DAV:""#),
            ("", r#"xmlns="DAV:""#),
            ("lp1", r#"xmlns:lp1="DAV:""#),
        ] {
            let s = MockServer::start().await;
            propfind_mock(&s, "/InfoDesk/", multistatus(prefix, ns)).await;
            let d = store(format!("{}/InfoDesk/", s.uri()));
            let files = d.list().await.unwrap();
            assert_eq!(files.len(), 1, "prefix {prefix:?}");
            assert_eq!(files[0].name, "a.json");
            assert_eq!(files[0].size, Some(5));
        }
    }

    #[tokio::test]
    async fn names_are_percent_decoded() {
        let body = r#"<?xml version="1.0"?><D:multistatus xmlns:D="DAV:">
<D:response><D:href>/My%20Folder/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop></D:propstat></D:response>
<D:response><D:href>/My%20Folder/day-2026-10-14.json.gz</D:href><D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified><D:getcontentlength>1</D:getcontentlength></D:prop></D:propstat></D:response>
<D:response><D:href>/My%20Folder/%E6%97%A5%E8%A8%98%20%E3%83%A1%E3%83%A2.json</D:href><D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified><D:getcontentlength>2</D:getcontentlength></D:prop></D:propstat></D:response>
<D:response><D:href>/My%20Folder/a&amp;b.json</D:href><D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified></D:prop></D:propstat></D:response>
</D:multistatus>"#;
        let s = MockServer::start().await;
        propfind_mock(&s, "/My%20Folder/", body.to_string()).await;
        let d = store(format!("{}/My%20Folder", s.uri()));
        let names: Vec<String> = d.list().await.unwrap().into_iter().map(|f| f.id).collect();
        assert_eq!(
            names,
            vec!["day-2026-10-14.json.gz", "日記 メモ.json", "a&b.json"]
        );
    }

    #[tokio::test]
    async fn put_creates_and_updates_by_name() {
        let s = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/InfoDesk/%E6%97%A5%20a.json"))
            .and(header("content-type", "application/json"))
            .and(body_bytes(b"{}".to_vec()))
            .respond_with(ResponseTemplate::new(201))
            .expect(2)
            .mount(&s)
            .await;
        Mock::given(method("PROPFIND"))
            .and(path("/InfoDesk/%E6%97%A5%20a.json"))
            .and(header("depth", "0"))
            .respond_with(ResponseTemplate::new(207).set_body_string(
                r#"<D:multistatus xmlns:D="DAV:"><D:response><D:href>/InfoDesk/%E6%97%A5%20a.json</D:href><D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified><D:getcontentlength>2</D:getcontentlength></D:prop></D:propstat></D:response></D:multistatus>"#,
            ))
            .mount(&s)
            .await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        let c = d
            .create("日 a.json", b"{}".to_vec(), "application/json")
            .await
            .unwrap();
        assert_eq!(c.id, "日 a.json");
        assert_eq!(c.size, Some(2));
        assert_eq!(c.modified_time, "2026-10-14T03:21:09Z");
        let u = d
            .update("日 a.json", b"{}".to_vec(), "application/json")
            .await
            .unwrap();
        assert_eq!(u.id, "日 a.json");
    }

    #[tokio::test]
    async fn get_downloads() {
        let s = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/InfoDesk/a.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1u8, 2, 3]))
            .mount(&s)
            .await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        assert_eq!(d.download("a.json").await.unwrap(), vec![1u8, 2, 3]);
        assert!(matches!(
            d.download("missing.json").await,
            Err(DriveError::Other(_))
        ));
    }

    #[tokio::test]
    async fn delete_treats_404_as_done() {
        let s = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/InfoDesk/a.json"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&s)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/InfoDesk/gone.json"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&s)
            .await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        d.delete("a.json").await.unwrap();
        d.delete("gone.json").await.unwrap();
    }

    #[tokio::test]
    async fn unauthorized_is_bad_credentials() {
        let s = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(401))
            .mount(&s)
            .await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        assert!(matches!(d.list().await, Err(DriveError::BadCredentials)));
        assert!(matches!(
            d.download("a").await,
            Err(DriveError::BadCredentials)
        ));
        assert!(matches!(
            d.create("a", vec![], "x").await,
            Err(DriveError::BadCredentials)
        ));
        assert_eq!(
            DriveError::BadCredentials.to_string(),
            "NAS のユーザー名かパスワードが違います"
        );
    }

    #[tokio::test]
    async fn server_errors_and_timeouts_are_retryable() {
        let s = MockServer::start().await;
        Mock::given(method("PROPFIND"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&s)
            .await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        assert!(matches!(d.list().await, Err(DriveError::Retryable(_))));

        let slow = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(600)))
            .mount(&slow)
            .await;
        let d = WebDavStore::with_timeouts(
            cfg(format!("{}/InfoDesk", slow.uri())),
            Duration::from_millis(200),
            Duration::from_millis(100),
        )
        .unwrap();
        assert!(matches!(
            d.download("a").await,
            Err(DriveError::Retryable(_))
        ));

        // Nothing listens here: a refused connection is retryable too.
        let d = store("http://127.0.0.1:1/InfoDesk".into());
        assert!(matches!(d.list().await, Err(DriveError::Retryable(_))));
    }

    #[tokio::test]
    async fn url_with_or_without_trailing_slash_resolves_the_same_files() {
        let s = MockServer::start().await;
        propfind_mock(&s, "/InfoDesk/", multistatus("D", r#"xmlns:D="DAV:""#)).await;
        Mock::given(method("GET"))
            .and(path("/InfoDesk/a.json"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![7u8]))
            .mount(&s)
            .await;
        for url in [
            format!("{}/InfoDesk", s.uri()),
            format!("{}/InfoDesk/", s.uri()),
        ] {
            let d = store(url);
            let files = d.list().await.unwrap();
            assert_eq!(files.len(), 1);
            assert_eq!(d.download(&files[0].id).await.unwrap(), vec![7u8]);
        }
    }

    fn one_file_body(extra: &str) -> String {
        format!(
            r#"<?xml version="1.0"?><D:multistatus xmlns:D="DAV:">{extra}<D:response><D:href>/InfoDesk/a.json</D:href><D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified><D:getcontentlength>5</D:getcontentlength></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response></D:multistatus>"#
        )
    }

    async fn list_names(body: String) -> Vec<String> {
        let s = MockServer::start().await;
        propfind_mock(&s, "/InfoDesk/", body).await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        d.list().await.unwrap().into_iter().map(|f| f.id).collect()
    }

    #[tokio::test]
    async fn unsafe_names_are_rejected_without_a_request() {
        let s = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(204))
            .expect(0)
            .mount(&s)
            .await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        for name in ["..", ".", "", "a/b", "a\\b", "../x"] {
            assert!(
                matches!(d.delete(name).await, Err(DriveError::Other(_))),
                "{name:?}"
            );
        }
    }

    #[tokio::test]
    async fn list_drops_entries_with_unsafe_names() {
        let body = one_file_body(
            r#"<D:response><D:href>/InfoDesk/a%5Cb.json</D:href><D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified></D:prop></D:propstat></D:response>"#,
        );
        assert_eq!(list_names(body).await, vec!["a.json"]);
    }

    #[tokio::test]
    async fn propfind_must_be_a_real_multistatus() {
        let s = MockServer::start().await;
        Mock::given(method("PROPFIND"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&s)
            .await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        assert!(
            matches!(d.list().await, Err(DriveError::Other(m)) if m == "unexpected PROPFIND response")
        );

        let s = MockServer::start().await;
        Mock::given(method("PROPFIND"))
            .respond_with(ResponseTemplate::new(207).set_body_string("<html>hi</html>"))
            .mount(&s)
            .await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        assert!(
            matches!(d.list().await, Err(DriveError::Other(m)) if m == "unexpected PROPFIND response")
        );
    }

    #[test]
    fn debug_redacts_secrets() {
        let c = WebDavConfig {
            url: "http://nas/x".into(),
            user: "kou".into(),
            password: "s3cretpw".into(),
            proxy: Some("socks5h://u:proxysecret@127.0.0.1:1055".into()),
        };
        let out = format!("{c:?}");
        assert!(!out.contains("s3cretpw"), "{out}");
        assert!(!out.contains("proxysecret"), "{out}");
    }

    #[tokio::test]
    async fn subfolder_entries_are_skipped() {
        let body = one_file_body(
            r#"<D:response><D:href>/InfoDesk/sub/</D:href><D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified><D:resourcetype><D:collection/></D:resourcetype></D:prop></D:propstat></D:response>"#,
        );
        assert_eq!(list_names(body).await, vec!["a.json"]);
    }

    #[tokio::test]
    async fn the_folders_own_entry_is_skipped_by_path_alone() {
        // No resourcetype, so only the path comparison can recognise it.
        for href in [
            "http://host/InfoDesk",
            "/InfoDesk",
            "/%49nfoDesk/",
            "/infodesk/",
            "https://host:5006/InfoDesk/",
        ] {
            let body = one_file_body(&format!(
                r#"<D:response><D:href>{href}</D:href><D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified></D:prop></D:propstat></D:response>"#
            ));
            assert_eq!(list_names(body).await, vec!["a.json"], "{href}");
        }
    }

    #[tokio::test]
    async fn only_the_base_path_is_case_insensitive() {
        let body = one_file_body(
            r#"<D:response><D:href>/infodesk/B.json</D:href><D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified></D:prop></D:propstat></D:response>"#,
        );
        assert_eq!(list_names(body).await, vec!["B.json", "a.json"]);
    }

    #[tokio::test]
    async fn redirects_are_not_followed() {
        let s = MockServer::start().await;
        Mock::given(method("PROPFIND"))
            .respond_with(ResponseTemplate::new(301).insert_header("location", "/elsewhere/"))
            .mount(&s)
            .await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        let err = d.list().await.unwrap_err();
        // Shown as is, without the generic "保存先のエラー: " prefix.
        assert_eq!(
            err.to_string(),
            "NAS の URL を確かめてください (転送されました)"
        );
    }

    #[tokio::test]
    async fn full_and_forbidden_have_friendly_errors() {
        for (code, msg) in [
            (507, "NAS の空き容量が足りません"),
            (413, "NAS の空き容量が足りません"),
            (403, "NAS のフォルダに書き込む権限がありません"),
        ] {
            let s = MockServer::start().await;
            Mock::given(method("PUT"))
                .respond_with(ResponseTemplate::new(code))
                .mount(&s)
                .await;
            let d = store(format!("{}/InfoDesk", s.uri()));
            let err = d.create("a", vec![], "x").await.unwrap_err();
            assert_eq!(err.to_string(), msg, "{code}");
            assert!(!matches!(err, DriveError::Other(_)), "{code}");
        }
    }

    #[test]
    fn storage_error_texts_are_neutral() {
        assert_eq!(
            DriveError::Other("x".into()).to_string(),
            "保存先のエラー: x"
        );
        assert_eq!(
            DriveError::Retryable("x".into()).to_string(),
            "保存先に接続できません: x"
        );
    }

    #[tokio::test]
    async fn entries_without_a_usable_modified_time_are_skipped() {
        let body = one_file_body(
            r#"<D:response><D:href>/InfoDesk/nolm.json</D:href><D:propstat><D:prop><D:getcontentlength>1</D:getcontentlength></D:prop></D:propstat></D:response>
<D:response><D:href>/InfoDesk/bad.json</D:href><D:propstat><D:prop><D:getlastmodified>yesterday</D:getlastmodified></D:prop></D:propstat></D:response>"#,
        );
        assert_eq!(list_names(body).await, vec!["a.json"]);
    }

    #[tokio::test]
    async fn only_the_200_propstat_is_read() {
        let body = r#"<D:multistatus xmlns:D="DAV:"><D:response><D:href>/InfoDesk/a.json</D:href>
<D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified><D:getcontentlength>5</D:getcontentlength></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
<D:propstat><D:prop><D:getcontentlength>999</D:getcontentlength><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 404 Not Found</D:status></D:propstat>
</D:response></D:multistatus>"#;
        let s = MockServer::start().await;
        propfind_mock(&s, "/InfoDesk/", body.to_string()).await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        let files = d.list().await.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].size, Some(5));
    }

    #[tokio::test]
    async fn size_is_none_without_content_length() {
        let body = r#"<D:multistatus xmlns:D="DAV:"><D:response><D:href>/InfoDesk/a.json</D:href><D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response></D:multistatus>"#;
        let s = MockServer::start().await;
        propfind_mock(&s, "/InfoDesk/", body.to_string()).await;
        let d = store(format!("{}/InfoDesk", s.uri()));
        assert_eq!(d.list().await.unwrap()[0].size, None);
    }

    #[test]
    fn a_url_without_a_port_uses_the_scheme_default() {
        let d = store("http://nas/InfoDesk".into());
        let url = d.file_url("a.json").unwrap();
        assert_eq!(url.as_str(), "http://nas/InfoDesk/a.json");
        assert_eq!(url.port(), None);
        assert_eq!(url.port_or_known_default(), Some(80));
    }

    #[tokio::test]
    async fn absolute_hrefs_with_a_port_are_listed() {
        let body = one_file_body(
            r#"<D:response><D:href>http://nas.tail1234.ts.net:5005/InfoDesk/b.json</D:href><D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified></D:prop></D:propstat></D:response>
<D:response><D:href>http://100.100.1.5:5005/Other/c.json</D:href><D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:21:09 GMT</D:getlastmodified></D:prop></D:propstat></D:response>"#,
        );
        assert_eq!(list_names(body).await, vec!["b.json", "a.json"]);
    }

    #[test]
    fn socks5h_proxy_with_credentials_is_accepted() {
        let mut c = cfg("http://nas.ts.net:5005/InfoDesk".into());
        c.proxy = Some("socks5h://u:p@127.0.0.1:1055".into());
        assert!(WebDavStore::new(c).is_ok());
    }

    #[test]
    fn invalid_url_is_an_error() {
        assert!(WebDavStore::new(cfg("not a url".into())).is_err());
    }
}
