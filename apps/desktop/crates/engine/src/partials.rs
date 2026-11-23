//! Cleanup of a cancelled job's partial files, by the exact names reported in `files` events.
use std::path::Path;

/// Delete each listed file that exists. Returns how many were removed. Pause keeps them: just do not call this.
pub fn cleanup(paths: &[String]) -> usize {
    paths
        .iter()
        .filter(|p| Path::new(p).is_file() && std::fs::remove_file(p).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    #[test]
    fn removes_only_listed_existing() {
        let d = std::env::temp_dir().join(format!("ytdlpd-partials-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (a, keep) = (d.join("a.mp4.part"), d.join("keep.mp4"));
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&keep, b"x").unwrap();
        let n = super::cleanup(&[
            a.to_string_lossy().into(),
            d.join("missing.ytdl").to_string_lossy().into(),
        ]);
        assert_eq!(n, 1);
        assert!(!a.exists() && keep.exists());
        std::fs::remove_dir_all(d).unwrap();
    }
}
