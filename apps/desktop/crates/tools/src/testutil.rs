//! Tiny std-only HTTP server for tests (stands in for `YTDLPD_TOOLS_MIRROR`).
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;

pub enum Route {
    Body(Vec<u8>),
    Redirect(String),
    /// Claims 100 more bytes than it sends, then closes.
    Truncated(Vec<u8>),
}

/// Bind a listener and return it with its base url (when routes need to know the url).
pub fn bind() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    (l, base)
}

/// Start serving `routes` (path -> response); returns the base url `http://127.0.0.1:<port>`.
pub fn serve(routes: Vec<(String, Route)>) -> String {
    let (l, base) = bind();
    serve_on(l, routes);
    base
}

/// Serve `routes` on an already bound listener.
pub fn serve_on(l: TcpListener, routes: Vec<(String, Route)>) {
    let routes: HashMap<String, Route> = routes.into_iter().collect();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut s = s;
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).into_owned();
            let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
            let resp: Vec<u8> = match routes.get(&path) {
                Some(Route::Body(b)) => [format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()).into_bytes(), b.clone()].concat(),
                Some(Route::Redirect(to)) => format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes(),
                Some(Route::Truncated(b)) => [format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len() + 100).into_bytes(), b.clone()].concat(),
                None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            };
            let _ = s.write_all(&resp);
        }
    });
}
