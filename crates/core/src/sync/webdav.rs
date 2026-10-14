//! WebDAV storage (Synology NAS) for cloud sync. Files live directly in the
//! configured folder, and a file's name is its id.

use std::time::Duration;

use async_trait::async_trait;

use super::drive::{DriveError, DriveStore, RemoteFile};

#[derive(Clone, Debug)]
pub struct WebDavConfig {
    pub url: String,
    pub user: String,
    pub password: String,
    /// `socks5h://user:pass@127.0.0.1:port`. `None` connects directly (tests).
    pub proxy: Option<String>,
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

    fn file_url(&self, name: &str) -> reqwest::Url {
        let mut url = self.base.clone();
        if let Ok(mut segs) = url.path_segments_mut() {
            segs.pop_if_empty().push(name);
        }
        url
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
        let text = resp.text().await.map_err(transport)?;
        parse_multistatus(&text)
    }

    async fn stat(&self, name: &str) -> Result<RemoteFile, DriveError> {
        let entries = self.propfind(self.file_url(name), "0").await?;
        let entry = entries
            .into_iter()
            .next()
            .ok_or_else(|| DriveError::Other(format!("no properties for {name}")))?;
        to_remote(name.to_string(), entry)
    }

    async fn put(&self, name: &str, bytes: Vec<u8>, mime: &str) -> Result<RemoteFile, DriveError> {
        self.send("PUT", self.file_url(name), None, Some((bytes, mime)), false)
            .await?;
        self.stat(name).await
    }
}

const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?><D:propfind xmlns:D="DAV:"><D:prop><D:getlastmodified/><D:getcontentlength/><D:resourcetype/></D:prop></D:propfind>"#;

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

/// Parse a `207 Multi-Status` body. Matches elements by local name, so any
/// prefix for the `DAV:` namespace (`D:`, `d:`, none) works.
fn parse_multistatus(xml: &str) -> Result<Vec<Entry>, DriveError> {
    use quick_xml::XmlVersion;
    use quick_xml::events::Event;
    let bad = |e: &dyn std::fmt::Display| DriveError::Other(format!("bad PROPFIND response: {e}"));
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut entries = Vec::new();
    let mut current: Option<Entry> = None;
    let mut field: Option<String> = None;
    let mut text = String::new();
    loop {
        match reader.read_event().map_err(|e| bad(&e))? {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_string();
                match name.as_str() {
                    "response" => current = Some(Entry::default()),
                    "href" | "getlastmodified" | "getcontentlength" => {
                        field = Some(name);
                        text.clear();
                    }
                    "collection" => {
                        if let Some(c) = current.as_mut() {
                            c.is_collection = true;
                        }
                    }
                    _ => {}
                }
            }
            Event::Empty(e) => {
                if e.local_name().as_ref() == "collection"
                    && let Some(c) = current.as_mut()
                {
                    c.is_collection = true;
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
                } else if field.as_deref() == Some(name.as_str()) {
                    if let Some(c) = current.as_mut() {
                        let value = text.trim().to_string();
                        match name.as_str() {
                            "href" => c.href = value,
                            "getlastmodified" => c.last_modified = Some(value),
                            _ => c.length = value.parse().ok(),
                        }
                    }
                    field = None;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(entries)
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
            let path = percent_decode(href_path(&e.href));
            let trimmed = path.trim_end_matches('/');
            if e.is_collection || trimmed.eq_ignore_ascii_case(&folder) {
                continue;
            }
            let name = trimmed.rsplit('/').next().unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            files.push(to_remote(name, e)?);
        }
        Ok(files)
    }

    async fn download(&self, id: &str) -> Result<Vec<u8>, DriveError> {
        let resp = self
            .send("GET", self.file_url(id), None, None, false)
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
        self.send("DELETE", self.file_url(id), None, None, true)
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

    #[test]
    fn invalid_url_is_an_error() {
        assert!(WebDavStore::new(cfg("not a url".into())).is_err());
    }
}
