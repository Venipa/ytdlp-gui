//! Presets to yt-dlp options (architecture.md section 4 "Opts mapping"). Pure: domain types in, JSON out.
use domain::{
    AudioFormat, CookiesMode, JobSpec, Preset, ProbeOpts, SponsorBlockMode, VideoQuality,
};
use serde_json::{Map, Value, json};

/// Default single-video output template.
pub const DEFAULT_OUTTMPL: &str = "%(title).180B [%(id)s].%(ext)s";
/// Default playlist output template.
pub const PLAYLIST_OUTTMPL: &str =
    "%(playlist_title).80B/%(playlist_index)03d %(title).160B [%(id)s].%(ext)s";
/// Out-of-band opts key carrying CLI-style extra arguments to the worker.
pub const EXTRA_ARGS_KEY: &str = "_extra_args";
/// SponsorBlock categories used when the spec lists none.
const DEFAULT_SPONSOR: [&str; 1] = ["sponsor"];

/// Machine-level settings the host adds to every job and probe (not part of the stored `JobSpec`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    /// yt-dlp cache dir.
    pub cache_dir: Option<String>,
    /// Fallback runtime when the spec carries none: `(key, path)`.
    pub js_runtime: Option<(String, String)>,
}

/// A spec with only the essentials set; for examples, tests and dev tools.
pub fn simple_job(
    url: impl Into<String>,
    preset: Preset,
    output_dir: impl AsRef<std::path::Path>,
) -> JobSpec {
    JobSpec {
        url: url.into(),
        preset,
        format_spec: String::new(),
        output_dir: output_dir.as_ref().to_string_lossy().into_owned(),
        outtmpl: String::new(),
        playlist_index: None,
        embed_thumbnail: false,
        embed_metadata: false,
        embed_chapters: false,
        subtitles: Default::default(),
        sponsorblock: Default::default(),
        cookies: Default::default(),
        proxy: None,
        rate_limit: None,
        retries: None,
        ffmpeg_location: None,
        js_runtime: None,
        extra_args: vec![],
        aria2c: None,
        aria2c_args: vec![],
        fix: None,
    }
}

/// `1M`, `500K`, `2.5m`, plain bytes: bytes per second for `ratelimit`. `None` when unparsable.
pub fn parse_rate(s: &str) -> Option<u64> {
    let t = s.trim().to_ascii_lowercase();
    let t = t.strip_suffix("/s").unwrap_or(&t);
    let (num, mult) = match t.chars().last()? {
        'k' => (&t[..t.len() - 1], 1u64 << 10),
        'm' => (&t[..t.len() - 1], 1 << 20),
        'g' => (&t[..t.len() - 1], 1 << 30),
        _ => (t, 1),
    };
    num.trim()
        .parse::<f64>()
        .ok()
        .filter(|n| *n > 0.0)
        .map(|n| (n * mult as f64) as u64)
}

/// Shell-style split of the Settings "extra arguments" text (quotes honoured). `None` on an unbalanced quote.
pub fn split_extra_args(s: &str) -> Option<Vec<String>> {
    shlex::split(s)
}

fn common(
    o: &mut Map<String, Value>,
    env: &Env,
    ffmpeg: Option<&str>,
    js: Option<&(String, String)>,
    proxy: Option<&str>,
) {
    o.insert("quiet".into(), json!(true));
    o.insert("noprogress".into(), json!(true));
    if let Some(f) = ffmpeg {
        o.insert("ffmpeg_location".into(), json!(f));
    }
    if let Some((k, p)) = js.or(env.js_runtime.as_ref()) {
        o.insert("js_runtimes".into(), json!({ k: { "path": p } }));
    }
    if let Some(c) = &env.cache_dir {
        o.insert("cachedir".into(), json!(c));
    }
    if let Some(x) = proxy {
        o.insert("proxy".into(), json!(x));
    }
}

fn cookies(o: &mut Map<String, Value>, c: &domain::CookiesSettings) {
    match (c.mode, &c.file, &c.browser) {
        (CookiesMode::File, Some(f), _) => {
            o.insert("cookiefile".into(), json!(f));
        }
        // the worker turns the list into the tuple yt-dlp wants
        (CookiesMode::Browser, _, Some(b)) => {
            o.insert("cookiesfrombrowser".into(), json!([b, null, null, null]));
        }
        _ => {}
    }
}

/// yt-dlp options for a download job (plain JSON; the worker adds hooks and logger).
pub fn to_opts(s: &JobSpec, env: &Env) -> Value {
    let mut o = Map::new();
    common(
        &mut o,
        env,
        s.ffmpeg_location.as_deref(),
        s.js_runtime.as_ref(),
        s.proxy.as_deref(),
    );
    let mut pps: Vec<Value> = vec![];
    match s.preset {
        Preset::Video(q) => {
            o.insert("format".into(), json!("bv*+ba/b"));
            if q != VideoQuality::Best {
                o.insert("format_sort".into(), json!([format!("res:{q}")]));
            }
            o.insert("merge_output_format".into(), json!("mp4/mkv"));
        }
        Preset::Audio(a) => {
            o.insert("format".into(), json!("ba/b"));
            if a != AudioFormat::Best {
                pps.push(json!({"key": "FFmpegExtractAudio", "preferredcodec": a.as_str(), "preferredquality": 0}));
            }
        }
        Preset::Custom => {
            o.insert("format".into(), json!(s.format_spec));
            o.insert("merge_output_format".into(), json!("mp4/mkv"));
        }
    }
    // "Compatible": prefer H.264 video and AAC audio over the codec-blind default, so most sites
    // need no conversion; ahead of the resolution cap, which can mean a lower resolution
    if s.fix == Some(domain::Fix::Compatible) && s.preset.kind() == domain::MediaKind::Video {
        let mut sort = vec![json!("vcodec:h264"), json!("acodec:m4a")];
        if let Some(Value::Array(rest)) = o.get("format_sort") {
            sort.extend(rest.iter().cloned());
        }
        o.insert("format_sort".into(), Value::Array(sort));
        o.insert("merge_output_format".into(), json!("mp4"));
    }
    let tmpl = if s.outtmpl.is_empty() {
        if s.playlist_index.is_some() {
            PLAYLIST_OUTTMPL
        } else {
            DEFAULT_OUTTMPL
        }
    } else {
        &s.outtmpl
    };
    o.insert("outtmpl".into(), json!(tmpl));
    o.insert("paths".into(), json!({ "home": s.output_dir }));
    o.insert("updatetime".into(), json!(false));
    o.insert("continuedl".into(), json!(true));
    o.insert("overwrites".into(), json!(false));
    o.insert("trim_file_name".into(), json!(200));
    match s.playlist_index {
        Some(n) => {
            o.insert("noplaylist".into(), json!(false));
            o.insert("playlist_items".into(), json!(n.to_string()));
        }
        None => {
            o.insert("noplaylist".into(), json!(true));
        }
    }
    if s.subtitles.enabled && !s.subtitles.langs.is_empty() {
        o.insert("writesubtitles".into(), json!(true));
        o.insert("subtitleslangs".into(), json!(s.subtitles.langs));
        if s.subtitles.embed {
            pps.push(json!({"key": "FFmpegEmbedSubtitle", "already_have_subtitle": false}));
        }
    }
    if s.embed_metadata || s.embed_chapters {
        pps.push(json!({"key": "FFmpegMetadata", "add_chapters": s.embed_chapters, "add_metadata": s.embed_metadata}));
    }
    if s.embed_thumbnail {
        o.insert("writethumbnail".into(), json!(true));
        pps.push(json!({"key": "EmbedThumbnail"}));
    }
    if s.sponsorblock.mode != SponsorBlockMode::Off {
        let cats: Vec<String> = if s.sponsorblock.categories.is_empty() {
            DEFAULT_SPONSOR.iter().map(|c| c.to_string()).collect()
        } else {
            s.sponsorblock.categories.clone()
        };
        let remove = if s.sponsorblock.mode == SponsorBlockMode::Remove {
            cats.clone()
        } else {
            vec![]
        };
        pps.push(json!({"key": "SponsorBlock", "categories": cats, "when": "after_filter"}));
        pps.push(json!({"key": "ModifyChapters", "remove_sponsor_segments": remove, "remove_chapters_patterns": [], "remove_ranges": [], "sponsorblock_chapter_title": "[SponsorBlock]: %(category_names)l", "force_keyframes": false}));
    }
    if !pps.is_empty() {
        o.insert("postprocessors".into(), Value::Array(pps));
    }
    cookies(&mut o, &s.cookies);
    if let Some(r) = s.rate_limit.as_deref().and_then(parse_rate) {
        o.insert("ratelimit".into(), json!(r));
    }
    if let Some(r) = s.retries {
        o.insert("retries".into(), json!(r));
    }
    // a full path works: yt-dlp picks the downloader by the file stem and runs that path; it
    // already passes aria2c 16 connections in 1 MiB pieces. aria2c saves its control file, which
    // the worker reads for progress, each second instead of yt-dlp's 10 (the last value wins).
    if let Some(a) = &s.aria2c {
        o.insert("external_downloader".into(), json!({ "default": a }));
        let args: Vec<&str> = std::iter::once("--auto-save-interval=1")
            .chain(s.aria2c_args.iter().map(String::as_str))
            .collect();
        o.insert("external_downloader_args".into(), json!({ "aria2c": args }));
    }
    // the worker pops this key and merges it through `yt_dlp.parse_options` (worker.rs)
    if !s.extra_args.is_empty() {
        o.insert(EXTRA_ARGS_KEY.into(), json!(s.extra_args));
    }
    Value::Object(o)
}

/// yt-dlp options for a metadata probe.
pub fn probe_opts(p: &ProbeOpts, env: &Env) -> Value {
    let mut m = Map::new();
    common(&mut m, env, None, p.js_runtime.as_ref(), p.proxy.as_deref());
    if let Some(c) = &p.cookies {
        cookies(&mut m, c);
    }
    m.insert("skip_download".into(), json!(true));
    // a lookup only reads the info: `best*` is the best format with video, audio or both, so it
    // never fails the way `best` (a combined file, which YouTube Music lacks) or a video selector
    // does; the download picks the real formats later
    m.insert("format".into(), json!("best*"));
    m.insert("noplaylist".into(), json!(false));
    if p.flat_playlist {
        m.insert("extract_flat".into(), json!("in_playlist"));
    }
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{CookiesSettings, SubtitleSettings};

    fn base(preset: Preset) -> JobSpec {
        simple_job("https://x/y", preset, "D:/out")
    }

    #[test]
    fn video_height() {
        let o = to_opts(&base(Preset::Video(VideoQuality::P1080)), &Env::default());
        assert_eq!(o["format"], "bv*+ba/b");
        assert_eq!(o["format_sort"], json!(["res:1080"]));
        assert_eq!(o["merge_output_format"], "mp4/mkv");
        assert_eq!(o["outtmpl"], DEFAULT_OUTTMPL);
        assert_eq!(o["paths"]["home"], "D:/out");
        assert_eq!(o["continuedl"], true);
        assert_eq!(o["noplaylist"], true);
        assert!(o.get("postprocessors").is_none());
    }

    #[test]
    fn video_best_has_no_sort() {
        let o = to_opts(&base(Preset::DEFAULT), &Env::default());
        assert!(o.get("format_sort").is_none());
    }

    #[test]
    fn audio_mp3_and_best() {
        let o = to_opts(&base(Preset::Audio(AudioFormat::Mp3)), &Env::default());
        assert_eq!(o["format"], "ba/b");
        assert_eq!(
            o["postprocessors"][0],
            json!({"key":"FFmpegExtractAudio","preferredcodec":"mp3","preferredquality":0})
        );
        assert!(
            to_opts(&base(Preset::Audio(AudioFormat::Best)), &Env::default())
                .get("postprocessors")
                .is_none()
        );
    }

    #[test]
    fn custom_uses_format_spec() {
        let mut s = base(Preset::Custom);
        s.format_spec = "137+140".into();
        assert_eq!(to_opts(&s, &Env::default())["format"], "137+140");
    }

    #[test]
    fn tools_and_options() {
        let mut s = base(Preset::DEFAULT);
        s.ffmpeg_location = Some("C:/t/ffmpeg.exe".into());
        s.js_runtime = Some(("deno".into(), "C:/t/deno.exe".into()));
        s.playlist_index = Some(3);
        s.embed_thumbnail = true;
        s.embed_metadata = true;
        s.embed_chapters = true;
        s.sponsorblock.mode = SponsorBlockMode::Remove;
        s.cookies = CookiesSettings {
            mode: CookiesMode::Browser,
            file: None,
            browser: Some("firefox".into()),
            ..Default::default()
        };
        s.rate_limit = Some("1M".into());
        s.subtitles = SubtitleSettings {
            enabled: true,
            langs: vec!["en".into()],
            embed: true,
        };
        let env = Env {
            cache_dir: Some("C:/cache/yt-dlp".into()),
            js_runtime: None,
        };
        let o = to_opts(&s, &env);
        assert_eq!(o["js_runtimes"], json!({"deno": {"path": "C:/t/deno.exe"}}));
        assert_eq!(o["ffmpeg_location"], "C:/t/ffmpeg.exe");
        assert_eq!(o["cachedir"], "C:/cache/yt-dlp");
        assert_eq!(o["outtmpl"], PLAYLIST_OUTTMPL);
        assert_eq!(o["noplaylist"], false);
        assert_eq!(o["playlist_items"], "3");
        assert_eq!(o["writethumbnail"], true);
        assert_eq!(o["ratelimit"], 1_048_576);
        assert_eq!(o["cookiesfrombrowser"][0], "firefox");
        assert_eq!(o["subtitleslangs"], json!(["en"]));
        let keys: Vec<_> = o["postprocessors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["key"].as_str().unwrap())
            .collect();
        assert_eq!(
            keys,
            [
                "FFmpegEmbedSubtitle",
                "FFmpegMetadata",
                "EmbedThumbnail",
                "SponsorBlock",
                "ModifyChapters"
            ]
        );
        assert_eq!(
            o["postprocessors"][4]["remove_sponsor_segments"],
            json!(["sponsor"])
        );
        assert!(o.get("remote_components").is_none());
    }

    #[test]
    fn sponsorblock_mark_removes_nothing_and_cookie_file() {
        let mut s = base(Preset::DEFAULT);
        s.sponsorblock.mode = SponsorBlockMode::Mark;
        s.cookies = CookiesSettings {
            mode: CookiesMode::File,
            file: Some("C:/c.txt".into()),
            browser: None,
            ..Default::default()
        };
        let o = to_opts(&s, &Env::default());
        assert_eq!(o["postprocessors"][1]["remove_sponsor_segments"], json!([]));
        assert_eq!(o["cookiefile"], "C:/c.txt");
    }

    #[test]
    fn extra_args_split_and_carried() {
        let a = split_extra_args(
            r#"--sleep-interval 2 --user-agent "My Agent/1.0" --add-header 'X-A:b c'"#,
        )
        .unwrap();
        assert_eq!(
            a,
            [
                "--sleep-interval",
                "2",
                "--user-agent",
                "My Agent/1.0",
                "--add-header",
                "X-A:b c"
            ]
        );
        assert_eq!(split_extra_args("--x \"open"), None);
        let mut s = base(Preset::DEFAULT);
        assert!(to_opts(&s, &Env::default()).get(EXTRA_ARGS_KEY).is_none());
        s.extra_args = a.clone();
        assert_eq!(to_opts(&s, &Env::default())[EXTRA_ARGS_KEY], json!(a));
    }

    #[test]
    fn subtitles_file_only_proxy_retries_explicit_template() {
        let mut s = base(Preset::DEFAULT);
        s.subtitles = SubtitleSettings {
            enabled: true,
            langs: vec!["en".into(), "de".into()],
            embed: false,
        };
        s.proxy = Some("socks5://h:1".into());
        s.retries = Some(7);
        s.outtmpl = "%(id)s.%(ext)s".into();
        s.playlist_index = Some(2);
        let o = to_opts(&s, &Env::default());
        assert_eq!(o["writesubtitles"], true);
        assert_eq!(o["subtitleslangs"], json!(["en", "de"]));
        assert!(
            o.get("postprocessors").is_none(),
            "file only: nothing embedded"
        );
        assert_eq!(
            (o["proxy"].as_str(), o["retries"].as_u64()),
            (Some("socks5://h:1"), Some(7))
        );
        assert_eq!(o["outtmpl"], "%(id)s.%(ext)s");
        assert_eq!(
            (o["playlist_items"].as_str(), o["noplaylist"].as_bool()),
            (Some("2"), Some(false))
        );
        s.subtitles.enabled = false;
        assert!(to_opts(&s, &Env::default()).get("writesubtitles").is_none());
    }

    #[test]
    fn aria2c_only_when_set() {
        let mut s = base(Preset::DEFAULT);
        let o = to_opts(&s, &Env::default());
        assert!(o.get("external_downloader").is_none());
        assert!(o.get("external_downloader_args").is_none());
        s.aria2c = Some("C:/t/aria2c.exe".into());
        let o = to_opts(&s, &Env::default());
        assert_eq!(
            o["external_downloader"],
            json!({"default": "C:/t/aria2c.exe"})
        );
        assert_eq!(
            o["external_downloader_args"],
            json!({"aria2c": ["--auto-save-interval=1"]})
        );
        s.aria2c_args = vec!["--max-tries=5".into()];
        assert_eq!(
            to_opts(&s, &Env::default())["external_downloader_args"],
            json!({"aria2c": ["--auto-save-interval=1", "--max-tries=5"]})
        );
    }

    #[test]
    fn rate_parses() {
        assert_eq!(parse_rate("500K"), Some(512_000));
        assert_eq!(parse_rate("2.5m"), Some(2_621_440));
        assert_eq!(parse_rate("1000"), Some(1000));
        assert_eq!(parse_rate("fast"), None);
        assert_eq!(parse_rate("0"), None);
    }

    #[test]
    fn compatible_prefers_h264_and_aac() {
        let mut s = base(Preset::Video(VideoQuality::P1080));
        s.fix = Some(domain::Fix::Compatible);
        let o = to_opts(&s, &Env::default());
        assert_eq!(
            o["format_sort"],
            json!(["vcodec:h264", "acodec:m4a", "res:1080"])
        );
        assert_eq!(o["merge_output_format"], "mp4");
        s.preset = Preset::Audio(AudioFormat::Mp3);
        assert!(to_opts(&s, &Env::default()).get("format_sort").is_none());
    }

    #[test]
    fn probe_flat_and_proxy() {
        let o = probe_opts(
            &ProbeOpts {
                flat_playlist: true,
                proxy: Some("http://p:1".into()),
                ..Default::default()
            },
            &Env::default(),
        );
        assert_eq!(o["extract_flat"], "in_playlist");
        assert_eq!(o["skip_download"], true);
        assert_eq!(o["format"], "best*");
        assert_eq!(o["proxy"], "http://p:1");
        let o = probe_opts(
            &ProbeOpts {
                js_runtime: Some(("deno".into(), "/t/deno".into())),
                ..Default::default()
            },
            &Env::default(),
        );
        assert_eq!(o["js_runtimes"], json!({"deno": {"path": "/t/deno"}}));
    }
}
