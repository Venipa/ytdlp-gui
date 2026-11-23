//! G1 numbers: `cargo run --release -p engine --example measure -- <url-of-slow-big-file>`
//! (start `fixture_server`, url like `http://127.0.0.1:8765/big.mp4?size=41943040&slow=10`).
//! Prints Python init time, probe time, worker time-to-ready and RSS (working set / private bytes) for 1 and 3 active workers.
use std::time::{Duration, Instant};

use domain::Preset;
use engine::{
    Event, WorkerHandle, runtime,
    spec::{Env, probe_opts},
};

#[cfg(windows)]
fn rss_mb(pid: u32) -> (f64, f64) {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };
    // SAFETY: query-only handle and a zeroed out-struct of the right size.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        let mut c: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        GetProcessMemoryInfo(
            h,
            (&raw mut c).cast(),
            size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        );
        CloseHandle(h);
        (
            c.WorkingSetSize as f64 / 1048576.0,
            c.PrivateUsage as f64 / 1048576.0,
        )
    }
}
#[cfg(not(windows))]
fn rss_mb(pid: u32) -> (f64, f64) {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    let kb = s
        .lines()
        .find_map(|l| {
            l.strip_prefix("VmRSS:")?
                .trim()
                .trim_end_matches(" kB")
                .parse::<f64>()
                .ok()
        })
        .unwrap_or(0.0);
    (kb / 1024.0, kb / 1024.0)
}

fn wait_downloading(w: &WorkerHandle) -> Duration {
    let mut ready = None;
    loop {
        match w
            .events
            .recv_timeout(Duration::from_secs(60))
            .expect("event")
        {
            Event::Ready => ready = Some(w.spawned_at.elapsed()),
            Event::Phase { name } if name == "downloading" => {
                return ready.expect("ready before downloading");
            }
            Event::Error { message, .. } => panic!("{message}"),
            _ => {}
        }
    }
}

fn spawn(url: &str, out: &str) -> WorkerHandle {
    engine::spawn_worker(
        &std::env::current_exe().unwrap(),
        &engine::spec::simple_job(url, Preset::DEFAULT, out),
    )
    .unwrap()
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.first().map(String::as_str) == Some("--engine-worker") {
        engine::worker::run_worker();
    }
    let url = a.first().expect("url");
    let out = std::env::temp_dir()
        .join("ytdlpd-measure")
        .to_string_lossy()
        .into_owned();

    let t = Instant::now();
    let rt = runtime::init(runtime::InitOptions {
        home: None,
        site_updates: None,
        sink: runtime::log_sink,
    })
    .expect("init");
    println!(
        "in-process: python init {} ms (call total {} ms)",
        rt.init_time.as_millis(),
        t.elapsed().as_millis()
    );
    let (ws, pv) = rss_mb(std::process::id());
    println!("host process after init: working set {ws:.0} MB, private {pv:.0} MB");
    let t = Instant::now();
    let r = engine::probe(
        url,
        &probe_opts(&Default::default(), &Env::default()),
        Duration::from_secs(30),
    )
    .expect("probe");
    println!(
        "first probe {} ms (id {:?}), includes import yt_dlp",
        t.elapsed().as_millis(),
        r.id
    );
    let t = Instant::now();
    engine::probe(
        url,
        &probe_opts(&Default::default(), &Env::default()),
        Duration::from_secs(30),
    )
    .expect("probe");
    println!("second probe {} ms", t.elapsed().as_millis());
    let (ws, pv) = rss_mb(std::process::id());
    println!("host process after probes: working set {ws:.0} MB, private {pv:.0} MB");

    for n in [1usize, 3] {
        let mut ws: Vec<WorkerHandle> = (0..n)
            .map(|i| spawn(url, &format!("{out}{n}-{i}")))
            .collect();
        let ready: Vec<_> = ws.iter().map(wait_downloading).collect();
        std::thread::sleep(Duration::from_millis(1500));
        let mem: Vec<_> = ws.iter().map(|w| rss_mb(w.pid())).collect();
        println!(
            "{n} worker(s): time-to-ready {:?} ms; working set {:?} MB; private {:?} MB; total working set {:.0} MB",
            ready.iter().map(|d| d.as_millis()).collect::<Vec<_>>(),
            mem.iter().map(|m| m.0.round()).collect::<Vec<_>>(),
            mem.iter().map(|m| m.1.round()).collect::<Vec<_>>(),
            mem.iter().map(|m| m.0).sum::<f64>()
        );
        ws.iter_mut().for_each(|w| {
            w.kill();
        });
    }
}
