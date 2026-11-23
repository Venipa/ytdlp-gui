//! Number formatting for the columns: sizes, speeds and durations, always with tabular figures.

/// `4.1 MB`, `1.2 GB`: binary units, one decimal from MB up.
pub fn bytes(n: i64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = n.max(0) as f64;
    let mut unit = 0;
    while value >= 1024. && unit < UNITS.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    match unit {
        0 | 1 => format!("{value:.0} {}", UNITS[unit]),
        _ => format!("{value:.1} {}", UNITS[unit]),
    }
}

/// `2.3 MB/s` from bytes per second.
pub fn speed(bytes_per_second: f64) -> String {
    format!("{}/s", bytes(bytes_per_second as i64))
}

/// `3:33` or `1:02:03` from seconds.
pub fn duration(seconds: f64) -> String {
    let total = seconds.max(0.) as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    match h {
        0 => format!("{m}:{s:02}"),
        _ => format!("{h}:{m:02}:{s:02}"),
    }
}

/// `1080p`, `best`, `mp3`: the short label of a preset, as stored in a row's format column.
pub fn preset_label(preset: domain::Preset) -> String {
    use domain::{Preset, VideoQuality};
    match preset {
        Preset::Video(VideoQuality::Best) => "best".into(),
        Preset::Video(q) => format!("{}p", q.as_str()),
        Preset::Audio(a) => a.as_str().into(),
        Preset::Custom => "custom".into(),
    }
}

/// `14:03:09` in the local time zone from unix milliseconds.
pub fn clock(at_ms: i64) -> String {
    jiff::Timestamp::from_millisecond(at_ms)
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_speeds_and_durations() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(4_300_000), "4.1 MB");
        assert_eq!(bytes(1_288_490_189), "1.2 GB");
        assert_eq!(speed(2_411_724.), "2.3 MB/s");
        assert_eq!(duration(213.), "3:33");
        assert_eq!(duration(3723.), "1:02:03");
        assert_eq!(preset_label(domain::Preset::DEFAULT), "best");
        assert_eq!(preset_label("video:1080".parse().unwrap()), "1080p");
    }
}
