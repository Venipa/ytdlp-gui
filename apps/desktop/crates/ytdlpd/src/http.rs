//! The HTTP client gpui loads thumbnails with: reqwest on the state runtime.
// shortcut: the disk cache keeps every successful GET as one file named by a hash of its link and
// prunes files untouched for 30 days at start; add a size cap when the folder grows too large.

use std::future::Future;
use std::io::Read as _;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::{Duration, SystemTime};

use anyhow::{Result, anyhow};
use gpui::http_client::http::HeaderValue;
use gpui::http_client::{AsyncBody, HttpClient, Inner, Request, Response, Url};
use tokio::runtime::Handle;

const USER_AGENT: &str = "ytdlpd";

type Sent = Pin<Box<dyn Future<Output = Result<Response<AsyncBody>>> + Send>>;

/// Sends gpui's requests through reqwest.
pub struct Client {
    inner: reqwest::Client,
    handle: Handle,
    user_agent: HeaderValue,
    /// Where fetched images are kept between runs.
    cache: Option<PathBuf>,
}

/// How long an untouched cached file lives.
const KEEP: Duration = Duration::from_secs(30 * 24 * 3600);

/// A stable file name for `link` (FNV-1a, 64 bit).
fn cache_name(link: &str) -> String {
    let hash = link.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}.img")
}

impl Client {
    /// A client that runs its requests on `handle` and keeps what it fetches in `cache`.
    pub fn new(handle: Handle, cache: Option<PathBuf>) -> Self {
        if let Some(dir) = cache.clone() {
            handle.spawn(async move {
                let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
                    return;
                };
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let old = entry
                        .metadata()
                        .await
                        .and_then(|m| m.modified())
                        .is_ok_and(|at| {
                            SystemTime::now().duration_since(at).unwrap_or_default() > KEEP
                        });
                    if old {
                        tokio::fs::remove_file(entry.path()).await.ok();
                    }
                }
            });
        }
        // reqwest has no built-in crypto provider; an error only means one is installed already
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let _guard = handle.enter();
        Self {
            inner: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .unwrap_or_default(),
            handle: handle.clone(),
            user_agent: HeaderValue::from_static(USER_AGENT),
            cache,
        }
    }
}

impl HttpClient for Client {
    fn user_agent(&self) -> Option<&HeaderValue> {
        Some(&self.user_agent)
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }

    fn send(&self, request: Request<AsyncBody>) -> Sent {
        let client = self.inner.clone();
        let handle = self.handle.clone();
        let cache = self.cache.clone();
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let body = read(body)?;
            let file = cache
                .filter(|_| parts.method == gpui::http_client::http::Method::GET)
                .map(|dir| dir.join(cache_name(&parts.uri.to_string())));
            let fetch = handle.spawn(async move {
                if let Some(file) = &file
                    && let Ok(bytes) = tokio::fs::read(file).await
                {
                    return Ok((reqwest::StatusCode::OK, bytes.into()));
                }
                let mut outgoing = client
                    .request(parts.method, parts.uri.to_string())
                    .headers(parts.headers);
                if let Some(body) = body {
                    outgoing = outgoing.body(body);
                }
                let incoming = outgoing.send().await.map_err(reqwest::Error::without_url)?;
                let status = incoming.status();
                let bytes = incoming.bytes().await?;
                if let Some(file) = file.filter(|_| status.is_success()) {
                    if let Some(dir) = file.parent() {
                        tokio::fs::create_dir_all(dir).await.ok();
                    }
                    if let Err(error) = tokio::fs::write(&file, &bytes).await {
                        log::warn!("http: cannot cache {}: {error}", file.display());
                    }
                }
                Ok::<_, anyhow::Error>((status, bytes))
            });
            let (status, bytes) = fetch.await??;
            Ok(Response::builder()
                .status(status)
                .body(AsyncBody::from(bytes))?)
        })
    }
}

fn read(body: AsyncBody) -> Result<Option<Vec<u8>>> {
    match body.0 {
        Inner::Empty => Ok(None),
        Inner::Bytes(mut cursor) => {
            let mut bytes = Vec::new();
            cursor.read_to_end(&mut bytes)?;
            Ok(Some(bytes))
        }
        Inner::AsyncReader(_) => Err(anyhow!("streaming request bodies are not supported")),
    }
}

#[cfg(test)]
mod tests {
    use super::cache_name;

    #[test]
    fn cache_names_are_stable_and_distinct() {
        assert_eq!(cache_name("https://a/b.jpg"), cache_name("https://a/b.jpg"));
        assert_ne!(cache_name("https://a/b.jpg"), cache_name("https://a/c.jpg"));
        assert_eq!(cache_name("").len(), 20);
    }
}
