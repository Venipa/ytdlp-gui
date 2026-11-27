//! Handshake, pairing, auth and limits, with a tokio-tungstenite client and a fake `Host`.
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use futures::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

use super::*;

const A: &str = "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "chrome-extension://bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const IDS: &[&str] = &[
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
];

#[derive(Default)]
struct Fake {
    paired: Mutex<Vec<String>>,
    adds: AtomicUsize,
}

impl Host for Fake {
    fn check(&self, url: String) -> BoxFuture<'static, Result<Checked, WireError>> {
        Box::pin(async move {
            Ok(Checked {
                url,
                supported: true,
                extractor: Some("Youtube".into()),
                name: Some("youtube".into()),
                kind: Kind::Video,
                presets: vec![PresetInfo {
                    id: "video:1080".into(),
                    label: "Video 1080p".into(),
                }],
                preset: Some("video:1080".into()),
            })
        })
    }
    fn add(&self, req: AddReq) -> BoxFuture<'static, Result<Added, WireError>> {
        self.adds.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            Ok(Added {
                download: req.start.then_some(7),
                state: if req.start {
                    AddState::Queued
                } else {
                    AddState::Intake
                },
                duplicate: false,
            })
        })
    }
    fn paired(&self, device: String) {
        self.paired.lock().unwrap().push(device);
    }
}

struct Rig {
    bridge: Bridge,
    host: Arc<Fake>,
    dir: tempfile::TempDir,
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let p = free_port();
    let host = Arc::new(Fake::default());
    let bridge = start(
        Config {
            data_dir: dir.path().into(),
            ports: p..=p,
            allowed_chrome_ids: IDS,
            app_version: "1.0.0".into(),
            label: "ytdlpd.test".into(),
        },
        host.clone(),
    )
    .unwrap();
    Rig { bridge, host, dir }
}

type Ws = WebSocketStream<TcpStream>;

async fn try_connect(port: u16, origin: Option<&str>, host: Option<&str>) -> Result<Ws, WsError> {
    let mut req = format!("ws://127.0.0.1:{port}/")
        .into_client_request()
        .unwrap();
    if let Some(o) = origin {
        req.headers_mut().insert("origin", o.parse().unwrap());
    }
    if let Some(h) = host {
        req.headers_mut().insert("host", h.parse().unwrap());
    }
    let s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    tokio_tungstenite::client_async(req, s)
        .await
        .map(|(ws, _)| ws)
}

async fn connect(r: &Rig, origin: &str) -> Ws {
    try_connect(r.bridge.port(), Some(origin), None)
        .await
        .unwrap()
}

fn status(e: WsError) -> u16 {
    match e {
        WsError::Http(r) => r.status().as_u16(),
        other => panic!("not an http refusal: {other:?}"),
    }
}

async fn send(ws: &mut Ws, v: Value) {
    let mut v = v;
    v["v"] = json!(1);
    ws.send(Message::text(v.to_string())).await.unwrap();
}

/// A JSON reply, or `Err(close code)` (0 = dropped without a code).
async fn recv(ws: &mut Ws) -> Result<Value, u16> {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("no answer")
        {
            Some(Ok(Message::Text(t))) => return Ok(serde_json::from_str(t.as_str()).unwrap()),
            Some(Ok(Message::Close(f))) => return Err(f.map_or(0, |f| u16::from(f.code))),
            Some(Ok(_)) => {}
            Some(Err(_)) | None => return Err(0),
        }
    }
}

async fn call(ws: &mut Ws, v: Value) -> Value {
    send(ws, v).await;
    recv(ws).await.expect("a reply")
}

fn code_of(v: &Value) -> &str {
    v["code"].as_str().unwrap()
}

/// Pairs a fresh socket from `origin`; returns the token.
async fn pair(r: &Rig, origin: &str) -> String {
    let code = r.bridge.new_code().code;
    let mut ws = connect(r, origin).await;
    let v = call(
        &mut ws,
        json!({"type":"pair","code":code,"device":"Test PC"}),
    )
    .await;
    assert_eq!(v["type"], "paired", "{v}");
    v["token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn handshake_checks_origin_and_host() {
    let r = rig();
    let p = r.bridge.port();
    assert_eq!(status(try_connect(p, None, None).await.err().unwrap()), 403);
    for bad in [
        "http://evil.example",
        "https://127.0.0.1",
        "null",
        "file://",
        "chrome-extension://cccccccccccccccccccccccccccccccc",
        "moz-extension://",
    ] {
        assert_eq!(
            status(try_connect(p, Some(bad), None).await.err().unwrap()),
            403,
            "{bad}"
        );
    }
    // DNS rebinding: right Origin, wrong Host
    assert_eq!(
        status(
            try_connect(p, Some(A), Some("evil.example"))
                .await
                .err()
                .unwrap()
        ),
        403
    );
    assert_eq!(
        status(
            try_connect(p, Some(A), Some("127.0.0.1:1"))
                .await
                .err()
                .unwrap()
        ),
        403
    );
    // accepted
    try_connect(p, Some(A), None).await.unwrap();
    try_connect(p, Some("moz-extension://1234-abcd"), None)
        .await
        .unwrap();
    try_connect(p, Some(A), Some(&format!("localhost:{p}")))
        .await
        .unwrap();
}

#[tokio::test]
async fn listens_on_loopback_only() {
    let r = rig();
    // the address a default route would use; no packet is sent by `connect` on UDP
    let udp = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
    if udp.connect("192.0.2.1:9").is_err() {
        return;
    }
    let ip = udp.local_addr().unwrap().ip();
    if ip.is_loopback() || ip.is_unspecified() {
        return;
    }
    let t = tokio::time::timeout(
        Duration::from_secs(2),
        TcpStream::connect((ip, r.bridge.port())),
    )
    .await;
    assert!(!matches!(t, Ok(Ok(_))), "reachable on {ip}");
}

#[tokio::test]
async fn hello_and_version() {
    let r = rig();
    let mut ws = connect(&r, A).await;
    let v = call(
        &mut ws,
        json!({"type":"hello","id":5,"client":"chrome","ext":"0.1.0"}),
    )
    .await;
    assert_eq!(v["type"], "hello_ok");
    assert_eq!(v["id"], 5);
    assert_eq!(v["app"], "ytdlpd");
    assert_eq!(v["proto"], 1);
    assert_eq!(v["label"], "ytdlpd.test");
    assert_eq!(v["instance"].as_str().unwrap().len(), 16);
    assert!(
        v.get("auth").is_none() && v.get("pair_open").is_none(),
        "no pairing oracle"
    );
    ws.send(Message::text(json!({"v":2,"type":"hello"}).to_string()))
        .await
        .unwrap();
    assert_eq!(code_of(&recv(&mut ws).await.unwrap()), "version");
    assert_eq!(recv(&mut ws).await, Err(4400));
}

#[tokio::test]
async fn nothing_works_before_auth() {
    let r = rig();
    for t in ["check", "add"] {
        let mut ws = connect(&r, A).await;
        let v = call(
            &mut ws,
            json!({"type":t,"url":"https://youtube.com/watch?v=1","start":true}),
        )
        .await;
        assert_eq!(code_of(&v), "unauthorized");
        assert_eq!(recv(&mut ws).await, Err(4401));
    }
    assert_eq!(r.host.adds.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn pairing_then_auth_then_use() {
    let r = rig();
    let token = pair(&r, A).await;
    assert_eq!(token.len(), 43, "32 bytes, base64url");
    assert_eq!(r.host.paired.lock().unwrap().as_slice(), ["Test PC"]);
    let file = std::fs::read_to_string(r.dir.path().join("extension.json")).unwrap();
    assert!(!file.contains(&token), "only the hash is stored");
    assert!(file.contains(A), "origin is pinned");

    let mut ws = connect(&r, A).await;
    assert_eq!(
        call(&mut ws, json!({"type":"auth","token":token})).await["type"],
        "auth_ok"
    );
    assert_eq!(call(&mut ws, json!({"type":"ping"})).await["type"], "pong");
    assert_eq!(
        call(&mut ws, json!({"type":"nope"})).await["code"],
        "unknown_type"
    );
    let c = call(
        &mut ws,
        json!({"type":"check","id":1,"url":"https://youtube.com/watch?v=1"}),
    )
    .await;
    assert_eq!(
        (
            c["type"].as_str(),
            c["supported"].as_bool(),
            c["preset"].as_str()
        ),
        (Some("checked"), Some(true), Some("video:1080"))
    );
    let a = call(
        &mut ws,
        json!({"type":"add","url":"https://youtube.com/watch?v=1","start":true,"preset":null}),
    )
    .await;
    assert_eq!(
        (
            a["type"].as_str(),
            a["download"].as_i64(),
            a["state"].as_str()
        ),
        (Some("added"), Some(7), Some("queued"))
    );
    assert_eq!(
        call(
            &mut ws,
            json!({"type":"add","url":"https://youtube.com/watch?v=1"})
        )
        .await["code"],
        "bad_request"
    );
    let d = r.bridge.devices();
    assert_eq!(
        (d.len(), d[0].connected, d[0].origin.as_str()),
        (1, true, A)
    );

    // a restart keeps the pairing
    let again = store::Store::load(r.dir.path());
    assert_eq!(again.devices.len(), 1);
}

#[tokio::test]
async fn code_is_single_use_expires_and_burns() {
    let r = rig();
    let code = r.bridge.new_code().code;
    assert_eq!(code.len(), 8);
    assert!(code.bytes().all(|b| b.is_ascii_digit()));
    let mut ws = connect(&r, A).await;
    assert_eq!(
        call(&mut ws, json!({"type":"pair","code":code,"device":"x"})).await["type"],
        "paired"
    );
    let mut ws2 = connect(&r, A).await;
    assert_eq!(
        code_of(&call(&mut ws2, json!({"type":"pair","code":code,"device":"x"})).await),
        "bad_code"
    );

    // three wrong tries burn the live code, even for the right guess afterwards
    let code = r.bridge.new_code().code;
    let wrong = if code == "00000000" {
        "11111111"
    } else {
        "00000000"
    };
    for _ in 0..3 {
        let mut w = connect(&r, A).await;
        assert_eq!(
            code_of(&call(&mut w, json!({"type":"pair","code":wrong,"device":"x"})).await),
            "bad_code"
        );
    }
    let mut w = connect(&r, A).await;
    assert_eq!(
        code_of(&call(&mut w, json!({"type":"pair","code":code,"device":"x"})).await),
        "bad_code"
    );
    // "New code" resets
    let code = r.bridge.new_code().code;
    let mut w = connect(&r, A).await;
    assert_eq!(
        call(&mut w, json!({"type":"pair","code":code,"device":"x"})).await["type"],
        "paired"
    );

    // expiry
    let code = r.bridge.new_code().code;
    lock(&r.bridge.inner.code).as_mut().unwrap().expires = Instant::now() - Duration::from_secs(1);
    let mut w = connect(&r, A).await;
    assert_eq!(
        code_of(&call(&mut w, json!({"type":"pair","code":code,"device":"x"})).await),
        "bad_code"
    );
}

#[tokio::test]
async fn no_open_code_answers_like_a_wrong_one() {
    let r = rig();
    let mut w = connect(&r, A).await;
    let none = call(
        &mut w,
        json!({"type":"pair","code":"12345678","device":"x"}),
    )
    .await;
    r.bridge.new_code();
    let mut w = connect(&r, A).await;
    let wrong = call(
        &mut w,
        json!({"type":"pair","code":"00000000","device":"x"}),
    )
    .await;
    let wrong = if wrong["type"] == "paired" {
        none.clone()
    } else {
        wrong
    };
    assert_eq!(
        (
            none["type"].as_str(),
            none["code"].as_str(),
            none["message"].as_str()
        ),
        (
            wrong["type"].as_str(),
            wrong["code"].as_str(),
            wrong["message"].as_str()
        )
    );
    r.bridge.cancel_code();
}

#[tokio::test]
async fn failures_never_lock_out_a_paired_browser() {
    let r = rig();
    let token = pair(&r, A).await;
    // a connection that keeps failing is closed on the third strike
    let mut w = connect(&r, A).await;
    for i in 0..3 {
        let v = call(&mut w, json!({"type":"auth","token":"nope"})).await;
        assert_eq!(code_of(&v), "bad_token", "{i}");
    }
    assert_eq!(recv(&mut w).await, Err(4401));
    // but a flood of bad pairs from other sockets never blocks auth
    for _ in 0..12 {
        let mut w = connect(&r, A).await;
        call(
            &mut w,
            json!({"type":"pair","code":"00000000","device":"x"}),
        )
        .await;
    }
    let mut ok = connect(&r, A).await;
    assert_eq!(
        call(&mut ok, json!({"type":"auth","token":token})).await["type"],
        "auth_ok"
    );
}

#[tokio::test]
async fn token_is_pinned_to_its_origin() {
    let r = rig();
    let token = pair(&r, A).await;
    let mut other = connect(&r, B).await;
    assert_eq!(
        code_of(&call(&mut other, json!({"type":"auth","token":token})).await),
        "bad_token"
    );
}

#[tokio::test]
async fn frame_and_url_caps() {
    let r = rig();
    let token = pair(&r, A).await;
    let mut ws = connect(&r, A).await;
    call(&mut ws, json!({"type":"auth","token":token})).await;
    for bad in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "not a url",
        &format!("https://a.b/{}", "x".repeat(2100)),
    ] {
        assert_eq!(
            code_of(&call(&mut ws, json!({"type":"check","url":bad})).await),
            "bad_url",
            "{}",
            &bad[..bad.len().min(30)]
        );
    }
    ws.send(Message::text("x".repeat(70 * 1024))).await.ok();
    match recv(&mut ws).await {
        Ok(v) => {
            assert_eq!(code_of(&v), "too_large");
            // under load the close frame can be lost to the reset (0)
            assert!(matches!(recv(&mut ws).await, Err(1009 | 0)));
        }
        Err(code) => assert!(code == 1009 || code == 0),
    }
}

#[tokio::test]
async fn add_and_check_are_rate_limited() {
    let r = rig();
    let token = pair(&r, A).await;
    let mut ws = connect(&r, A).await;
    call(&mut ws, json!({"type":"auth","token":token})).await;
    let url = "https://youtube.com/watch?v=1";
    for i in 0..20 {
        assert_eq!(
            call(&mut ws, json!({"type":"add","url":url,"start":true})).await["type"],
            "added",
            "{i}"
        );
    }
    let v = call(&mut ws, json!({"type":"add","url":url,"start":true})).await;
    assert_eq!(code_of(&v), "rate_limited");
    assert!(v["retry_after_ms"].as_u64().unwrap() > 0);
    assert_eq!(r.host.adds.load(Ordering::SeqCst), 20);
    for _ in 0..120 {
        assert_eq!(
            call(&mut ws, json!({"type":"check","url":url})).await["type"],
            "checked"
        );
    }
    assert_eq!(
        code_of(&call(&mut ws, json!({"type":"check","url":url})).await),
        "rate_limited"
    );
}

#[tokio::test]
async fn port_falls_back_and_is_loopback() {
    // hold port p, keep p+1 free for the fallback
    let (p, held) = (0..50)
        .find_map(|_| {
            let p = free_port();
            let held = TcpListener::bind(("127.0.0.1", p)).ok()?;
            drop(TcpListener::bind(("127.0.0.1", p.checked_add(1)?)).ok()?);
            Some((p, held))
        })
        .expect("two adjacent free ports");
    let dir = tempfile::tempdir().unwrap();
    let b = start(
        Config {
            data_dir: dir.path().into(),
            ports: p..=p + 1,
            allowed_chrome_ids: IDS,
            app_version: "1".into(),
            label: "t".into(),
        },
        Arc::new(Fake::default()),
    )
    .unwrap();
    assert_eq!(b.port(), p + 1);
    drop(held);
    let none = start(
        Config {
            data_dir: dir.path().into(),
            ports: b.port()..=b.port(),
            allowed_chrome_ids: IDS,
            app_version: "1".into(),
            label: "t".into(),
        },
        Arc::new(Fake::default()),
    );
    assert!(none.is_err(), "a fully occupied range is an error");
}

#[tokio::test]
async fn revoke_closes_the_socket_and_the_token() {
    let r = rig();
    let token = pair(&r, A).await;
    let mut ws = connect(&r, A).await;
    call(&mut ws, json!({"type":"auth","token":token})).await;
    let hash = r.bridge.devices()[0].hash.clone();
    r.bridge.remove(&hash);
    assert_eq!(recv(&mut ws).await, Err(4403));
    assert!(r.bridge.devices().is_empty());
    let mut again = connect(&r, A).await;
    assert_eq!(
        code_of(&call(&mut again, json!({"type":"auth","token":token})).await),
        "bad_token"
    );
}

#[tokio::test]
async fn connection_cap_and_stop() {
    let r = rig();
    let mut open = vec![];
    for _ in 0..8 {
        open.push(connect(&r, A).await);
    }
    assert!(
        try_connect(r.bridge.port(), Some(A), None).await.is_err(),
        "9th is dropped"
    );
    let mut first = open.remove(0);
    let port = r.bridge.port();
    std::thread::spawn(move || r.bridge.stop());
    assert_eq!(recv(&mut first).await, Err(1001));
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
        TcpStream::connect(("127.0.0.1", port)).await.is_err(),
        "listener is gone"
    );
}

#[tokio::test]
async fn revoke_holds_even_without_the_broadcast() {
    let r = rig();
    let token = pair(&r, A).await;
    let mut ws = connect(&r, A).await;
    call(&mut ws, json!({"type":"auth","token":token})).await;
    // drop the device without the Revoke message reaching the session
    lock(&r.bridge.inner.store).devices.clear();
    let v = call(
        &mut ws,
        json!({"type":"add","url":"https://youtube.com/watch?v=1","start":true}),
    )
    .await;
    assert_eq!(code_of(&v), "unauthorized");
    assert_eq!(recv(&mut ws).await, Err(4403));
    assert_eq!(r.host.adds.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn preset_is_capped() {
    let r = rig();
    let token = pair(&r, A).await;
    let mut ws = connect(&r, A).await;
    call(&mut ws, json!({"type":"auth","token":token})).await;
    let long = "x".repeat(65);
    let url = "https://youtube.com/watch?v=1";
    let v = call(
        &mut ws,
        json!({"type":"add","url":url,"start":true,"preset":long}),
    )
    .await;
    assert_eq!(code_of(&v), "bad_request");
    let ok = "y".repeat(64);
    assert_eq!(
        call(
            &mut ws,
            json!({"type":"add","url":url,"start":true,"preset":ok})
        )
        .await["type"],
        "added"
    );
}

#[test]
fn unreadable_store_is_set_aside_not_wiped() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("extension.json");
    std::fs::write(&f, b"{ not json").unwrap();
    let s = store::Store::load(dir.path());
    assert!(s.devices.is_empty());
    assert!(!f.exists(), "moved aside");
    assert_eq!(
        std::fs::read(dir.path().join("extension.json.bad")).unwrap(),
        b"{ not json"
    );
    s.save().unwrap();
    assert!(
        dir.path().join("extension.json.bad").exists(),
        "save leaves the .bad copy"
    );
    // a missing file is normal and creates no .bad
    let empty = tempfile::tempdir().unwrap();
    store::Store::load(empty.path());
    assert!(!empty.path().join("extension.json.bad").exists());
}
