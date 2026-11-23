//! Deterministic fixture HTTP server (std only): Range/206, HEAD, per-chunk throttle, virtual big file.
//! Routes: any file under the fixtures root; `/big.mp4?size=N` zero-filled; add `slow=MS` to sleep per 8 KB chunk.
#![allow(dead_code)]
use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

/// A running server (threads detach; the process exit ends them).
/// `(path, range start)` of every request, in arrival order.
pub type Log = Arc<Mutex<Vec<(String, Option<u64>)>>>;

pub struct Server {
    pub addr: SocketAddr,
    pub log: Log,
    /// `Cookie:` and `User-Agent:` header lines (as sent) of every request.
    pub cookies: Arc<Mutex<Vec<String>>>,
}

/// Failures already served per `/flaky.mp4?id=` (`fail=N`: the first N GETs are cut off halfway).
static FLAKY: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());

impl Server {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

/// Default fixtures dir of this crate.
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Bind `127.0.0.1:port` (0 = any) and serve `root` in the background.
pub fn start(root: PathBuf, port: u16) -> Server {
    let l = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    let addr = l.local_addr().unwrap();
    let log: Log = Default::default();
    let cookies: Arc<Mutex<Vec<String>>> = Default::default();
    let (lg, ck) = (log.clone(), cookies.clone());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let (root, lg, ck) = (root.clone(), lg.clone(), ck.clone());
            std::thread::spawn(move || {
                let keep = s.try_clone();
                let _ = handle(s, &root, &lg, &ck);
                // graceful close: on Windows dropping a socket with unread input sends RST, and a slow
                // (rate-limited) client then loses data it has not read yet (os error 10054)
                if let Ok(mut s) = keep {
                    let _ = s.shutdown(std::net::Shutdown::Write);
                    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
                    let _ = std::io::copy(&mut s, &mut std::io::sink());
                }
            });
        }
    });
    Server { addr, log, cookies }
}

fn ctype(p: &str) -> &'static str {
    match p.rsplit('.').next().unwrap_or("") {
        "mp4" => "video/mp4",
        "m3u8" => "application/vnd.apple.mpegurl",
        "ts" => "video/mp2t",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

fn query(q: &str, k: &str) -> Option<u64> {
    q.split('&')
        .find_map(|kv| kv.strip_prefix(&format!("{k}="))?.parse().ok())
}

fn handle(mut s: TcpStream, root: &Path, lg: &Log, ck: &Mutex<Vec<String>>) -> std::io::Result<()> {
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("/").to_string(),
    );
    let mut range: Option<(u64, Option<u64>)> = None;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if h.to_ascii_lowercase().starts_with("cookie:")
            || h.to_ascii_lowercase().starts_with("user-agent:")
        {
            ck.lock().unwrap().push(h.trim().to_string());
        }
        if let Some(v) = h.to_lowercase().strip_prefix("range: bytes=") {
            let (a, b) = v.trim().split_once('-').unwrap_or(("0", ""));
            range = a.parse().ok().map(|a| (a, b.parse().ok()));
        }
    }
    let (path, q) = target.split_once('?').unwrap_or((target.as_str(), ""));
    lg.lock()
        .unwrap()
        .push((path.to_string(), range.map(|r| r.0)));
    if path == "/limited.mp4" {
        return s.write_all(
            b"HTTP/1.1 429 Too Many Requests
Retry-After: 7
Content-Length: 0
Connection: close

",
        );
    }
    let slow = query(q, "slow").unwrap_or(0);
    let data: Vec<u8>;
    let zeros: Option<u64>;
    if path == "/big.mp4" || path == "/flaky.mp4" {
        zeros = Some(query(q, "size").unwrap_or(40 << 20));
        data = vec![];
    } else {
        zeros = None;
        let rel = path.trim_start_matches('/');
        match (rel.contains(".."), std::fs::read(root.join(rel))) {
            (false, Ok(d)) => data = d,
            _ => {
                return s.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        }
    }
    let total = zeros.unwrap_or(data.len() as u64);
    let (start, end, status) = match range {
        Some((a, b)) if a < total => (
            a,
            b.map_or(total - 1, |b| b.min(total - 1)),
            "206 Partial Content",
        ),
        _ => (0, total.saturating_sub(1), "200 OK"),
    };
    let len = if total == 0 { 0 } else { end - start + 1 };
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {}\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n",
        ctype(path)
    );
    if status.starts_with("206") {
        head += &format!("Content-Range: bytes {start}-{end}/{total}\r\n");
    }
    s.write_all(format!("{head}\r\n").as_bytes())?;
    if method == "HEAD" {
        return Ok(());
    }
    // flaky: cut the body off halfway for the first `fail` GETs of this `id`
    let mut cut = len;
    if path == "/flaky.mp4" && method == "GET" {
        let id = format!(
            "{}{}",
            s.local_addr()?,
            q.split('&').find(|p| p.starts_with("id=")).unwrap_or("")
        );
        let mut f = FLAKY.lock().unwrap();
        let i = f.iter().position(|(k, _)| *k == id).unwrap_or_else(|| {
            f.push((id, 0));
            f.len() - 1
        });
        if f[i].1 < query(q, "fail").unwrap_or(0) {
            f[i].1 += 1;
            cut = len / 2;
        }
    }
    let mut sent = 0u64;
    let zero = [0u8; 8192];
    while sent < cut {
        let n = (cut - sent).min(8192) as usize;
        if zeros.is_some() {
            s.write_all(&zero[..n])?;
        } else {
            s.write_all(&data[(start + sent) as usize..(start + sent) as usize + n])?;
        }
        sent += n as u64;
        if slow > 0 {
            std::thread::sleep(Duration::from_millis(slow));
        }
    }
    Ok(())
}
