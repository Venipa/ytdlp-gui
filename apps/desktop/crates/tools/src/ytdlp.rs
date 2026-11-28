//! yt-dlp + yt-dlp-ejs pair update from PyPI (D-104). The two wheels always move together: the
//! yt-dlp wheel's METADATA pins the exact yt-dlp-ejs version.
use crate::{
    Dirs,
    fetch::{Ctx, download, get_text, remove_dir_retry, rename_retry, unzip_wheel},
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};

const SITE_CAP: u64 = 300 << 20;

/// One PyPI wheel (py3-none-any) with its expected sha256.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wheel {
    pub name: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
}

/// A newer yt-dlp together with the yt-dlp-ejs it pins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairUpdate {
    pub ytdlp: Wheel,
    pub ejs: Wheel,
}

/// Result of a successful install: persist `active`; keep `previous` for rollback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub active: PathBuf,
    pub previous: Option<PathBuf>,
}

/// `a > b` comparing dotted numeric parts (yt-dlp versions are dates: 2026.10.26, 2026.10.26.1).
fn newer(a: &str, b: &str) -> bool {
    let p = |s: &str| {
        s.split('.')
            .map(|x| x.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    p(a) > p(b)
}

/// Pick the py3-none-any wheel of `version` (or the latest) from a PyPI JSON document.
fn parse_pypi(doc: &Value, name: &str, version: Option<&str>) -> Result<Wheel> {
    let version = match version {
        Some(v) => v.to_string(),
        None => doc["info"]["version"]
            .as_str()
            .context("cannot find version in PyPI json")?
            .to_string(),
    };
    let files = doc["releases"][&version]
        .as_array()
        .with_context(|| format!("{name} {version} not on PyPI"))?;
    let f = files
        .iter()
        .find(|f| {
            f["packagetype"] == "bdist_wheel"
                && f["yanked"] != true
                && f["filename"]
                    .as_str()
                    .is_some_and(|n| n.ends_with("-py3-none-any.whl"))
        })
        .with_context(|| format!("no py3-none-any wheel for {name} {version}"))?;
    let sha = f["digests"]["sha256"]
        .as_str()
        .context("wheel has no sha256 digest")?;
    let url = f["url"].as_str().context("wheel has no url")?;
    Ok(Wheel {
        name: name.into(),
        version,
        url: url.into(),
        sha256: sha.into(),
    })
}

/// Exact `yt-dlp-ejs==X` pin from wheel METADATA (extra markers and parentheses tolerated).
fn ejs_pin(metadata: &str) -> Result<String> {
    for line in metadata.lines() {
        let Some(req) = line.strip_prefix("Requires-Dist:") else {
            continue;
        };
        let req = req
            .split(';')
            .next()
            .unwrap_or("")
            .replace(['(', ')', ' '], "");
        let Some(spec) = req
            .strip_prefix("yt-dlp-ejs")
            .or_else(|| req.strip_prefix("yt_dlp_ejs"))
        else {
            continue;
        };
        if let Some(v) = spec
            .strip_prefix("==")
            .filter(|v| !v.is_empty() && !v.contains(['*', ',', '<', '>']))
        {
            return Ok(v.to_string());
        }
    }
    bail!("yt-dlp wheel has no exact yt-dlp-ejs pin")
}

fn read_metadata(wheel: &Path) -> Result<String> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(wheel)?).context("cannot read wheel")?;
    for i in 0..z.len() {
        let mut e = z.by_index(i)?;
        if e.name().ends_with(".dist-info/METADATA") {
            let mut s = String::new();
            e.read_to_string(&mut s).context("cannot read METADATA")?;
            return Ok(s);
        }
    }
    bail!("wheel has no METADATA")
}

fn pypi_url(ctx: &Ctx, name: &str) -> String {
    match &ctx.mirror {
        Some(m) => format!("{}/pypi/{name}/json", m.trim_end_matches('/')),
        None => format!("https://pypi.org/pypi/{name}/json"),
    }
}

async fn fetch_wheel(ctx: &Ctx, w: &Wheel, dir: &Path) -> Result<tempfile::NamedTempFile> {
    download(ctx, &w.url, &w.sha256, dir, &|_, _| {})
        .await
        .with_context(|| format!("cannot fetch {} {}", w.name, w.version))
}

/// Latest yt-dlp from PyPI plus its pinned yt-dlp-ejs; `None` when `current` is already latest.
/// Downloads the (small) yt-dlp wheel to read its METADATA pin.
pub async fn check_ytdlp_update(dirs: &Dirs, current: Option<&str>) -> Result<Option<PairUpdate>> {
    check_with(&Ctx::new()?, dirs, current).await
}

pub(crate) async fn check_with(
    ctx: &Ctx,
    dirs: &Dirs,
    current: Option<&str>,
) -> Result<Option<PairUpdate>> {
    let doc: Value = serde_json::from_str(&get_text(ctx, &pypi_url(ctx, "yt-dlp")).await?)
        .context("cannot parse PyPI json for yt-dlp")?;
    let ytdlp = parse_pypi(&doc, "yt-dlp", None)?;
    if current.is_some_and(|c| !newer(&ytdlp.version, c)) {
        return Ok(None);
    }
    let f = fetch_wheel(ctx, &ytdlp, &dirs.dl()).await?;
    let pin = ejs_pin(&read_metadata(f.path())?)?;
    let doc: Value = serde_json::from_str(&get_text(ctx, &pypi_url(ctx, "yt-dlp-ejs")).await?)
        .context("cannot parse PyPI json for yt-dlp-ejs")?;
    let ejs = parse_pypi(&doc, "yt-dlp-ejs", Some(&pin))?;
    Ok(Some(PairUpdate { ytdlp, ejs }))
}

/// Download both wheels, unzip into `<data>/python-site/<ver>.tmp`, run `verify` on that dir (the
/// real one runs `ytdlpd --self-test --site <dir>` in a throwaway process), then rename it to
/// `<data>/python-site/<ver>`. On any failure nothing is left and the caller keeps its old pointer.
/// `current_active` is returned as `previous` and kept with the new site; older sites are deleted.
pub async fn install_ytdlp_pair(
    dirs: &Dirs,
    update: &PairUpdate,
    current_active: Option<&Path>,
    verify: impl FnOnce(&Path) -> Result<()> + Send + 'static,
) -> Result<Installed> {
    install_with(&Ctx::new()?, dirs, update, current_active, verify).await
}

/// A folder's own name, for log lines (no home path).
fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

pub(crate) async fn install_with(
    ctx: &Ctx,
    dirs: &Dirs,
    update: &PairUpdate,
    current_active: Option<&Path>,
    verify: impl FnOnce(&Path) -> Result<()> + Send + 'static,
) -> Result<Installed> {
    let root = dirs.data.join("python-site");
    let (tmp, fin) = (
        root.join(format!("{}.tmp", update.ytdlp.version)),
        root.join(&update.ytdlp.version),
    );
    std::fs::create_dir_all(&root).context("cannot create python-site")?;
    let a = fetch_wheel(ctx, &update.ytdlp, &dirs.dl()).await?;
    let b = fetch_wheel(ctx, &update.ejs, &dirs.dl()).await?;
    let t = tmp.clone();
    let built = tokio::task::spawn_blocking(move || -> Result<()> {
        remove_dir_retry(&t)?;
        std::fs::create_dir_all(&t)?;
        unzip_wheel(a.path(), &t, SITE_CAP)?;
        unzip_wheel(b.path(), &t, SITE_CAP)?;
        verify(&t).context("self-test of the new yt-dlp failed")
    })
    .await
    .context("install task failed")
    .and_then(|r| r);
    let (t, f) = (tmp.clone(), fin.clone());
    let moved = tokio::task::spawn_blocking(move || -> Result<()> {
        built?;
        remove_dir_retry(&f)?;
        rename_retry(&t, &f).context("cannot move site into place")
    })
    .await
    .context("install task failed")
    .and_then(|r| r);
    if let Err(e) = moved {
        if let Err(e) = remove_dir_retry(&tmp) {
            log::warn!("tools: cannot remove {}: {e}", dir_name(&tmp));
        }
        return Err(e);
    }
    let previous = current_active.filter(|p| *p != fin).map(Path::to_path_buf);
    // keep only active + previous
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten().map(|e| e.path()) {
            if e != fin
                && Some(&e) != previous.as_ref()
                && let Err(err) = remove_dir_retry(&e)
            {
                log::warn!("tools: cannot remove {}: {err}", dir_name(&e));
            }
        }
    }
    Ok(Installed {
        active: fin,
        previous,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_order_is_numeric() {
        assert!(newer("2026.10.26", "2026.9.1"));
        assert!(newer("2026.10.26.1", "2026.10.26"));
        assert!(!newer("2026.10.26", "2026.10.26"));
    }

    #[test]
    fn pin_parse() {
        let md = "Name: yt-dlp\nRequires-Dist: brotli; implementation_name == \"cpython\" and extra == \"default\"\nRequires-Dist: yt-dlp-ejs (==0.8.0) ; extra == \"default\"\n";
        assert_eq!(ejs_pin(md).unwrap(), "0.8.0");
        assert_eq!(
            ejs_pin("Requires-Dist: yt-dlp-ejs==0.9.1\n").unwrap(),
            "0.9.1"
        );
        assert!(ejs_pin("Requires-Dist: yt-dlp-ejs>=0.8\n").is_err());
        assert!(ejs_pin("Requires-Dist: other==1\n").is_err());
    }

    #[test]
    fn pypi_pick() {
        let doc: Value = serde_json::json!({"info": {"version": "2.0"}, "releases": {"2.0": [
            {"packagetype": "sdist", "filename": "x-2.0.tar.gz", "url": "u0", "digests": {"sha256": "00"}},
            {"packagetype": "bdist_wheel", "filename": "x-2.0-py3-none-any.whl", "url": "u1", "digests": {"sha256": "11"}}]}});
        let w = parse_pypi(&doc, "x", None).unwrap();
        assert_eq!(
            (w.version.as_str(), w.url.as_str(), w.sha256.as_str()),
            ("2.0", "u1", "11")
        );
        assert!(parse_pypi(&doc, "x", Some("3.0")).is_err());
    }
}
