//! G3 proofs: every download option changes real yt-dlp behaviour (fixture server, recording proxy, real workers).
mod common;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use common::server::{self, Server};
use domain::{CookiesMode, CookiesSettings, ErrorCode, JobSpec, Preset};
use engine::{
    Event, WorkerConfig, WorkerHandle, runtime,
    spec::{Env, simple_job, split_extra_args},
};

fn srv() -> Server {
    server::start(server::fixtures_dir(), 0)
}

fn worker_exe() -> PathBuf {
    let mut p = std::env::current_exe().unwrap();
    p.pop();
    p.pop();
    p.push("examples");
    p.push(if cfg!(windows) { "fetch.exe" } else { "fetch" });
    assert!(p.exists(), "build examples first: {}", p.display());
    p
}

fn out_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ytdlpd-options-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(spec: &JobSpec, env: &Env) -> Vec<Event> {
    let mut w: WorkerHandle =
        engine::host::spawn_worker_with(&worker_exe(), spec, env, &WorkerConfig::default())
            .unwrap();
    let mut evs = vec![];
    while let Ok(e) = w.events.recv_timeout(Duration::from_secs(90)) {
        let term = e.is_terminal();
        evs.push(e);
        if term {
            break;
        }
    }
    let _ = w.wait();
    evs
}

fn ok_path(evs: &[Event]) -> PathBuf {
    match evs.last() {
        Some(Event::Result {
            ok: true,
            path: Some(p),
            ..
        }) => PathBuf::from(p),
        o => panic!("expected result, got {o:?}\n{evs:#?}"),
    }
}

#[test]
fn ratelimit_takes_at_least_3s_for_4mb_at_1mb_s() {
    let s = srv();
    let mut j = simple_job(
        s.url("/big.mp4?size=4194304"),
        Preset::DEFAULT,
        out_dir("rate"),
    );
    j.rate_limit = Some("1M".into());
    let t = Instant::now();
    ok_path(&run(&j, &Env::default()));
    assert!(t.elapsed() >= Duration::from_secs(3), "{:?}", t.elapsed());
}

#[test]
fn retries_resume_after_dropped_connections() {
    let s = srv();
    let url = s.url("/flaky.mp4?size=2097152&fail=2&id=a");
    let mut j = simple_job(url, Preset::DEFAULT, out_dir("retry"));
    j.retries = Some(5);
    let p = ok_path(&run(&j, &Env::default()));
    assert_eq!(p.metadata().unwrap().len(), 2_097_152);
    let gets = s
        .log
        .lock()
        .unwrap()
        .iter()
        .filter(|(p, _)| p == "/flaky.mp4")
        .count();
    assert!(gets >= 3, "server saw {gets} requests");
    // without retries the same server behaviour is a failure
    let mut j = simple_job(
        s.url("/flaky.mp4?size=2097152&fail=2&id=b"),
        Preset::DEFAULT,
        out_dir("noretry"),
    );
    j.retries = Some(0);
    assert!(matches!(
        run(&j, &Env::default()).last(),
        Some(Event::Error { .. })
    ));
}

/// Plain-HTTP recording proxy: logs each request line, forwards to the origin, relays the answer.
fn proxy() -> (String, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let log = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming().flatten() {
            let log = log.clone();
            std::thread::spawn(move || {
                let _ = forward(c, &log);
            });
        }
    });
    (url, seen)
}

fn forward(mut c: TcpStream, log: &Mutex<Vec<String>>) -> std::io::Result<()> {
    let mut head = vec![];
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if c.read(&mut b)? == 0 {
            return Ok(());
        }
        head.push(b[0]);
    }
    let text = String::from_utf8_lossy(&head).to_string();
    let line = text.lines().next().unwrap_or("").to_string();
    log.lock().unwrap().push(line.clone());
    // `GET http://host:port/path HTTP/1.1`: connect to host:port and send the origin-form request
    let url = line.split_whitespace().nth(1).unwrap_or("");
    let rest = url.strip_prefix("http://").unwrap_or(url);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let mut o = TcpStream::connect(host)?;
    let fixed = text.replacen(url, &format!("/{path}"), 1);
    o.write_all(fixed.as_bytes())?;
    let mut o2 = o.try_clone()?;
    let mut c2 = c.try_clone()?;
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut c2, &mut o2);
    });
    std::io::copy(&mut o, &mut c)?;
    Ok(())
}

#[test]
fn proxy_sees_the_request() {
    let s = srv();
    let (p, seen) = proxy();
    let mut j = simple_job(s.url("/sample.mp4"), Preset::DEFAULT, out_dir("proxy"));
    j.proxy = Some(p);
    ok_path(&run(&j, &Env::default()));
    let seen = seen.lock().unwrap();
    assert!(seen.iter().any(|l| l.contains("/sample.mp4")), "{seen:?}");
}

#[test]
fn cookies_file_reaches_the_server() {
    let s = srv();
    let data = out_dir("cookie-data");
    let text = "# Netscape HTTP Cookie File\n127.0.0.1\tFALSE\t/\tFALSE\t0\tsid\tabc123\n";
    std::fs::create_dir_all(&data).unwrap();
    let f = data.join("cookies.txt");
    std::fs::write(&f, text).unwrap();
    let mut j = simple_job(s.url("/sample.mp4"), Preset::DEFAULT, out_dir("cookies"));
    j.cookies = CookiesSettings {
        mode: CookiesMode::File,
        file: Some(f.to_string_lossy().into()),
        browser: None,
        ..Default::default()
    };
    ok_path(&run(&j, &Env::default()));
    let c = s.cookies.lock().unwrap();
    assert!(
        c.iter()
            .any(|v| v.starts_with("Cookie:") && v.contains("sid=abc123")),
        "{c:?}"
    );
}

#[test]
fn extra_args_are_applied_with_quoting() {
    let s = srv();
    let mut j = simple_job(
        s.url("/big.mp4?size=4194304"),
        Preset::DEFAULT,
        out_dir("extra"),
    );
    j.extra_args = split_extra_args(r#"--limit-rate 1M --user-agent "My Agent/1.0""#).unwrap();
    let t = Instant::now();
    ok_path(&run(&j, &Env::default()));
    assert!(
        t.elapsed() >= Duration::from_secs(3),
        "limit-rate not applied"
    );
    let c = s.cookies.lock().unwrap();
    assert!(c.iter().any(|v| v == "User-Agent: My Agent/1.0"), "{c:?}");
    // a bad flag is a failure, not a hang
    let mut j = simple_job(s.url("/sample.mp4"), Preset::DEFAULT, out_dir("extra-bad"));
    j.extra_args = vec!["--no-such-flag".into()];
    assert!(matches!(
        run(&j, &Env::default()).last(),
        Some(Event::Error { .. })
    ));
}

#[test]
fn http_429_carries_retry_after() {
    let s = srv();
    let j = simple_job(s.url("/limited.mp4"), Preset::DEFAULT, out_dir("429"));
    match run(&j, &Env::default()).last() {
        Some(Event::Error {
            code: ErrorCode::RateLimited,
            retry_after: Some(7),
            ..
        }) => {}
        o => panic!("{o:?}"),
    }
}

#[test]
fn outtmpl_preview_equals_real_filename() {
    runtime::init(runtime::InitOptions {
        home: None,
        site_updates: None,
        sink: runtime::log_sink,
    })
    .unwrap();
    let s = srv();
    for tmpl in [
        "",
        "%(id)s - %(title)s.%(ext)s",
        "clips/%(extractor)s/%(id)s.%(ext)s",
    ] {
        let mut j = simple_job(s.url("/sample.mp4"), Preset::DEFAULT, out_dir("preview"));
        j.outtmpl = tmpl.into();
        let evs = run(&j, &Env::default());
        let real = ok_path(&evs);
        let Some(Event::Result { info, .. }) = evs.last() else {
            unreachable!()
        };
        let mut info = info.clone();
        info["extractor"] = "generic".into(); // the Result summary carries extractor_key only
        let got = engine::preview_filename(&j, &Env::default(), &info).unwrap();
        assert_eq!(Path::new(&got), real, "template {tmpl:?}");
    }
}
