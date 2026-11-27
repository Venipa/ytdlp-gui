//! Opt-in live smoke over the real internet through `EngineBackend`.
//! `cargo run -p engine --example live -- --deno PATH --ffmpeg PATH [--aria2c PATH] [--kill-after SECS] [--out DIR] [--url URL] [--playlist URL]`
//! (get deno/ffmpeg with `cargo run -p tools --example ensure -- --data-dir DIR`). Downloads 720p and mp3, probes a playlist flat.
use std::{path::PathBuf, time::Instant};

use domain::{AudioFormat, Backend, DownloadId, JobEvent, Preset, ProbeOpts, VideoQuality};
use engine::{EngineBackend, EngineConfig, spec::Env};
use futures::{StreamExt, executor::block_on};

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
    let url = opt("--url").unwrap_or_else(|| "https://www.youtube.com/watch?v=jNQXAC9IVRw".into());
    let playlist = opt("--playlist").unwrap_or_else(|| {
        "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI".into()
    });
    let out = opt("--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("ytdlpd-live-out"));
    std::fs::create_dir_all(&out).unwrap();
    let deno = opt("--deno").expect("--deno PATH");
    let backend = EngineBackend::new(EngineConfig {
        worker_exe: std::env::current_exe().unwrap(),
        python_home: None,
        site_updates: None,
        env: Env {
            cache_dir: None,
            js_runtime: Some(("deno".into(), deno)),
            temp_dir: None,
        },
    });

    let t = Instant::now();
    let r = block_on(backend.probe(
        playlist,
        ProbeOpts {
            flat_playlist: true,
            ..Default::default()
        },
    ));
    match r {
        Ok(p) => println!(
            "playlist probe {} ms: title={:?} entries={}",
            t.elapsed().as_millis(),
            p.title,
            p.entries.as_ref().map_or(0, Vec::len)
        ),
        Err(e) => println!(
            "playlist probe FAILED {} ms: {e:?}",
            t.elapsed().as_millis()
        ),
    }

    for (name, preset) in [
        ("720p", Preset::Video(VideoQuality::P720)),
        ("mp3", Preset::Audio(AudioFormat::Mp3)),
    ] {
        let mut spec = engine::spec::simple_job(url.clone(), preset, &out);
        spec.ffmpeg_location = opt("--ffmpeg");
        spec.aria2c = opt("--aria2c");
        let t = Instant::now();
        let (h, mut ev) = block_on(backend.start(DownloadId(0), spec)).expect("start");
        let kill_after = opt("--kill-after").and_then(|s| s.parse::<u64>().ok());
        let mut files = vec![];
        let (mut path, mut warns, mut last) = (None, vec![], String::new());
        let (mut ticks, mut peak) = (0u32, 0u64);
        while let Some(e) = block_on(ev.next()) {
            if kill_after.is_some_and(|k| t.elapsed().as_secs() >= k) {
                block_on(backend.kill(h));
                println!(
                    "{name}: killed after {} ms; files named: {files:#?}",
                    t.elapsed().as_millis()
                );
                break;
            }
            match e {
                JobEvent::Path(p) => path = Some(p),
                JobEvent::Log { level, message } if level != "debug" => warns.push(message),
                JobEvent::Phase(p) => last = p,
                JobEvent::Progress {
                    done, total, speed, ..
                } => {
                    ticks += 1;
                    peak = peak.max(done);
                    println!("  progress {done} / {total:?} at {speed:?} B/s");
                }
                JobEvent::Files(f) => files.extend(f),
                JobEvent::Finished { .. } => {
                    println!(
                        "{name}: finished {} ms, {ticks} progress events up to {peak} bytes, last phase {last}, path {path:?}, size {:?}",
                        t.elapsed().as_millis(),
                        path.as_ref()
                            .and_then(|p| std::fs::metadata(p).ok())
                            .map(|m| m.len())
                    );
                    break;
                }
                JobEvent::Failed(e) => {
                    println!("{name}: FAILED {} ms: {e:?}", t.elapsed().as_millis());
                    break;
                }
                _ => {}
            }
        }
        for w in warns {
            println!("  warning: {w}");
        }
    }
}
