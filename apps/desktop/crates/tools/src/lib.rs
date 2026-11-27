//! External tools for ytdlpd: detect, download (ffmpeg, ffprobe, deno) and update yt-dlp.
//! No gpui. Network code is async and must run on a tokio runtime.
mod fetch;
pub mod matrix;
pub mod ytdlp;

pub use fetch::ProgressFn;
pub use ytdlp::{Installed, PairUpdate, Wheel, check_ytdlp_update, install_ytdlp_pair};

use anyhow::{Context, Result};
use fetch::{Ctx, extract, finish_binary, get_text, rename_retry};
use matrix::{Kind, select_btbn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A tool ytdlpd manages or detects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tool {
    Ffmpeg,
    Ffprobe,
    /// JavaScript runtime for yt-dlp's challenge solving.
    Deno,
    /// Optional external downloader yt-dlp can hand downloads to; detected only, never installed.
    Aria2c,
}

impl Tool {
    /// Short lowercase id (directory name, binary stem).
    pub fn id(self) -> &'static str {
        match self {
            Tool::Ffmpeg => "ffmpeg",
            Tool::Ffprobe => "ffprobe",
            Tool::Deno => "deno",
            Tool::Aria2c => "aria2c",
        }
    }

    /// Binary file name on this OS.
    pub fn exe_name(self) -> String {
        exe(self.id())
    }

    fn version_arg(self) -> &'static str {
        if matches!(self, Tool::Deno | Tool::Aria2c) {
            "--version"
        } else {
            "-version"
        }
    }
}

fn exe(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

/// Where a usable binary came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Installed by us under `<data>/tools`.
    Managed,
    /// Found on PATH or in a well-known directory.
    Path,
    /// Path set by the user in settings.
    Custom,
}

/// A detected, version-probed binary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolStatus {
    pub tool: Tool,
    pub source: Source,
    pub path: PathBuf,
    pub version: String,
}

/// App directories (the caller resolves them through storage paths).
#[derive(Clone, Debug)]
pub struct Dirs {
    /// Persistent data dir; managed tools live in `<data>/tools`, python sites in `<data>/python-site`.
    pub data: PathBuf,
    /// Cache dir; downloads stream into `<cache>/tools`.
    pub cache: PathBuf,
}

impl Dirs {
    fn tools(&self) -> PathBuf {
        self.data.join("tools")
    }
    fn dl(&self) -> PathBuf {
        self.cache.join("tools")
    }
}

// ---- version probing ----

/// Run `path args`, return stdout when it exits 0. Never shows a console window on Windows.
async fn run_probe(path: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = tokio::process::Command::new(path);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    // first run under Defender can take a few seconds
    let out = tokio::time::timeout(Duration::from_secs(20), cmd.output())
        .await
        .ok()?
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Version token from `-version` / `--version` output; `None` when it is not that program.
fn parse_version(name: &str, out: &str) -> Option<String> {
    let first = out.lines().next()?.trim();
    let v = match name {
        "ffmpeg" | "ffprobe" => first
            .strip_prefix(name)?
            .trim_start()
            .strip_prefix("version ")?
            .split_whitespace()
            .next()?,
        "deno" => first.strip_prefix("deno ")?.split_whitespace().next()?,
        "aria2c" => first
            .strip_prefix("aria2 version ")?
            .split_whitespace()
            .next()?,
        "node" => first.strip_prefix('v').unwrap_or(first),
        _ => first,
    };
    (!v.is_empty()).then(|| v.to_string())
}

async fn probe(tool: Tool, path: &Path) -> Option<String> {
    parse_version(tool.id(), &run_probe(path, &[tool.version_arg()]).await?)
}

// ---- detection ----

fn on_path(name: &str) -> Vec<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .filter(|p| p.is_file())
        .collect()
}

fn well_known_dirs(tool: Tool) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        v.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    if cfg!(windows) {
        if let Some(l) = dirs::data_local_dir() {
            v.push(l.join("Microsoft").join("WinGet").join("Links"));
        }
        if let Some(s) = std::env::var_os("SCOOP") {
            v.push(PathBuf::from(s).join("shims"));
        }
        if let Some(h) = dirs::home_dir() {
            v.push(h.join("scoop").join("shims"));
        }
    }
    if tool == Tool::Deno
        && let Some(h) = dirs::home_dir()
    {
        v.push(h.join(".deno").join("bin"));
    }
    v
}

/// Managed installs of `tool`, newest first (by directory mtime).
fn managed_candidates(tool: Tool, dirs: &Dirs) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dirs.tools().join(tool.id())) else {
        return vec![];
    };
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            Some((
                e.metadata().ok()?.modified().ok()?,
                e.path().join(tool.exe_name()),
            ))
        })
        .filter(|(_, p)| p.is_file())
        .collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.0));
    v.into_iter().map(|(_, p)| p).collect()
}

/// yt-dlp looks for ffprobe only next to the ffmpeg it is given, but managed installs keep each tool
/// in its own dir: link the newest of each into the other's dir. Also repairs older installs.
pub fn pair_ffmpeg(dirs: &Dirs) {
    let newest = |t| managed_candidates(t, dirs).into_iter().next();
    let (Some(ff), Some(fp)) = (newest(Tool::Ffmpeg), newest(Tool::Ffprobe)) else {
        return;
    };
    for (src, other) in [(&fp, &ff), (&ff, &fp)] {
        let (Some(dir), Some(name)) = (other.parent(), src.file_name()) else {
            continue;
        };
        let dest = dir.join(name);
        let len = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
        if len(&dest) == len(src) {
            continue;
        }
        let _ = std::fs::remove_file(&dest);
        if let Err(error) =
            std::fs::hard_link(src, &dest).or_else(|_| std::fs::copy(src, &dest).map(drop))
        {
            log::warn!(
                "tools: cannot place {} next to {}: {error}",
                src.display(),
                other.display()
            );
        }
    }
}

/// Find a usable `tool`: settings path, PATH, well-known dirs, then managed install. Every
/// candidate must pass its version probe.
pub async fn detect(tool: Tool, dirs: &Dirs, settings_path: Option<&Path>) -> Option<ToolStatus> {
    let mut cands: Vec<(Source, PathBuf)> = Vec::new();
    if let Some(p) = settings_path {
        cands.push((Source::Custom, p.to_path_buf()));
    }
    cands.extend(
        on_path(&tool.exe_name())
            .into_iter()
            .map(|p| (Source::Path, p)),
    );
    cands.extend(
        well_known_dirs(tool)
            .into_iter()
            .map(|d| d.join(tool.exe_name()))
            .filter(|p| p.is_file())
            .map(|p| (Source::Path, p)),
    );
    cands.extend(
        managed_candidates(tool, dirs)
            .into_iter()
            .map(|p| (Source::Managed, p)),
    );
    for (source, path) in cands {
        if let Some(version) = probe(tool, &path).await {
            return Some(ToolStatus {
                tool,
                source,
                path,
                version,
            });
        }
        log::warn!(
            "tools: {} at {} failed its version probe",
            tool.id(),
            path.display()
        );
    }
    None
}

// ---- JS runtimes ----

/// A JavaScript runtime yt-dlp can use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsRuntime {
    /// yt-dlp key: deno, node, bun or quickjs.
    pub name: &'static str,
    pub path: PathBuf,
    pub version: String,
}

fn at_least(version: &str, (maj, min): (u32, u32)) -> bool {
    let mut p = version
        .trim_start_matches('v')
        .split('.')
        .map(|s| s.parse::<u32>().unwrap_or(0));
    (p.next().unwrap_or(0), p.next().unwrap_or(0)) >= (maj, min)
}

/// yt-dlp `js_runtimes` option value: `{"deno": {"path": ...}, ...}` in preference order.
pub fn js_runtimes_dict(rt: &[JsRuntime]) -> serde_json::Value {
    rt.iter()
        .map(|r| (r.name.to_string(), serde_json::json!({ "path": r.path })))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// Detect usable runtimes: deno >= 2.3 (settings, PATH, managed), node >= 22, bun, quickjs on PATH.
pub async fn js_runtimes(dirs: &Dirs, deno_settings_path: Option<&Path>) -> Vec<JsRuntime> {
    let mut out = Vec::new();
    if let Some(s) = detect(Tool::Deno, dirs, deno_settings_path).await
        && at_least(&s.version, (2, 3))
    {
        out.push(JsRuntime {
            name: "deno",
            path: s.path,
            version: s.version,
        });
    }
    let others = [
        ("node", "node", "--version", Some((22, 0))),
        ("bun", "bun", "--version", None),
        ("quickjs", "qjs", "--help", None),
        ("quickjs", "quickjs", "--help", None),
    ];
    for (name, bin, arg, min) in others {
        if out.iter().any(|r| r.name == name) {
            continue;
        }
        for path in on_path(&exe(bin)) {
            if let Some(v) = run_probe(&path, &[arg])
                .await
                .and_then(|o| parse_version(name, &o))
                && min.is_none_or(|m| at_least(&v, m))
            {
                out.push(JsRuntime {
                    name,
                    path,
                    version: v,
                });
                break;
            }
        }
    }
    out
}

// ---- ensure / install ----

/// Detect `tool`; when missing, download, verify and install it under `<data>/tools`.
/// `progress` reports download bytes. Fails closed on any checksum problem.
pub async fn ensure(
    tool: Tool,
    dirs: &Dirs,
    settings_path: Option<&Path>,
    progress: ProgressFn<'_>,
) -> Result<ToolStatus> {
    ensure_with(&Ctx::new()?, tool, dirs, settings_path, progress).await
}

/// Download and install `tool` even when one is already detected (Tools page "Re-download").
pub async fn reinstall(tool: Tool, dirs: &Dirs, progress: ProgressFn<'_>) -> Result<ToolStatus> {
    install(&Ctx::new()?, tool, dirs, progress).await
}

pub(crate) async fn ensure_with(
    ctx: &Ctx,
    tool: Tool,
    dirs: &Dirs,
    settings_path: Option<&Path>,
    progress: ProgressFn<'_>,
) -> Result<ToolStatus> {
    if let Some(s) = detect(tool, dirs, settings_path).await {
        return Ok(s);
    }
    install(ctx, tool, dirs, progress).await
}

/// Resolve asset url and expected sha256 for a plan.
async fn resolve(ctx: &Ctx, plan: &matrix::Plan) -> Result<(String, String)> {
    match &plan.kind {
        Kind::Btbn { base } => {
            let sums = get_text(ctx, &format!("{base}/checksums.sha256")).await?;
            let (name, sha) = select_btbn(&sums, ctx.target)?;
            Ok((format!("{base}/{name}"), sha))
        }
        Kind::Redirect { url } => {
            // The redirect carries the version; the checksum sits next to the resolved url.
            let resp = ctx
                .client
                .get(url)
                .send()
                .await
                .with_context(|| format!("cannot reach {url}"))?;
            let final_url = resp
                .error_for_status()
                .with_context(|| format!("cannot fetch {url}"))?
                .url()
                .to_string();
            let sha = matrix::parse_checksum(
                &get_text(ctx, &format!("{final_url}.sha256")).await?,
                None,
            )?;
            Ok((final_url, sha))
        }
        Kind::Direct { url, sha_url } => Ok((
            url.clone(),
            matrix::parse_checksum(&get_text(ctx, sha_url).await?, None)?,
        )),
    }
}

fn sweep_tmp(dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp-"))
        {
            let _ = std::fs::remove_dir_all(e.path()).or_else(|_| std::fs::remove_file(e.path()));
        }
    }
}

fn dir_name(version: &str) -> String {
    version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-+".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

async fn install(
    ctx: &Ctx,
    tool: Tool,
    dirs: &Dirs,
    progress: ProgressFn<'_>,
) -> Result<ToolStatus> {
    anyhow::ensure!(tool != Tool::Aria2c, "ytdlpd does not download aria2c");
    let plan = matrix::plan(tool, ctx.target, ctx.mirror.as_deref());
    let (tools_dir, dl_dir) = (dirs.tools(), dirs.dl());
    std::fs::create_dir_all(&tools_dir).context("cannot create tools dir")?;
    // shortcut: assumes one installer at a time; stale temp from a killed run is swept here
    sweep_tmp(&tools_dir);
    sweep_tmp(&dl_dir);
    let (url, sha) = resolve(ctx, &plan).await?;
    log::info!("tools: downloading {url}");
    let archive = fetch::download(ctx, &url, &sha, &dl_dir, progress).await?;
    let staging = tempfile::Builder::new()
        .prefix(".tmp-")
        .tempdir_in(&tools_dir)
        .context("cannot create staging dir")?;
    let (apath, out, wanted, kind) = (
        archive.path().to_path_buf(),
        staging.path().to_path_buf(),
        plan.tools.clone(),
        plan.archive,
    );
    tokio::task::spawn_blocking(move || extract(&apath, kind, &wanted, &out))
        .await
        .context("unpack task failed")??;
    drop(archive);

    let mut result = None;
    for t in &plan.tools {
        let staged = staging.path().join(t.id());
        finish_binary(&staged.join(t.exe_name()));
        let version = match &ctx.fake_version {
            Some(v) => v.clone(),
            None => probe(*t, &staged.join(t.exe_name()))
                .await
                .with_context(|| format!("installed {} does not run", t.id()))?,
        };
        let dest = tools_dir.join(t.id()).join(dir_name(&version));
        let (from, to) = (staged.clone(), dest.clone());
        tokio::task::spawn_blocking(move || -> Result<()> {
            std::fs::create_dir_all(to.parent().unwrap())?;
            fetch::remove_dir_retry(&to).context("cannot replace old install")?;
            rename_retry(&from, &to).context("cannot move tool into place")
        })
        .await
        .context("install task failed")??;
        if *t == tool {
            result = Some(ToolStatus {
                tool,
                source: Source::Managed,
                path: dest.join(t.exe_name()),
                version,
            });
        }
    }
    if matches!(tool, Tool::Ffmpeg | Tool::Ffprobe) {
        pair_ffmpeg(dirs);
    }
    result.context("tool missing from plan")
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod testutil;
