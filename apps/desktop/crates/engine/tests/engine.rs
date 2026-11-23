//! Integration: real worker processes against the fixture server, kill-tree, in-process probe.
mod common;
use std::{
    io::{BufRead, BufReader},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use common::server::{self, Server};
use domain::{ErrorCode, Preset};
use engine::{
    Event, WorkerHandle,
    host::TreeChild,
    partials, runtime,
    spec::{Env, probe_opts, simple_job},
};

fn srv() -> Server {
    server::start(server::fixtures_dir(), 0)
}

/// The re-exec'd worker exe: the `fetch` example (`cargo test` builds examples).
fn worker_exe() -> PathBuf {
    let mut p = std::env::current_exe().unwrap();
    p.pop();
    p.pop();
    p.push("examples");
    p.push(if cfg!(windows) { "fetch.exe" } else { "fetch" });
    assert!(p.exists(), "build examples first: {}", p.display());
    p
}

fn ffmpeg() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("YTDLPD_TEST_FFMPEG") {
        return Some(p.into());
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| {
            d.join(if cfg!(windows) {
                "ffmpeg.exe"
            } else {
                "ffmpeg"
            })
        })
        .find(|p| p.is_file())
}

fn out_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ytdlpd-engine-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn collect(w: &mut WorkerHandle) -> Vec<Event> {
    let mut evs = vec![];
    while let Ok(e) = w.events.recv_timeout(Duration::from_secs(90)) {
        let term = e.is_terminal();
        evs.push(e);
        if term {
            break;
        }
    }
    assert!(w.wait().unwrap().success(), "worker exit; events: {evs:?}");
    evs
}

fn result_path(evs: &[Event]) -> PathBuf {
    match evs.last() {
        Some(Event::Result {
            ok: true,
            path: Some(p),
            ..
        }) => PathBuf::from(p),
        other => panic!("expected result, got {other:?}\nall: {evs:#?}"),
    }
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    // SAFETY: query-only handle, closed right away.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0 && code == 259; // STILL_ACTIVE
        CloseHandle(h);
        ok
    }
}
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only probes existence.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[test]
fn mp4_download_finishes_with_real_path() {
    let s = srv();
    let out = out_dir("mp4");
    let mut w = engine::spawn_worker(
        &worker_exe(),
        &simple_job(s.url("/sample.mp4"), Preset::DEFAULT, &out),
    )
    .unwrap();
    let evs = collect(&mut w);
    assert!(evs.contains(&Event::Ready));
    let p = result_path(&evs);
    assert_eq!(
        std::fs::read(&p).unwrap(),
        std::fs::read(server::fixtures_dir().join("sample.mp4")).unwrap()
    );
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Progress { done, total: Some(t), .. } if done == t)),
        "final progress"
    );
    assert!(evs.iter().any(|e| matches!(e, Event::Files { .. })));
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Path { path } if Path::new(path) == p))
    );
}

#[test]
fn hls_download_finishes() {
    let Some(ff) = ffmpeg() else {
        return eprintln!("skip: no ffmpeg (set YTDLPD_TEST_FFMPEG)");
    };
    let s = srv();
    let out = out_dir("hls");
    let mut job = simple_job(s.url("/hls/index.m3u8"), Preset::DEFAULT, &out);
    job.ffmpeg_location = Some(ff.to_string_lossy().into_owned());
    let mut w = engine::spawn_worker(&worker_exe(), &job).unwrap();
    let evs = collect(&mut w);
    let p = result_path(&evs);
    assert!(p.metadata().unwrap().len() > 10_000, "{}", p.display());
    assert!(
        evs.iter().any(|e| matches!(
            e,
            Event::Progress {
                frag: Some((_, 3)),
                ..
            }
        )),
        "fragment progress (frag n/3): {evs:#?}"
    );
}

#[test]
fn missing_url_is_classified() {
    let s = srv();
    let mut w = engine::spawn_worker(
        &worker_exe(),
        &simple_job(s.url("/nope.mp4"), Preset::DEFAULT, out_dir("404")),
    )
    .unwrap();
    let mut last = None;
    while let Ok(e) = w.events.recv_timeout(Duration::from_secs(60)) {
        if e.is_terminal() {
            last = Some(e);
            break;
        }
    }
    let _ = w.wait();
    assert!(
        matches!(
            last,
            Some(Event::Error {
                code: ErrorCode::Unavailable,
                retryable: false,
                ..
            })
        ),
        "{last:?}"
    );
}

#[test]
fn cancel_mid_download_kills_worker_and_cleans_partials() {
    let s = srv();
    let out = out_dir("cancel");
    let mut w = engine::spawn_worker(
        &worker_exe(),
        &simple_job(
            s.url("/big.mp4?size=41943040&slow=10"),
            Preset::DEFAULT,
            &out,
        ),
    )
    .unwrap();
    let pid = w.pid();
    let (mut files, mut progressed) = (vec![], false);
    let t = Instant::now();
    while !progressed && t.elapsed() < Duration::from_secs(60) {
        match w.events.recv_timeout(Duration::from_secs(1)) {
            Ok(Event::Files { paths }) => files.extend(paths),
            Ok(Event::Progress { done, .. }) if done > 100_000 => progressed = true,
            _ => {}
        }
    }
    assert!(progressed, "download never progressed");
    let k = Instant::now();
    w.kill();
    assert!(k.elapsed() < Duration::from_secs(2), "kill is immediate");
    assert!(!alive(pid), "worker must be dead");
    assert!(
        partials::cleanup(&files) >= 1,
        "the .part file existed and is removed"
    );
    assert_eq!(
        std::fs::read_dir(&out).unwrap().count(),
        0,
        "nothing left behind"
    );
}

#[test]
fn kill_tree_kills_grandchild() {
    let home = runtime::resolve_home(None).expect("runtime");
    let py = if cfg!(windows) {
        home.join("python.exe")
    } else {
        home.join("bin/python3")
    };
    let code = "import subprocess,sys,time\np=subprocess.Popen([sys.executable,'-c','import time;time.sleep(120)'])\nprint(p.pid,flush=True)\ntime.sleep(120)";
    let mut tc = TreeChild::spawn(
        Command::new(py)
            .args(["-I", "-c", code])
            .stdout(Stdio::piped()),
    )
    .unwrap();
    let mut line = String::new();
    BufReader::new(tc.child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let (child, grand) = (tc.child.id(), line.trim().parse::<u32>().unwrap());
    assert!(alive(child) && alive(grand));
    tc.kill();
    std::thread::sleep(Duration::from_millis(300));
    assert!(!alive(child), "child dead");
    assert!(!alive(grand), "grandchild dead");
}

fn popts() -> serde_json::Value {
    probe_opts(&Default::default(), &Env::default())
}

fn init() {
    runtime::init(runtime::InitOptions {
        home: None,
        site_updates: None,
        sink: runtime::log_sink,
    })
    .expect("init");
}

#[test]
fn runtime_init_is_idempotent_and_probe_works() {
    init();
    let first = runtime::get().unwrap() as *const _;
    init();
    assert_eq!(
        first,
        runtime::get().unwrap() as *const _,
        "second call returns the same handle"
    );
    let s = srv();
    let r = engine::probe(&s.url("/sample.mp4"), &popts(), Duration::from_secs(30)).expect("probe");
    assert_eq!(
        (r.id.as_deref(), r.extractor.as_deref()),
        (Some("sample"), Some("Generic"))
    );
    assert!(!r.formats.is_empty());
}

#[test]
fn probe_times_out_and_counts_abandoned() {
    init();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/stall.mp4", l.local_addr().unwrap());
    std::thread::spawn(move || {
        let _held: Vec<_> = l.incoming().flatten().collect(); // accept, never answer
        std::thread::sleep(Duration::from_secs(300));
    });
    let before = engine::probe::abandoned_probes();
    let e = engine::probe(&url, &popts(), Duration::from_millis(800)).unwrap_err();
    assert!(e.retryable && e.message.contains("timed out"), "{e:?}");
    assert_eq!(engine::probe::abandoned_probes(), before + 1);
}

#[test]
fn pause_then_resume_uses_range_and_completes() {
    let s = srv();
    let out = out_dir("resume");
    let size = 3_000_000u64;
    let job = simple_job(
        s.url(&format!("/big.mp4?size={size}&slow=5")),
        Preset::DEFAULT,
        &out,
    );
    let mut w = engine::spawn_worker(&worker_exe(), &job).unwrap();
    loop {
        match w
            .events
            .recv_timeout(Duration::from_secs(60))
            .expect("event")
        {
            Event::Progress { done, .. } if done > 400_000 => break,
            Event::Error { message, .. } => panic!("{message}"),
            _ => {}
        }
    }
    w.kill(); // pause = kill, partials stay
    let part = std::fs::read_dir(&out)
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().ends_with(".part"))
        .expect("a .part file survives the kill");
    let kept = part.metadata().unwrap().len();
    assert!(kept > 0 && kept < size);
    let mut w = engine::spawn_worker(&worker_exe(), &job).unwrap();
    let evs = collect(&mut w);
    assert_eq!(result_path(&evs).metadata().unwrap().len(), size);
    let log = s.log.lock().unwrap();
    assert!(
        log.iter()
            .any(|(p, r)| p == "/big.mp4" && r.is_some_and(|r| r > 0)),
        "resume must send a Range request: {log:?}"
    );
}

#[test]
fn merge_runs_the_merger() {
    let Some(ff) = ffmpeg() else {
        return eprintln!("skip: no ffmpeg (set YTDLPD_TEST_FFMPEG)");
    };
    let s = srv();
    let mut job = simple_job(
        s.url("/merge/master.m3u8"),
        Preset::DEFAULT,
        out_dir("merge"),
    );
    job.ffmpeg_location = Some(ff.to_string_lossy().into_owned());
    let mut w = engine::spawn_worker(&worker_exe(), &job).unwrap();
    let evs = collect(&mut w);
    assert!(
        evs.iter()
            .any(|e| matches!(e, Event::Phase { name } if name == "merging")),
        "merging phase: {evs:#?}"
    );
    let p = result_path(&evs);
    assert!(matches!(
        p.extension().and_then(|e| e.to_str()),
        Some("mp4" | "mkv")
    ));
    assert!(p.metadata().unwrap().len() > 5_000);
}

fn backend() -> engine::EngineBackend {
    engine::EngineBackend::new(engine::EngineConfig {
        worker_exe: worker_exe(),
        python_home: None,
        site_updates: None,
        env: Env::default(),
    })
}

#[test]
fn backend_trait_probe_start_and_kill() {
    use domain::{Backend, DownloadId, JobEvent};
    use futures::{StreamExt, executor::block_on};
    let s = srv();
    let b = backend();
    let r = block_on(b.probe(s.url("/sample.mp4"), Default::default())).expect("probe");
    assert_eq!(r.id.as_deref(), Some("sample"));

    let (_h, mut st) = block_on(b.start(
        DownloadId(1),
        simple_job(s.url("/sample.mp4"), Preset::DEFAULT, out_dir("be-ok")),
    ))
    .unwrap();
    let mut evs = vec![];
    while let Some(e) = block_on(st.next()) {
        evs.push(e);
    }
    assert!(matches!(evs.last(), Some(JobEvent::Finished { .. })));
    assert!(
        evs.iter()
            .any(|e| matches!(e, JobEvent::Path(p) if Path::new(p).is_file()))
    );

    let (h, mut st) = block_on(b.start(
        DownloadId(2),
        simple_job(
            s.url("/big.mp4?size=41943040&slow=10"),
            Preset::DEFAULT,
            out_dir("be-kill"),
        ),
    ))
    .unwrap();
    while let Some(e) = block_on(st.next()) {
        if matches!(e, JobEvent::Progress { done, .. } if done > 100_000) {
            break;
        }
    }
    block_on(b.kill(h));
    let rest: Vec<_> = block_on(st.collect());
    assert!(
        !rest
            .iter()
            .any(|e| matches!(e, JobEvent::Finished { .. } | JobEvent::Failed(_))),
        "a killed job ends silently: {rest:?}"
    );
}

/// 20 reps of 8 concurrent workers, all must finish. `cargo test -- --ignored stress`.
#[test]
#[ignore]
#[allow(clippy::needless_collect)] // all threads must start before the first join
fn stress_8_concurrent_x20() {
    let s = srv();
    let reps: usize = std::env::var("YTDLPD_STRESS_REPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    for rep in 0..reps {
        let hs: Vec<_> = (0..8)
            .map(|i| {
                let url = s.url("/sample.mp4");
                let out = out_dir(&format!("stress-{i}"));
                std::thread::spawn(move || {
                    let mut w = engine::spawn_worker(
                        &worker_exe(),
                        &simple_job(url, Preset::DEFAULT, &out),
                    )
                    .unwrap();
                    let evs = collect(&mut w);
                    result_path(&evs).is_file()
                })
            })
            .collect();
        for (i, h) in hs.into_iter().enumerate() {
            assert!(h.join().unwrap(), "rep {rep} worker {i}");
        }
    }
}
