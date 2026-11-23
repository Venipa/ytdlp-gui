//! `cargo run --release -p storage --example seed -- 10000 [db-path]`: fills a DB and times `list_summaries`.
use domain::{DownloadStatus, JobSpec, Preset, Settings};
use std::time::Instant;
use storage::{Db, NewDownload};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let n: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(10_000);
    let path = args
        .next()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("ytdlpd-seed.db"));
    let _ = std::fs::remove_file(&path);
    let db = Db::open(&path, "seed").await?;
    let s = Settings::default();
    let t = Instant::now();
    for i in 0..n {
        let spec = JobSpec {
            url: format!("https://site{}.example.com/watch?v={i}", i % 20),
            preset: Preset::DEFAULT,
            format_spec: "bv*+ba/b".into(),
            output_dir: s.output_dir.clone(),
            outtmpl: s.outtmpl.clone(),
            playlist_index: None,
            embed_thumbnail: false,
            embed_metadata: true,
            embed_chapters: true,
            subtitles: Default::default(),
            sponsorblock: Default::default(),
            cookies: Default::default(),
            proxy: None,
            rate_limit: None,
            retries: None,
            ffmpeg_location: None,
            js_runtime: None,
            extra_args: vec![],
            fix: None,
        };
        let mut d = NewDownload::from_spec(&spec)?;
        d.title = Some(format!("Seeded video number {i}"));
        d.uploader = Some(format!("Channel {}", i % 50));
        let id = db.insert_download(&d).await?;
        let st = DownloadStatus::ALL[i % DownloadStatus::ALL.len()];
        db.set_status(id, st, None).await?;
    }
    println!("inserted {n} in {:?}", t.elapsed());
    let t = Instant::now();
    let rows = db.list_summaries().await?;
    println!("list_summaries: {} rows in {:?}", rows.len(), t.elapsed());
    println!("db: {}", path.display());
    db.close().await;
    Ok(())
}
