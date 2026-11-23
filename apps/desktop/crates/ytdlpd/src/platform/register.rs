//! Registers the `ytdlpd://` scheme. Windows: HKCU entries written at run time (portable builds).
//! macOS: `CFBundleURLTypes` in Info.plist, written by packaging. Linux: a `.desktop` file with
//! `x-scheme-handler/ytdlpd`, shipped by packages and written here for AppImage and portable use.
use std::path::Path;

use anyhow::Result;

/// Registry values for the scheme under `<base>`: (subkey, value name or default, data).
#[cfg_attr(not(windows), allow(dead_code))]
fn entries(exe: &Path) -> Vec<(String, Option<&'static str>, String)> {
    let exe = exe.display();
    vec![
        ("ytdlpd".into(), None, "URL:ytdlpd".into()),
        ("ytdlpd".into(), Some("URL Protocol"), String::new()),
        (r"ytdlpd\DefaultIcon".into(), None, format!("\"{exe}\",0")),
        (
            r"ytdlpd\shell\open\command".into(),
            None,
            format!("\"{exe}\" add \"%1\""),
        ),
    ]
}

/// `.desktop` file text for the scheme handler.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=ytdlpd\nExec=\"{}\" add %u\nTerminal=false\nNoDisplay=true\nMimeType=x-scheme-handler/ytdlpd;\nCategories=Network;\n",
        exe.display()
    )
}

/// `Info.plist` fragment packaging inserts (macOS).
#[allow(dead_code)]
pub const INFO_PLIST_URL_TYPES: &str = "<key>CFBundleURLTypes</key><array><dict><key>CFBundleURLName</key><string>ytdlpd</string><key>CFBundleURLSchemes</key><array><string>ytdlpd</string></array></dict></array>";

/// Make sure the scheme points at this executable (idempotent).
pub fn ensure() -> Result<()> {
    #[cfg(windows)]
    return windows::write(r"Software\Classes", &std::env::current_exe()?);
    #[cfg(target_os = "linux")]
    return linux::write();
    #[cfg(not(any(windows, target_os = "linux")))]
    Ok(())
}

#[cfg(windows)]
mod windows {
    use std::io;
    use std::path::Path;
    use std::ptr::{null, null_mut};

    use anyhow::{Context as _, Result};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegSetValueExW,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn set(base: &str, sub: &str, name: Option<&str>, data: &str) -> io::Result<()> {
        let key_path = wide(&format!(r"{base}\{sub}"));
        let mut key: HKEY = null_mut();
        // SAFETY: valid NUL-terminated wide string and live out pointer; the key is closed below.
        let rc = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                null(),
                &mut key,
                null_mut(),
            )
        };
        if rc != 0 {
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
        let (name, data) = (name.map(wide), wide(data));
        // SAFETY: `key` is open; name and data are NUL-terminated wide strings sized in bytes.
        let rc = unsafe {
            RegSetValueExW(
                key,
                name.as_ref().map_or(null(), |n| n.as_ptr()),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        // SAFETY: `key` was opened above.
        unsafe { RegCloseKey(key) };
        if rc != 0 {
            Err(io::Error::from_raw_os_error(rc as i32))
        } else {
            Ok(())
        }
    }

    /// Writes the scheme entries under `HKCU\<base>`.
    pub fn write(base: &str, exe: &Path) -> Result<()> {
        for (sub, name, data) in super::entries(exe) {
            set(base, &sub, name, &data)
                .with_context(|| format!("cannot write HKCU\\{base}\\{sub}"))?;
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::System::Registry::RegDeleteTreeW;

        #[test]
        fn writes_and_cleans_a_scratch_key() {
            let base = format!(r"Software\ytdlpd-test-{}", std::process::id());
            write(&base, Path::new(r"C:\x\ytdlpd.exe")).unwrap();
            let out = std::process::Command::new("reg")
                .args(["query", &format!(r"HKCU\{base}\ytdlpd\shell\open\command")])
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let w = wide(&base);
            // SAFETY: valid NUL-terminated wide string; removes only the scratch key created above.
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, w.as_ptr()) };
            assert!(text.contains(r#""C:\x\ytdlpd.exe" add "%1""#), "{text}");
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use anyhow::{Context as _, Result};

    pub fn write() -> Result<()> {
        let exe = std::env::current_exe()?;
        let home = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
            })
            .context("no data home")?;
        let dir = home.join("applications");
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("ytdlpd-url-handler.desktop");
        let text = super::desktop_entry(&exe);
        if std::fs::read_to_string(&file).ok().as_deref() != Some(&text) {
            std::fs::write(&file, text)?;
            // best effort: the handler works once the desktop re-reads its database
            let _ = std::process::Command::new("xdg-mime")
                .args([
                    "default",
                    "ytdlpd-url-handler.desktop",
                    "x-scheme-handler/ytdlpd",
                ])
                .status();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_text() {
        let e = entries(Path::new("/x/ytdlpd"));
        assert!(e.iter().any(|(s, n, d)| s.ends_with(r"shell\open\command")
            && n.is_none()
            && d == r#""/x/ytdlpd" add "%1""#));
        assert!(e.iter().any(|(_, n, _)| *n == Some("URL Protocol")));
        let d = desktop_entry(Path::new("/x/ytdlpd"));
        assert!(
            d.contains("MimeType=x-scheme-handler/ytdlpd;")
                && d.contains(r#"Exec="/x/ytdlpd" add %u"#)
        );
        assert!(INFO_PLIST_URL_TYPES.contains("<string>ytdlpd</string>"));
    }
}
