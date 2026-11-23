//! Cookie files kept private under `<data>/cookies/<hash>.txt` (pasted text or an imported file).
use std::{
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

/// Netscape cookie text sanity check: a header comment or at least one 7-field tab line.
fn looks_netscape(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim_start();
        l.starts_with("# Netscape HTTP Cookie File")
            || l.starts_with("# HTTP Cookie File")
            || (!l.starts_with('#') && l.split('\t').count() >= 7)
    })
}

/// Stores pasted cookie text (Netscape format) and returns the file path, named by content hash.
/// Owner-only permissions: mode 0600 on unix, an owner-only ACL on Windows.
pub fn store_text(data_dir: &Path, text: &str) -> Result<PathBuf> {
    if !looks_netscape(text) {
        bail!("the text is not in Netscape cookie format");
    }
    let hash = Sha256::digest(text.as_bytes());
    let name: String = hash.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let dir = data_dir.join("cookies");
    std::fs::create_dir_all(&dir).context("cannot create the cookies folder")?;
    let path = dir.join(format!("{name}.txt"));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(&path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .context("cannot write the cookie file")?;
    #[cfg(windows)]
    restrict_acl(&path)?;
    Ok(path)
}

/// Copies a user's cookie file into the private folder (see [`store_text`]).
pub fn store_file(data_dir: &Path, src: &Path) -> Result<PathBuf> {
    let text = std::fs::read_to_string(src).context("cannot read the cookie file")?;
    store_text(data_dir, &text)
}

#[cfg(windows)]
fn restrict_acl(path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let user = std::env::var("USERNAME").context("cannot find the user name")?;
    let st = std::process::Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(format!("{user}:(R,W,D)"))
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .context("cannot run icacls")?;
    if !st.status.success() {
        bail!("cannot restrict the cookie file permissions");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Netscape HTTP Cookie File\n127.0.0.1\tFALSE\t/\tFALSE\t0\tsid\tabc\n";

    #[test]
    fn stores_private_and_deduplicates() {
        let d = std::env::temp_dir().join(format!("ytdlpd-cookies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let a = store_text(&d, SAMPLE).unwrap();
        assert_eq!(a, store_text(&d, SAMPLE).unwrap());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), SAMPLE);
        assert!(a.starts_with(d.join("cookies")));
        #[cfg(unix)]
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&a).unwrap().permissions())
                & 0o777,
            0o600
        );
        #[cfg(windows)]
        {
            let out = std::process::Command::new("icacls")
                .arg(&a)
                .output()
                .unwrap();
            let acl = String::from_utf8_lossy(&out.stdout).to_lowercase();
            // match ACL entries (`<who>:(`), not the path, which can itself contain `\users\`
            for who in ["everyone:(", "\\users:("] {
                assert!(!acl.contains(who), "{acl}");
            }
            assert!(acl.contains(&std::env::var("USERNAME").unwrap().to_lowercase()));
        }
        assert!(store_text(&d, "just some text").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
