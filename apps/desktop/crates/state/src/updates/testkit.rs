//! A tiny HTTP fixture for the updater (tests and `examples/updates_fixture.rs`): serves a fake
//! GitHub releases list plus its assets on 127.0.0.1, so no real GitHub is needed. Point
//! `YTDLPD_UPDATES_API` at [`Fixture::base`].
use std::collections::HashMap;
use std::io::{BufRead as _, BufReader, Write as _};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use sha2::{Digest as _, Sha256};

/// Served bodies by path, each with the length the response claims.
type Files = Arc<Mutex<HashMap<String, (Vec<u8>, usize)>>>;

/// A running fixture server; it lives until the process ends.
pub struct Fixture {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    files: Files,
}

/// The lower-case hex sha256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl Fixture {
    /// Starts the server on a free port.
    pub fn start() -> std::io::Result<Self> {
        Self::start_on(0)
    }

    /// Starts the server on `port` (0 picks one).
    pub fn start_on(port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let base = format!("http://{}", listener.local_addr()?);
        let files: Files = Arc::default();
        let served = files.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let served = served.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(&stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() {
                        return;
                    }
                    // skip the headers
                    let mut header = String::new();
                    while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                        header.clear();
                    }
                    let path = line.split_whitespace().nth(1).unwrap_or("/");
                    let path = path.split('?').next().unwrap_or(path).to_string();
                    let body = served.lock().unwrap().get(&path).cloned();
                    let mut out = &stream;
                    let _ = match body {
                        Some((body, claimed)) => out
                            .write_all(
                                format!(
                                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                    claimed
                                )
                                .as_bytes(),
                            )
                            .and_then(|()| out.write_all(&body)),
                        None => out.write_all(
                            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        ),
                    };
                });
            }
        });
        Ok(Self { base, files })
    }

    /// Serves `body` at `path`.
    pub fn put(&self, path: &str, body: impl Into<Vec<u8>>) {
        let body = body.into();
        let claimed = body.len();
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), (body, claimed));
    }

    /// Serves `body` at `path` but claims `claimed` bytes, so the connection breaks off early.
    pub fn put_truncated(&self, path: &str, body: impl Into<Vec<u8>>, claimed: usize) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), (body.into(), claimed));
    }

    /// Publishes one release `tag` with `assets` (name, bytes) and a `SHA256SUMS.txt` over them;
    /// `sums` overrides the checksums text (to test mismatches). Replaces the releases list.
    pub fn release(
        &self,
        tag: &str,
        prerelease: bool,
        assets: &[(&str, &[u8])],
        sums: Option<&str>,
    ) {
        let mut listed = String::new();
        let mut entries = Vec::new();
        for (name, bytes) in assets {
            let path = format!("/download/{tag}/{name}");
            self.put(&path, bytes.to_vec());
            listed.push_str(&format!("{}  {name}\n", sha256_hex(bytes)));
            entries.push(serde_json::json!({
                "name": name, "size": bytes.len(), "browser_download_url": format!("{}{path}", self.base),
            }));
        }
        let sums_path = format!("/download/{tag}/{}", super::SUMS);
        self.put(&sums_path, sums.map(str::to_string).unwrap_or(listed));
        entries.push(serde_json::json!({
            "name": super::SUMS, "size": 0, "browser_download_url": format!("{}{sums_path}", self.base),
        }));
        let list = serde_json::json!([{
            "tag_name": tag, "prerelease": prerelease, "draft": false,
            "html_url": format!("{}/releases/{tag}", self.base), "assets": entries,
        }]);
        self.put(
            &format!("/repos/{}/releases", super::REPO),
            list.to_string(),
        );
    }
}
