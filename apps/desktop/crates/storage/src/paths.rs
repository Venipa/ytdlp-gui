use std::path::{Path, PathBuf};

/// Directory name under the OS dirs: `ytdlpd` in release, `ytdlpd.dev` in debug builds.
pub fn app_dir_name() -> &'static str {
    if cfg!(debug_assertions) {
        "ytdlpd.dev"
    } else {
        "ytdlpd"
    }
}

/// Resolved application directories.
#[derive(Debug, Clone, PartialEq)]
pub struct Paths {
    /// settings.json
    pub config: PathBuf,
    /// `ytdlpd.db`, `tools/`, `python-site/`, `cookies/`, `logs/`
    pub data: PathBuf,
    /// tool downloads, `yt-dlp/`, thumbnails
    pub cache: PathBuf,
}

impl Paths {
    /// OS defaults, or `<over>/{config,data,cache}` when `--data-dir` is given.
    pub fn resolve(data_dir_override: Option<&Path>) -> Paths {
        if let Some(o) = data_dir_override {
            return Paths {
                config: o.join("config"),
                data: o.join("data"),
                cache: o.join("cache"),
            };
        }
        let name = app_dir_name();
        let data = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(name);
        Paths {
            config: dirs::config_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join(name),
            // Windows keeps the cache under the data dir (`%LOCALAPPDATA%\ytdlpd\cache`).
            cache: if cfg!(windows) {
                data.join("cache")
            } else {
                dirs::cache_dir()
                    .unwrap_or_else(std::env::temp_dir)
                    .join(name)
            },
            data,
        }
    }
    /// `<config>/settings.json`
    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }
    /// `<data>/ytdlpd.db`
    pub fn db_file(&self) -> PathBuf {
        self.data.join("ytdlpd.db")
    }
    /// `<data>/logs`
    pub fn logs(&self) -> PathBuf {
        self.data.join("logs")
    }
    /// Creates all directories.
    pub fn ensure(&self) -> std::io::Result<()> {
        [&self.config, &self.data, &self.cache]
            .into_iter()
            .chain([&self.logs()])
            .try_for_each(std::fs::create_dir_all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_and_default() {
        let p = Paths::resolve(Some(Path::new("/x")));
        assert_eq!(p.db_file(), Path::new("/x/data/ytdlpd.db"));
        assert_eq!(p.settings_file(), Path::new("/x/config/settings.json"));
        let d = Paths::resolve(None);
        assert!(d.data.ends_with(app_dir_name()) && d.config.ends_with(app_dir_name()));
        assert_eq!(app_dir_name(), "ytdlpd.dev"); // tests run in debug
    }
}
