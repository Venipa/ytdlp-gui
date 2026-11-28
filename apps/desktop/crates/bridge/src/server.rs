//! Accept loop and one session per socket.
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use futures::{SinkExt as _, StreamExt as _};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast::error::RecvError;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::{Error as WsError, Message, http};

use crate::store::{Stored, hash_token, now};
use crate::wire::{AddReq, PROTO, Resp, WireError};
use crate::{Ctl, Inner, lock};

pub(crate) const MAX_CONNECTIONS: usize = 8;
const MAX_FRAME: usize = 64 * 1024;
const MAX_URL: usize = 2048;
const MAX_PRESET: usize = 64;
const AUTH_DEADLINE: Duration = Duration::from_secs(5);
const HOST_TIMEOUT: Duration = Duration::from_secs(15);
const ADD_PER_MIN: usize = 20;
const CHECK_PER_MIN: usize = 120;
const MAX_FAILS: u8 = 3;

pub(crate) async fn accept_loop(inner: Arc<Inner>, listener: TcpListener) {
    let mut ctl = inner.ctl.subscribe();
    let mut last_warn: Option<Instant> = None;
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        // a broken listener fails every call at once: at most one line per 10 s
                        if last_warn.is_none_or(|t| t.elapsed() >= Duration::from_secs(10)) {
                            last_warn = Some(Instant::now());
                            log::warn!("bridge: cannot accept a connection: {error}");
                        }
                        continue;
                    }
                };
                match inner.slots.clone().try_acquire_owned() {
                    Ok(permit) => {
                        let inner = inner.clone();
                        tokio::spawn(async move {
                            session(inner, stream).await;
                            drop(permit);
                        });
                    }
                    Err(_) => log::warn!("bridge: too many connections, dropping one"),
                }
            }
            c = ctl.recv() => if matches!(c, Ok(Ctl::Stop) | Err(RecvError::Closed)) { break },
        }
    }
}

/// Origin of an allowed extension, or why the handshake is refused.
fn vet(inner: &Inner, req: &Request) -> Result<String, &'static str> {
    let h = |n: &str| req.headers().get(n).and_then(|v| v.to_str().ok());
    let origin = h("origin").ok_or("no Origin")?;
    let ok = match origin.split_once("://") {
        Some(("chrome-extension", id)) => inner.cfg.allowed_chrome_ids.contains(&id),
        Some(("moz-extension", id)) => !id.is_empty() && !id.contains('/'),
        _ => false,
    };
    if !ok {
        return Err("Origin not allowed");
    }
    let port = inner.port;
    match h("host") {
        Some(x) if x == format!("127.0.0.1:{port}") || x == format!("localhost:{port}") => {
            Ok(origin.to_owned())
        }
        _ => Err("Host not allowed"),
    }
}

fn forbidden() -> ErrorResponse {
    http::Response::builder()
        .status(403)
        .body(Some("forbidden".into()))
        .unwrap_or_default()
}

/// At most `max` hits per minute.
struct Window {
    hits: VecDeque<Instant>,
    max: usize,
}

impl Window {
    fn new(max: usize) -> Self {
        Self {
            hits: VecDeque::new(),
            max,
        }
    }

    /// `Err(ms)` = wait this long.
    fn hit(&mut self) -> Result<(), u64> {
        let now = Instant::now();
        let minute = Duration::from_secs(60);
        while self.hits.front().is_some_and(|t| now - *t >= minute) {
            self.hits.pop_front();
        }
        if self.hits.len() >= self.max {
            let wait = minute - (now - self.hits[0]);
            return Err(wait.as_millis() as u64);
        }
        self.hits.push_back(now);
        Ok(())
    }
}

struct Conn {
    inner: Arc<Inner>,
    id: u64,
    origin: String,
    authed: Option<String>,
    fails: u8,
    adds: Window,
    checks: Window,
}

/// What to send and whether to close afterwards.
struct Out {
    frame: String,
    close: Option<u16>,
}

fn reply(r: Resp, id: Option<u64>) -> Out {
    Out {
        frame: r.frame(id),
        close: None,
    }
}

fn err(id: Option<u64>, code: &str, msg: &str) -> Out {
    reply(Resp::Error(WireError::new(code, msg)), id)
}

fn wire_err(id: Option<u64>, e: WireError) -> Out {
    reply(Resp::Error(e), id)
}

fn rate_limited(id: Option<u64>, ms: u64) -> Out {
    let mut e = WireError::new("rate_limited", "too many requests");
    e.retry_after_ms = Some(ms);
    wire_err(id, e)
}

fn clean_url(u: &str) -> Option<String> {
    if u.len() > MAX_URL {
        return None;
    }
    cli::deeplink::http_url(u)
}

impl Conn {
    /// A failed pair/auth; the third closes the socket.
    fn fail(&mut self, id: Option<u64>, code: &str, msg: &str) -> Out {
        self.fails += 1;
        let mut o = err(id, code, msg);
        if self.fails >= MAX_FAILS {
            o.close = Some(4401);
        }
        o
    }

    fn set_authed(&mut self, hash: String) {
        lock(&self.inner.conns).insert(self.id, Some(hash.clone()));
        self.authed = Some(hash);
    }

    async fn handle(&mut self, text: &str) -> Out {
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return err(None, "bad_request", "not json");
        };
        let id = v.get("id").and_then(Value::as_u64);
        if v.get("v").and_then(Value::as_u64) != Some(u64::from(PROTO)) {
            let mut o = err(id, "version", &format!("speaks protocol {PROTO} only"));
            o.close = Some(4400);
            return o;
        }
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        let Some(kind) = s("type") else {
            return err(id, "bad_request", "no type");
        };
        if self.authed.is_none() && !matches!(kind.as_str(), "hello" | "pair" | "auth" | "ping") {
            let mut o = err(id, "unauthorized", "pair or authenticate first");
            o.close = Some(4401);
            return o;
        }
        // revoke must not depend on the broadcast having reached this session
        if let Some(h) = &self.authed
            && matches!(kind.as_str(), "check" | "add")
            && !lock(&self.inner.store).devices.iter().any(|d| d.hash == *h)
        {
            let mut o = err(id, "unauthorized", "this browser was unpaired");
            o.close = Some(4403);
            return o;
        }
        match kind.as_str() {
            "hello" => reply(
                Resp::HelloOk {
                    app: "ytdlpd",
                    app_version: self.inner.cfg.app_version.clone(),
                    proto: PROTO,
                    instance: self.inner.instance.clone(),
                    label: self.inner.cfg.label.clone(),
                },
                id,
            ),
            "ping" => reply(Resp::Pong {}, id),
            "pair" => self.pair(id, s("code"), s("device")),
            "auth" => self.auth(id, s("token")),
            "check" => {
                if let Err(ms) = self.checks.hit() {
                    return rate_limited(id, ms);
                }
                let Some(url) = s("url").and_then(|u| clean_url(&u)) else {
                    return err(id, "bad_url", "not an http(s) link");
                };
                match tokio::time::timeout(HOST_TIMEOUT, self.inner.host.check(url)).await {
                    Ok(Ok(c)) => reply(Resp::Checked(c), id),
                    Ok(Err(e)) => {
                        log::info!("bridge: check refused: {} {}", e.code, e.message);
                        wire_err(id, e)
                    }
                    Err(_) => {
                        log::warn!("bridge: the app did not answer a check in time");
                        err(id, "busy", "the app did not answer in time")
                    }
                }
            }
            "add" => {
                if let Err(ms) = self.adds.hit() {
                    return rate_limited(id, ms);
                }
                let Some(url) = s("url").and_then(|u| clean_url(&u)) else {
                    return err(id, "bad_url", "not an http(s) link");
                };
                if s("preset").is_some_and(|p| p.len() > MAX_PRESET) {
                    return err(id, "bad_request", "preset too long");
                }
                let Some(start) = v.get("start").and_then(Value::as_bool) else {
                    return err(id, "bad_request", "start must be true or false");
                };
                let req = AddReq {
                    url,
                    preset: s("preset"),
                    start,
                };
                match tokio::time::timeout(HOST_TIMEOUT, self.inner.host.add(req)).await {
                    Ok(Ok(a)) => reply(Resp::Added(a), id),
                    Ok(Err(e)) => {
                        log::info!("bridge: add refused: {} {}", e.code, e.message);
                        wire_err(id, e)
                    }
                    Err(_) => {
                        log::warn!("bridge: the app did not answer an add in time");
                        err(id, "busy", "the app did not answer in time")
                    }
                }
            }
            _ => err(id, "unknown_type", "unknown message type"),
        }
    }

    fn pair(&mut self, id: Option<u64>, code: Option<String>, device: Option<String>) -> Out {
        let (Some(code), Some(device)) = (code, device) else {
            return err(id, "bad_request", "pair needs code and device");
        };
        // wrong, expired and "no code is open" look the same on purpose
        if !self.inner.redeem(&code) {
            log::warn!("bridge: pairing refused: wrong, used or expired code");
            return self.fail(id, "bad_code", "wrong or expired code");
        }
        let device: String = device
            .chars()
            .filter(|c| !c.is_control())
            .take(64)
            .collect();
        let mut raw = [0u8; 32];
        getrandom::fill(&mut raw).expect("the OS random source works");
        use base64::Engine as _;
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
        let hash = hash_token(&token);
        {
            let mut st = lock(&self.inner.store);
            st.devices.push(Stored {
                hash: hash.clone(),
                name: device.clone(),
                origin: self.origin.clone(),
                created: now(),
                last_seen: now(),
            });
            if let Err(e) = st.save() {
                st.devices.retain(|d| d.hash != hash);
                log::error!("bridge: {e:#}");
                return err(id, "internal", "cannot save the pairing");
            }
        }
        self.set_authed(hash);
        log::info!("bridge: paired {device:?}");
        self.inner.host.paired(device.clone());
        reply(Resp::Paired { token, device }, id)
    }

    fn auth(&mut self, id: Option<u64>, token: Option<String>) -> Out {
        let Some(token) = token else {
            return err(id, "bad_request", "auth needs a token");
        };
        let hash = hash_token(&token);
        let name = {
            let mut st = lock(&self.inner.store);
            let origin = &self.origin;
            let found = st
                .devices
                .iter_mut()
                .find(|d| d.hash == hash && d.origin == *origin);
            found.map(|d| {
                d.last_seen = now();
                d.name.clone()
            })
        };
        match name {
            Some(device) => {
                if let Err(e) = lock(&self.inner.store).save() {
                    log::warn!("bridge: {e:#}");
                }
                self.set_authed(hash);
                log::info!("bridge: {device:?} signed in");
                reply(Resp::AuthOk { device }, id)
            }
            None => {
                log::warn!("bridge: sign-in refused: unknown token or another origin");
                self.fail(id, "bad_token", "unknown token")
            }
        }
    }
}

async fn close(tx: &mut (impl futures::Sink<Message, Error = WsError> + Unpin), code: u16) {
    let frame = CloseFrame {
        code: CloseCode::from(code),
        reason: "".into(),
    };
    tx.send(Message::Close(Some(frame))).await.ok();
    tx.close().await.ok();
}

// the handshake callback's error type is fixed by tungstenite
#[allow(clippy::result_large_err)]
async fn session(inner: Arc<Inner>, stream: TcpStream) {
    let mut origin = String::new();
    let cb = |req: &Request, resp: Response| match vet(&inner, req) {
        Ok(o) => {
            origin = o;
            Ok(resp)
        }
        Err(why) => {
            log::warn!("bridge: refused a connection: {why}");
            Err(forbidden())
        }
    };
    let cfg = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME));
    let accepted = tokio::time::timeout(
        AUTH_DEADLINE,
        tokio_tungstenite::accept_hdr_async_with_config(stream, cb, Some(cfg)),
    )
    .await;
    let Ok(Ok(ws)) = accepted else { return };
    let id = inner.next_conn.fetch_add(1, Ordering::Relaxed);
    lock(&inner.conns).insert(id, None);
    let _gone = Gone(inner.clone(), id);
    let mut conn = Conn {
        inner: inner.clone(),
        id,
        origin,
        authed: None,
        fails: 0,
        adds: Window::new(ADD_PER_MIN),
        checks: Window::new(CHECK_PER_MIN),
    };
    let (mut tx, mut rx) = ws.split();
    let mut ctl = inner.ctl.subscribe();
    let deadline = tokio::time::Instant::now() + AUTH_DEADLINE;
    loop {
        let authed = conn.authed.is_some();
        let wait_auth = async {
            if authed {
                std::future::pending().await
            } else {
                tokio::time::sleep_until(deadline).await
            }
        };
        tokio::select! {
            m = rx.next() => match m {
                Some(Ok(Message::Text(t))) => {
                    let out = conn.handle(t.as_str()).await;
                    if tx.send(Message::text(out.frame)).await.is_err() { break }
                    if let Some(c) = out.close { close(&mut tx, c).await; break }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(Message::Binary(_))) => {
                    tx.send(Message::text(err(None, "bad_request", "text frames only").frame)).await.ok();
                }
                Some(Ok(_)) => {}
                Some(Err(WsError::Capacity(_))) => {
                    tx.send(Message::text(err(None, "too_large", "frame over 64 KiB").frame)).await.ok();
                    close(&mut tx, 1009).await;
                    break;
                }
                Some(Err(_)) => break,
            },
            c = ctl.recv() => match c {
                Ok(Ctl::Stop) => { close(&mut tx, 1001).await; break }
                Ok(Ctl::Revoke(h)) if conn.authed.as_deref() == Some(h.as_str()) => {
                    close(&mut tx, 4403).await;
                    break;
                }
                Ok(Ctl::Revoke(_)) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            },
            () = wait_auth => { close(&mut tx, 4401).await; break }
        }
    }
    if conn.authed.is_some() {
        log::info!("bridge: a signed-in browser disconnected");
    }
}

/// Removes the connection from `conns` however the session ends (even a panic in a Host future).
struct Gone(Arc<Inner>, u64);

impl Drop for Gone {
    fn drop(&mut self) {
        lock(&self.0.conns).remove(&self.1);
    }
}
