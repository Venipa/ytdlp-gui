//! `cargo run -p engine --example fetch -- <url> [--out DIR] [--preset best|audio-mp3|<height>] [--cancel-after MS] [--ffmpeg PATH]`
//! Re-executes itself with `--engine-worker` as the worker child (the real app does the same with `ytdlpd`).
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use domain::{AudioFormat, Preset};
use engine::{Event, WorkerConfig, partials, spec::Env};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--engine-worker") {
        engine::worker::run_worker();
    }
    let opt = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let Some(url) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!(
            "usage: fetch <url> [--out DIR] [--preset best|audio-mp3|<height>] [--cancel-after MS] [--ffmpeg PATH]"
        );
        std::process::exit(2);
    };
    let preset = match opt("--preset").as_deref() {
        None | Some("best") => Preset::DEFAULT,
        Some("audio-mp3") => Preset::Audio(AudioFormat::Mp3),
        Some(h) => Preset::Video(
            h.parse()
                .expect("quality: best, 2160, 1440, 1080, 720, 480"),
        ),
    };
    let out = opt("--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("ytdlpd-fetch-out"));
    std::fs::create_dir_all(&out).unwrap();
    let mut job = engine::spec::simple_job(url.clone(), preset, &out);
    job.ffmpeg_location = opt("--ffmpeg");
    let cancel_after = opt("--cancel-after")
        .and_then(|m| m.parse::<u64>().ok())
        .map(Duration::from_millis);

    let exe = std::env::current_exe().unwrap();
    let t0 = Instant::now();
    let mut w =
        engine::host::spawn_worker_with(&exe, &job, &Env::default(), &WorkerConfig::default())
            .expect("spawn worker");
    let pid = w.pid();
    let mut files: Vec<String> = vec![];
    loop {
        let wait =
            cancel_after.map_or(Duration::from_secs(120), |c| c.saturating_sub(t0.elapsed()));
        match w.events.recv_timeout(wait) {
            Ok(ev) => {
                println!(
                    "{:>6} ms  {}",
                    t0.elapsed().as_millis(),
                    serde_json::to_string(&ev).unwrap()
                );
                if let Event::Files { paths } = &ev {
                    files.extend(paths.iter().cloned());
                }
                if ev.is_terminal() {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let st = w.kill();
                println!(
                    "{:>6} ms  KILLED worker pid {pid} (exit {st}); removed {} partial files",
                    t0.elapsed().as_millis(),
                    partials::cleanup(&files)
                );
                return;
            }
            Err(_) => break,
        }
    }
    println!("worker exit: {:?}", w.wait().map(|s| s.code()));
}
