//! `cargo run --example ensure -- [--data-dir DIR]`: ensure ffmpeg, ffprobe and deno, print status and timings.
use std::time::Instant;
use tools::{Dirs, Tool};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut args = std::env::args().skip(1);
    let mut root = std::path::PathBuf::from(r"F:\tmp\ytdlpd-rs\tmp\d");
    while let Some(a) = args.next() {
        if a == "--data-dir" {
            root = args.next().expect("--data-dir needs a value").into();
        }
    }
    let dirs = Dirs {
        data: root.join("data"),
        cache: root.join("cache"),
    };
    for tool in [Tool::Ffmpeg, Tool::Ffprobe, Tool::Deno] {
        let t = Instant::now();
        let last = std::sync::atomic::AtomicU64::new(0);
        let s = tools::ensure(tool, &dirs, None, &|done, total| {
            let mb = done >> 20;
            if mb >= last.load(std::sync::atomic::Ordering::Relaxed) + 25 {
                last.store(mb, std::sync::atomic::Ordering::Relaxed);
                eprintln!("  {} {mb} / {} MB", tool.id(), total.unwrap_or(0) >> 20);
            }
        })
        .await?;
        println!(
            "{:8} {:?} {} ({}) in {:.1}s",
            tool.id(),
            s.source,
            s.path.display(),
            s.version,
            t.elapsed().as_secs_f32()
        );
    }
    println!(
        "js runtimes: {}",
        tools::js_runtimes_dict(&tools::js_runtimes(&dirs, None).await)
    );
    let t = Instant::now();
    match tools::check_ytdlp_update(&dirs, Some("2020.01.01")).await? {
        Some(u) => println!(
            "yt-dlp {} + yt-dlp-ejs {} in {:.1}s",
            u.ytdlp.version,
            u.ejs.version,
            t.elapsed().as_secs_f32()
        ),
        None => println!("yt-dlp up to date"),
    }
    Ok(())
}
