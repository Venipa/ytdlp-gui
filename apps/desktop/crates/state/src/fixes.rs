//! `Fixes`: after a download finishes, ffprobe reads the file; a site rule's (or the default) fix
//! runs ffmpeg on it, and a file that may not play everywhere gets a hint the detail panel shows
//! with a button that runs the Compatible fix. Plans are pure (`domain::fixes`); this runs them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use domain::fixes::{Plan, Streams, Unplayable, hw_encoder, parse_ffprobe, plan, unplayable};
use domain::{DownloadId, Fix, JobSpec, ToolKind};
use gpui::{App, Context, Entity};

use crate::{AppSettings, Downloads, Io, Store, Tools};

/// What the detail panel shows about a finished file.
#[derive(Debug, Clone, PartialEq)]
pub enum FixState {
    Running(Fix),
    Failed(Fix, String),
    /// The file may not play in the system players.
    Unplayable(Unplayable),
}

pub struct Fixes {
    io: Io,
    store: Store,
    tools: Entity<Tools>,
    settings: Entity<AppSettings>,
    downloads: Entity<Downloads>,
    states: HashMap<DownloadId, FixState>,
    /// Hints the user closed.
    dismissed: HashSet<DownloadId>,
}

/// ffmpeg's encoder list, read once per run of the app.
static ENCODERS: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn tool(path: &Option<String>, name: &str) -> PathBuf {
    path.clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(name))
}

async fn run(program: &Path, args: &[String]) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = cmd
        .output()
        .await
        .map_err(|e| format!("cannot run {}: {e}", program.display()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("ffmpeg failed")
            .to_owned())
    }
}

async fn probe(ffprobe: &Path, file: &Path) -> Result<Streams, String> {
    let args: Vec<String> = ["-v", "error", "-print_format", "json", "-show_streams"]
        .iter()
        .map(|s| s.to_string())
        .chain([file.to_string_lossy().into_owned()])
        .collect();
    parse_ffprobe(&run(ffprobe, &args).await?).ok_or_else(|| "ffprobe output not understood".into())
}

/// Runs `plan` on `file`, writing next to it; returns the new file (the original is replaced).
async fn apply(ffmpeg: &Path, file: &Path, plan: &Plan) -> Result<PathBuf, String> {
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = file.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!("{stem}.fixing.{}", plan.ext));
    let mut args: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.extend(plan.input.iter().cloned());
    args.extend(["-i".into(), file.to_string_lossy().into_owned()]);
    args.extend(plan.output.iter().cloned());
    args.push(tmp.to_string_lossy().into_owned());
    if let Err(error) = run(ffmpeg, &args).await {
        std::fs::remove_file(&tmp).ok();
        return Err(error);
    }
    let dest = dir.join(format!("{stem}.{}", plan.ext));
    // a different extension must not overwrite some other file of that name
    let dest = match dest != file && dest.exists() {
        true => dir.join(format!("{stem} (fixed).{}", plan.ext)),
        false => dest,
    };
    std::fs::rename(&tmp, &dest).map_err(|e| format!("cannot replace the file: {e}"))?;
    if dest != file {
        std::fs::remove_file(file).ok();
    }
    Ok(dest)
}

/// Runs `fix` on `file`: probes it, picks encoders (hardware unless `encoder` is `software`,
/// software again when the hardware run fails) and applies the plan. Returns the new file.
async fn fix_file(
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    file: PathBuf,
    fix: Fix,
    encoder: String,
) -> Result<PathBuf, String> {
    let streams = probe(&ffprobe, &file).await?;
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let macos = cfg!(target_os = "macos");
    let soft = plan(fix, &ext, &streams, "libx264", "libx265", macos);
    if encoder == "software" {
        return apply(&ffmpeg, &file, &soft).await;
    }
    if ENCODERS.get().is_none() {
        let list = run(&ffmpeg, &["-hide_banner".into(), "-encoders".into()])
            .await
            .unwrap_or_default();
        ENCODERS.set(list).ok();
    }
    let list = ENCODERS.get().map(String::as_str).unwrap_or("");
    let h264 = hw_encoder(list, "h264").unwrap_or_else(|| "libx264".into());
    let hevc = hw_encoder(list, "hevc").unwrap_or_else(|| "libx265".into());
    let hard = plan(fix, &ext, &streams, &h264, &hevc, macos);
    match apply(&ffmpeg, &file, &hard).await {
        Ok(done) => Ok(done),
        // a listed GPU encoder can still fail (no GPU, old driver): software is the fallback
        Err(_) if hard != soft && encoder == "auto" => apply(&ffmpeg, &file, &soft).await,
        Err(error) => Err(error),
    }
}

impl Fixes {
    pub fn new(
        io: Io,
        store: Store,
        tools: Entity<Tools>,
        settings: Entity<AppSettings>,
        downloads: Entity<Downloads>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&downloads, |this, _, event: &crate::DownloadsEvent, cx| {
            if let crate::DownloadsEvent::Completed(id) = event {
                this.finished(*id, cx);
            }
        })
        .detach();
        Self {
            io,
            store,
            tools,
            settings,
            downloads,
            states: HashMap::new(),
            dismissed: HashSet::new(),
        }
    }

    /// What the detail panel shows for `id`.
    pub fn state(&self, id: DownloadId) -> Option<&FixState> {
        self.states
            .get(&id)
            .filter(|_| !self.dismissed.contains(&id))
    }

    pub fn dismiss(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        self.dismissed.insert(id);
        cx.notify();
    }

    fn paths(&self, cx: &App) -> (PathBuf, PathBuf) {
        let tools = self.tools.read(cx);
        let path = |kind| tools.get(kind).and_then(|s| s.path.clone());
        (
            tool(&path(ToolKind::Ffmpeg), "ffmpeg"),
            tool(&path(ToolKind::Ffprobe), "ffprobe"),
        )
    }

    /// A download finished: run its fix (from its stored spec), then check the result.
    fn finished(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let read = self.store.read(move |db| async move {
            let detail = db.get_detail(id).await?;
            Ok(detail.and_then(|d| serde_json::from_str::<JobSpec>(&d.options_json).ok()))
        });
        cx.spawn(async move |this, cx| {
            let fix = read.await.ok().flatten().and_then(|spec| spec.fix);
            this.update(cx, |this, cx| match fix {
                Some(fix) => this.run(id, fix, cx),
                None => this.check(id, cx),
            })
            .ok();
        })
        .detach();
    }

    /// Reads the finished file of `id` with ffprobe and records a hint when it may not play.
    pub fn check(&mut self, id: DownloadId, cx: &mut Context<Self>) {
        let Some(file) = self
            .downloads
            .read(cx)
            .get(id)
            .and_then(|r| r.file_path.clone())
        else {
            return;
        };
        let (_, ffprobe) = self.paths(cx);
        let io = self.io.clone();
        cx.spawn(async move |this, cx| {
            let file = PathBuf::from(file);
            let ext = file
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();
            let result = io.spawn(async move { probe(&ffprobe, &file).await }).await;
            let Ok(Ok(streams)) = result else {
                return;
            };
            this.update(cx, |this, cx| {
                match unplayable(&ext, &streams) {
                    Some(why) => this.states.insert(id, FixState::Unplayable(why)),
                    None => this.states.remove(&id),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Runs `fix` on the finished file of `id` (a rule after download, or the hint's button).
    pub fn run(&mut self, id: DownloadId, fix: Fix, cx: &mut Context<Self>) {
        if matches!(self.states.get(&id), Some(FixState::Running(_))) {
            return;
        }
        let Some(file) = self
            .downloads
            .read(cx)
            .get(id)
            .and_then(|r| r.file_path.clone())
        else {
            return;
        };
        let (ffmpeg, ffprobe) = self.paths(cx);
        let encoder = self.settings.read(cx).get().encoder.clone();
        self.dismissed.remove(&id);
        self.states.insert(id, FixState::Running(fix));
        cx.notify();
        let io = self.io.clone();
        let downloads = self.downloads.clone();
        cx.spawn(async move |this, cx| {
            let job = io.spawn(fix_file(ffmpeg, ffprobe, PathBuf::from(file), fix, encoder));
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            this.update(cx, |this, cx| {
                match result {
                    Ok(done) => {
                        this.states.remove(&id);
                        let size = std::fs::metadata(&done).ok().map(|m| m.len() as i64);
                        let path = done.to_string_lossy().into_owned();
                        downloads.update(cx, |d, cx| d.replace_file(id, path, size, cx));
                        this.check(id, cx);
                    }
                    Err(error) => {
                        log::warn!("fixes: {fix:?} on download {id} failed: {error}");
                        this.states.insert(id, FixState::Failed(fix, error));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs every fix on a real 2 s AV1/Opus clip. Needs `YTDLPD_TEST_FFMPEG` and
    /// `YTDLPD_TEST_FFPROBE`; skipped without them (CI has no ffmpeg).
    #[test]
    fn every_fix_produces_a_playable_file() {
        let (Ok(ffmpeg), Ok(ffprobe)) = (
            std::env::var("YTDLPD_TEST_FFMPEG"),
            std::env::var("YTDLPD_TEST_FFPROBE"),
        ) else {
            eprintln!("skipped: set YTDLPD_TEST_FFMPEG and YTDLPD_TEST_FFPROBE");
            return;
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        for fix in Fix::ALL {
            let src = dir.path().join(format!("{fix:?}.mp4"));
            let make: Vec<String> = [
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=25:duration=2",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-c:v",
                "libsvtav1",
                "-c:a",
                "libopus",
                "-shortest",
            ]
            .iter()
            .map(|s| s.to_string())
            .chain([src.to_string_lossy().into_owned()])
            .collect();
            rt.block_on(run(Path::new(&ffmpeg), &make)).unwrap();
            let done = rt
                .block_on(fix_file(
                    ffmpeg.clone().into(),
                    ffprobe.clone().into(),
                    src.clone(),
                    fix,
                    "auto".into(),
                ))
                .unwrap_or_else(|e| panic!("{fix:?}: {e}"));
            let streams = rt.block_on(probe(Path::new(&ffprobe), &done)).unwrap();
            assert!(
                streams.video.is_some() && streams.audio.is_some(),
                "{fix:?}: {streams:?}"
            );
            if fix == Fix::Compatible {
                let ext = done.extension().unwrap().to_string_lossy().into_owned();
                assert_eq!(unplayable(&ext, &streams), None, "{streams:?}");
                assert!(!src.exists() || src == done);
            }
            eprintln!("{fix:?}: {} {streams:?}", done.display());
        }
    }
}
