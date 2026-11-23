//! HTTP + archive plumbing: streaming download with sha256, safe unpack, atomic rename.
use crate::{
    Tool,
    matrix::{Archive, Target},
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;

/// Progress callback: (bytes received, total if known).
pub type ProgressFn<'a> = &'a (dyn Fn(u64, Option<u64>) + Send + Sync);

/// Hard cap for one download.
pub(crate) const DOWNLOAD_CAP: u64 = 1 << 30;
/// Hard cap for one extracted file.
pub(crate) const EXTRACT_CAP: u64 = 700 << 20;

/// Shared client and test seams.
pub(crate) struct Ctx {
    pub client: reqwest::Client,
    pub mirror: Option<String>,
    pub target: Target,
    /// Tests only: skip running installed binaries (they are fake) and use this version.
    pub fake_version: Option<String>,
}

impl Ctx {
    /// Real context: reads `YTDLPD_TOOLS_MIRROR`.
    pub fn new() -> Result<Ctx> {
        // reqwest has no built-in crypto provider; a second install only means someone else did it.
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let client = reqwest::Client::builder()
            .user_agent(concat!("ytdlpd-tools/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(20))
            .read_timeout(std::time::Duration::from_secs(60))
            .build()
            .context("cannot build http client")?;
        let mirror = std::env::var("YTDLPD_TOOLS_MIRROR")
            .ok()
            .filter(|m| !m.is_empty());
        Ok(Ctx {
            client,
            mirror,
            target: Target::current(),
            fake_version: None,
        })
    }
}

/// GET a small text body; non-2xx is an error.
pub(crate) async fn get_text(ctx: &Ctx, url: &str) -> Result<String> {
    let r = ctx
        .client
        .get(url)
        .send()
        .await
        .with_context(|| format!("cannot reach {url}"))?;
    let r = r
        .error_for_status()
        .with_context(|| format!("cannot fetch {url}"))?;
    r.text().await.with_context(|| format!("cannot read {url}"))
}

/// Lowercase hex of a digest.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Stream `url` into a temp file in `dir`, hashing on the way. A checksum mismatch, a short body or
/// any error drops the temp file, so nothing is left behind.
pub(crate) async fn download(
    ctx: &Ctx,
    url: &str,
    expected_sha: &str,
    dir: &Path,
    progress: ProgressFn<'_>,
) -> Result<NamedTempFile> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let tmp = tempfile::Builder::new()
        .prefix(".tmp-dl-")
        .tempfile_in(dir)
        .context("cannot create download file")?;
    let mut file = tokio::fs::File::from_std(tmp.reopen().context("cannot open download file")?);
    let mut resp = ctx
        .client
        .get(url)
        .send()
        .await
        .with_context(|| format!("cannot reach {url}"))?;
    resp = resp
        .error_for_status()
        .with_context(|| format!("cannot download {url}"))?;
    let total = resp.content_length();
    ensure!(
        total.is_none_or(|t| t <= DOWNLOAD_CAP),
        "download too large: {url}"
    );
    let (mut hasher, mut done) = (Sha256::new(), 0u64);
    while let Some(chunk) = resp
        .chunk()
        .await
        .with_context(|| format!("cannot read {url}"))?
    {
        hasher.update(&chunk);
        done += chunk.len() as u64;
        ensure!(done <= DOWNLOAD_CAP, "download too large: {url}");
        file.write_all(&chunk)
            .await
            .context("cannot write download")?;
        progress(done, total);
    }
    file.flush().await.context("cannot write download")?;
    ensure!(
        total.is_none_or(|t| t == done),
        "short download of {url}: {done} of {} bytes",
        total.unwrap_or(0)
    );
    let got = hex(&hasher.finalize());
    ensure!(
        got == expected_sha.to_ascii_lowercase(),
        "checksum mismatch for {url}: expected {expected_sha}, got {got}"
    );
    Ok(tmp)
}

/// Rename with retry for Windows Defender / file locks (5 x 200 ms).
pub(crate) fn rename_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut last = None;
    for i in 0..5 {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
        if i < 4 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    Err(last.unwrap())
}

/// `remove_dir_all` with the same retry.
pub(crate) fn remove_dir_retry(p: &Path) -> std::io::Result<()> {
    let mut last = None;
    for i in 0..5 {
        match std::fs::remove_dir_all(p) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => last = Some(e),
        }
        if i < 4 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    Err(last.unwrap())
}

/// Entry names must be relative and free of `..` (zip-slip); backslashes count as separators.
fn safe_name(name: &str) -> bool {
    let n = name.replace('\\', "/");
    !n.is_empty()
        && !n.starts_with('/')
        && !n.contains(':')
        && Path::new(&n)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Which wanted tool does this archive entry hold? Only `<tool>` at the root or in a `bin/` dir.
fn wanted_tool(name: &str, wanted: &[Tool]) -> Option<Tool> {
    let n = name.replace('\\', "/");
    let mut parts = n.rsplit('/');
    let file = parts.next()?;
    let parent = parts.next();
    if !(parent.is_none() || parent == Some("bin")) {
        return None;
    }
    let stem = file.strip_suffix(".exe").unwrap_or(file);
    wanted.iter().copied().find(|t| t.id() == stem)
}

fn copy_capped(mut src: impl Read, dest: &Path, cap: u64) -> Result<()> {
    let mut out =
        std::fs::File::create(dest).with_context(|| format!("cannot create {}", dest.display()))?;
    let n =
        std::io::copy(&mut (&mut src).take(cap + 1), &mut out).context("cannot extract file")?;
    ensure!(n <= cap, "archive entry too large");
    Ok(())
}

/// Extract the `wanted` binaries to `out/<tool id>/<exe name>`. Every entry name is checked
/// (zip-slip is an error even for entries we do not extract); links are never followed or
/// extracted, and a link in place of a wanted binary is an error.
pub(crate) fn extract(archive: &Path, kind: Archive, wanted: &[Tool], out: &Path) -> Result<()> {
    let mut found: Vec<Tool> = Vec::new();
    let mut put = |tool: Tool, src: &mut dyn Read| -> Result<()> {
        let dir = out.join(tool.id());
        std::fs::create_dir_all(&dir)?;
        copy_capped(src, &dir.join(tool.exe_name()), EXTRACT_CAP)?;
        found.push(tool);
        Ok(())
    };
    let file = std::fs::File::open(archive)
        .with_context(|| format!("cannot open {}", archive.display()))?;
    match kind {
        Archive::Zip => {
            let mut z = zip::ZipArchive::new(file).context("cannot read zip")?;
            for i in 0..z.len() {
                let mut e = z.by_index(i).context("cannot read zip entry")?;
                ensure!(safe_name(e.name()), "unsafe path in archive: {}", e.name());
                let Some(t) = wanted_tool(e.name(), wanted) else {
                    continue;
                };
                ensure!(
                    e.is_file() && e.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
                    "link in place of {}",
                    e.name()
                );
                put(t, &mut e)?;
            }
        }
        Archive::TarXz | Archive::TarGz => {
            let r = std::io::BufReader::new(file);
            let dec: Box<dyn Read> = if kind == Archive::TarXz {
                Box::new(liblzma::read::XzDecoder::new(r))
            } else {
                Box::new(flate2::read::GzDecoder::new(r))
            };
            let mut t = tar::Archive::new(dec);
            for e in t.entries().context("cannot read tar")? {
                let mut e = e.context("cannot read tar entry")?;
                let name = e
                    .path()
                    .context("bad tar path")?
                    .to_string_lossy()
                    .into_owned();
                ensure!(safe_name(&name), "unsafe path in archive: {name}");
                let Some(tool) = wanted_tool(&name, wanted) else {
                    continue;
                };
                ensure!(e.header().entry_type().is_file(), "link in place of {name}");
                put(tool, &mut e)?;
            }
        }
    }
    for t in wanted {
        ensure!(found.contains(t), "{} not found in archive", t.id());
    }
    Ok(())
}

/// Unzip a wheel into `out`: zip-slip is an error, links and `*.data/` entries are skipped, total size capped.
pub(crate) fn unzip_wheel(wheel: &Path, out: &Path, cap: u64) -> Result<()> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(wheel)?).context("cannot read wheel")?;
    let mut total = 0u64;
    for i in 0..z.len() {
        let mut e = z.by_index(i).context("cannot read wheel entry")?;
        ensure!(safe_name(e.name()), "unsafe path in wheel: {}", e.name());
        let name = e.name().replace('\\', "/");
        let linked = e.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000);
        if e.is_dir() || linked || name.split('/').next().is_some_and(|d| d.ends_with(".data")) {
            continue;
        }
        total += e.size();
        ensure!(total <= cap, "wheel unpacks too large");
        let dest: PathBuf = out.join(&name);
        std::fs::create_dir_all(dest.parent().context("bad wheel path")?)?;
        copy_capped(&mut e, &dest, cap)?;
    }
    Ok(())
}

/// Best-effort platform fixups on a freshly unpacked binary: exec bit (unix), quarantine (mac).
pub(crate) fn finish_binary(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)) {
            log::warn!("tools: cannot set exec bit on {}: {e}", path.display());
        }
    }
    #[cfg(target_os = "macos")]
    {
        // Our own HTTP client never sets it; strip anyway in case a mirror or proxy did.
        let _ = std::process::Command::new("xattr")
            .args(["-d", "com.apple.quarantine"])
            .arg(path)
            .output();
    }
    let _ = path;
}
