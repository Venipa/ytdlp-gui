//! Pure decisions: which asset for which tool on which OS/arch, and checksum file parsing.
use crate::Tool;
use anyhow::{Context, Result, bail};

/// Host operating system family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

/// CPU architecture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X64,
    Arm64,
}

/// An OS/arch pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub os: Os,
    pub arch: Arch,
}

impl Target {
    /// The six supported targets.
    pub const ALL: [Target; 6] = [
        Target {
            os: Os::Windows,
            arch: Arch::X64,
        },
        Target {
            os: Os::Windows,
            arch: Arch::Arm64,
        },
        Target {
            os: Os::Mac,
            arch: Arch::X64,
        },
        Target {
            os: Os::Mac,
            arch: Arch::Arm64,
        },
        Target {
            os: Os::Linux,
            arch: Arch::X64,
        },
        Target {
            os: Os::Linux,
            arch: Arch::Arm64,
        },
    ];

    /// The target this binary runs on.
    pub fn current() -> Target {
        let os = if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        };
        let arch = if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else {
            Arch::X64
        };
        Target { os, arch }
    }

    /// Rust triple as used in denoland asset names.
    pub fn triple(self) -> &'static str {
        match (self.os, self.arch) {
            (Os::Windows, Arch::X64) => "x86_64-pc-windows-msvc",
            (Os::Windows, Arch::Arm64) => "aarch64-pc-windows-msvc",
            (Os::Mac, Arch::X64) => "x86_64-apple-darwin",
            (Os::Mac, Arch::Arm64) => "aarch64-apple-darwin",
            (Os::Linux, Arch::X64) => "x86_64-unknown-linux-gnu",
            (Os::Linux, Arch::Arm64) => "aarch64-unknown-linux-gnu",
        }
    }

    /// BtbN platform token (Windows and Linux only).
    fn btbn_plat(self) -> &'static str {
        match (self.os, self.arch) {
            (Os::Windows, Arch::X64) => "win64",
            (Os::Windows, Arch::Arm64) => "winarm64",
            (_, Arch::X64) => "linux64",
            (_, Arch::Arm64) => "linuxarm64",
        }
    }
}

/// Archive container of a download.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Archive {
    Zip,
    TarXz,
    TarGz,
}

/// Where the asset name and checksum come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// BtbN: `<base>/checksums.sha256` lists every asset; pick by [`select_btbn`].
    Btbn { base: String },
    /// martin-riedl: `url` answers 307; the checksum is `<final url>.sha256`.
    Redirect { url: String },
    /// Fixed asset url plus its checksum url.
    Direct { url: String, sha_url: String },
}

/// What to download for one tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub kind: Kind,
    /// Tools found inside the archive (all get installed).
    pub tools: Vec<Tool>,
    /// Archive type.
    pub archive: Archive,
}

fn base(mirror: Option<&str>, sub: &str, default: &str) -> String {
    match mirror {
        Some(m) => format!("{}/{sub}", m.trim_end_matches('/')),
        None => default.to_string(),
    }
}

/// Fetch plan for `tool` on `t`; `mirror` replaces every base url (`<mirror>/{btbn,martin-riedl,deno,pypi}`).
pub fn plan(tool: Tool, t: Target, mirror: Option<&str>) -> Plan {
    match (tool, t.os) {
        (Tool::Deno, _) => {
            let b = base(
                mirror,
                "deno",
                "https://github.com/denoland/deno/releases/latest/download",
            );
            let url = format!("{b}/deno-{}.zip", t.triple());
            Plan {
                kind: Kind::Direct {
                    sha_url: format!("{url}.sha256sum"),
                    url,
                },
                tools: vec![Tool::Deno],
                archive: Archive::Zip,
            }
        }
        (_, Os::Mac) => {
            let b = base(mirror, "martin-riedl", "https://ffmpeg.martin-riedl.de");
            let arch = if t.arch == Arch::Arm64 {
                "arm64"
            } else {
                "amd64"
            };
            let url = format!("{b}/redirect/latest/macos/{arch}/release/{}.zip", tool.id());
            Plan {
                kind: Kind::Redirect { url },
                tools: vec![tool],
                archive: Archive::Zip,
            }
        }
        _ => {
            let b = base(
                mirror,
                "btbn",
                "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest",
            );
            let archive = if t.os == Os::Windows {
                Archive::Zip
            } else {
                Archive::TarXz
            };
            Plan {
                kind: Kind::Btbn { base: b },
                tools: vec![Tool::Ffmpeg, Tool::Ffprobe],
                archive,
            }
        }
    }
}

/// Pick the gpl static build with the highest `n<maj>.<min>` branch from a `checksums.sha256` body.
/// Returns (asset name, sha256).
pub fn select_btbn(checksums: &str, t: Target) -> Result<(String, String)> {
    let plat = t.btbn_plat();
    let ext = if t.os == Os::Windows { "zip" } else { "tar.xz" };
    let mut best: Option<((u32, u32), String, String)> = None;
    for line in checksums.lines() {
        let mut it = line.split_whitespace();
        let (Some(hash), Some(name)) = (it.next(), it.next()) else {
            continue;
        };
        let name = name.trim_start_matches('*');
        let Some(rest) = name.strip_prefix("ffmpeg-n") else {
            continue;
        };
        let Some((ver, tail)) = rest.split_once("-latest-") else {
            continue;
        };
        let Some((maj, min)) = ver.split_once('.') else {
            continue;
        };
        let (Ok(maj), Ok(min)) = (maj.parse::<u32>(), min.parse::<u32>()) else {
            continue;
        };
        // exact tail: excludes `-gpl-shared-` and `-lgpl-`
        if tail != format!("{plat}-gpl-{ver}.{ext}") {
            continue;
        }
        if best.as_ref().is_none_or(|b| (maj, min) > b.0) {
            best = Some(((maj, min), name.to_string(), parse_checksum(hash, None)?));
        }
    }
    let (_, name, hash) = best.with_context(|| {
        format!("cannot find a gpl ffmpeg build for {plat} in checksums.sha256")
    })?;
    Ok((name, hash))
}

/// Extract a sha256 from `<hash> *<name>`, `<hash>  <name>`, a bare hash, or PowerShell
/// `Get-FileHash` output (`Hash : <HASH>`, used by deno's Windows `.sha256sum`).
/// With `name`, a line naming another file is skipped. No valid hash is an error (fail closed).
pub fn parse_checksum(text: &str, name: Option<&str>) -> Result<String> {
    let is_hash = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
    for line in text.lines() {
        if let Some((k, v)) = line.trim().split_once(':')
            && k.trim().eq_ignore_ascii_case("hash")
            && is_hash(v.trim())
        {
            return Ok(v.trim().to_ascii_lowercase());
        }
    }
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let Some(h) = it.next().filter(|h| is_hash(h)) else {
            continue;
        };
        let file = it.next().map(|f| f.trim_start_matches('*'));
        if let (Some(want), Some(file)) = (name, file)
            && file.rsplit(['/', '\\']).next() != Some(want)
        {
            continue;
        }
        return Ok(h.to_ascii_lowercase());
    }
    bail!("cannot find a sha256 in checksum file")
}

#[cfg(test)]
mod tests {
    use super::*;

    const H1: &str = "bf51b6948d275f82f8bc507d32c06842f6f02917f1d0bcf2a4b9b79a23d1236c";
    const H2: &str = "0ea60bd3943fc0d0f802e892b4636b49665b5b52fd212ea376d0ea49b5f9fed8";

    #[test]
    fn matrix_all_six() {
        for t in Target::ALL {
            let d = plan(Tool::Deno, t, None);
            assert_eq!(d.tools, vec![Tool::Deno]);
            let Kind::Direct { url, sha_url } = &d.kind else {
                panic!()
            };
            assert_eq!(
                url,
                &format!(
                    "https://github.com/denoland/deno/releases/latest/download/deno-{}.zip",
                    t.triple()
                )
            );
            assert_eq!(sha_url, &format!("{url}.sha256sum"));
            let f = plan(Tool::Ffprobe, t, None);
            match t.os {
                Os::Mac => {
                    let arch = if t.arch == Arch::Arm64 {
                        "arm64"
                    } else {
                        "amd64"
                    };
                    assert_eq!(f.tools, vec![Tool::Ffprobe]);
                    assert_eq!(
                        f.kind,
                        Kind::Redirect {
                            url: format!(
                                "https://ffmpeg.martin-riedl.de/redirect/latest/macos/{arch}/release/ffprobe.zip"
                            )
                        }
                    );
                }
                os => {
                    assert_eq!(f.tools, vec![Tool::Ffmpeg, Tool::Ffprobe]);
                    assert_eq!(
                        f.archive,
                        if os == Os::Windows {
                            Archive::Zip
                        } else {
                            Archive::TarXz
                        }
                    );
                    assert!(matches!(f.kind, Kind::Btbn { .. }));
                }
            }
        }
        assert_eq!(Target::ALL[1].triple(), "aarch64-pc-windows-msvc");
    }

    #[test]
    fn mirror_replaces_bases() {
        let t = Target::ALL[0];
        assert_eq!(
            plan(Tool::Ffmpeg, t, Some("http://x:1/")).kind,
            Kind::Btbn {
                base: "http://x:1/btbn".into()
            }
        );
        let Kind::Direct { url, .. } = plan(Tool::Deno, t, Some("http://x:1")).kind else {
            panic!()
        };
        assert_eq!(url, "http://x:1/deno/deno-x86_64-pc-windows-msvc.zip");
    }

    #[test]
    fn btbn_picks_highest_gpl_static() {
        let sums = format!(
            "{H1}  ffmpeg-n8.1-latest-win64-gpl-8.1.zip\n\
             {H1}  ffmpeg-n8.1-latest-win64-gpl-shared-8.1.zip\n\
             {H1}  ffmpeg-n9.0-latest-win64-lgpl-9.0.zip\n\
             {H1}  ffmpeg-master-latest-win64-gpl.zip\n\
             {H2} *ffmpeg-n9.0-latest-win64-gpl-9.0.zip\n\
             {H1}  ffmpeg-n9.0-latest-win64-gpl-shared-9.0.zip\n\
             {H1}  ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz\n"
        );
        let (name, h) = select_btbn(&sums, Target::ALL[0]).unwrap();
        assert_eq!(
            (name.as_str(), h.as_str()),
            ("ffmpeg-n9.0-latest-win64-gpl-9.0.zip", H2)
        );
        assert_eq!(
            select_btbn(&sums, Target::ALL[4]).unwrap().0,
            "ffmpeg-n9.0-latest-linux64-gpl-9.0.tar.xz"
        );
        assert!(select_btbn(&sums, Target::ALL[1]).is_err());
        let ten = format!("{H1}  ffmpeg-n10.0-latest-win64-gpl-10.0.zip\n{sums}");
        assert_eq!(
            select_btbn(&ten, Target::ALL[0]).unwrap().0,
            "ffmpeg-n10.0-latest-win64-gpl-10.0.zip"
        );
    }

    #[test]
    fn checksum_formats() {
        assert_eq!(
            parse_checksum(&format!("{H1} *a.zip\n"), Some("a.zip")).unwrap(),
            H1
        );
        assert_eq!(
            parse_checksum(&format!("{H1}  a.zip\n"), Some("a.zip")).unwrap(),
            H1
        );
        assert!(parse_checksum(&format!("{H1}  b.zip\n"), Some("a.zip")).is_err());
        assert_eq!(
            parse_checksum(&format!("{H1}  ffmpeg.zip\n"), None).unwrap(),
            H1
        );
        // deno windows .sha256sum is Get-FileHash output
        let ps = format!(
            "\nAlgorithm : SHA256\nHash      : {}\nPath      : C:\\a\\deno\\deno.zip\n\n",
            H1.to_uppercase()
        );
        assert_eq!(parse_checksum(&ps, None).unwrap(), H1);
        assert!(parse_checksum("", None).is_err());
        assert!(parse_checksum("<html>404</html>", None).is_err());
        assert!(parse_checksum("abc  a.zip", None).is_err());
    }
}
