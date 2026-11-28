//! The ffmpeg fixes as pure plans: what ffprobe says about a file, whether it is likely to play
//! everywhere (QuickTime, Photos, Windows, TVs), and the ffmpeg arguments of each [`Fix`].

use serde::Deserialize;

use crate::{Fix, FixChain};

/// The hidden folder inside a download folder that holds partial files, and finished files
/// waiting for their fix.
pub const TEMP_NAME: &str = ".ytdlpd-tmp";

/// The folder a file in `dir` belongs in once it leaves the temp folder: `dir` without its
/// [`TEMP_NAME`] part. `None` when `dir` is not inside one.
pub fn out_of_temp(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut parts = dir.components();
    let at = parts.clone().position(|c| c.as_os_str() == TEMP_NAME)?;
    let mut home: std::path::PathBuf = parts.by_ref().take(at).collect();
    home.extend(parts.skip(1));
    Some(home)
}

/// What ffprobe reported about a file: its first video and audio codec, the video's height and
/// the bit rates (bit/s) it states.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Streams {
    pub video: Option<String>,
    pub audio: Option<String>,
    pub height: Option<i64>,
    /// The video stream's own bit rate.
    pub video_bps: Option<u64>,
    /// The audio stream's own bit rate.
    pub audio_bps: Option<u64>,
    /// The whole file's bit rate: the format's, else its size over its duration.
    pub format_bps: Option<u64>,
    /// How long the file plays, in microseconds.
    pub duration_us: Option<u64>,
}

/// Reads `ffprobe -print_format json -show_streams -show_format` output.
pub fn parse_ffprobe(json: &str) -> Option<Streams> {
    #[derive(Deserialize)]
    struct Out {
        #[serde(default)]
        streams: Vec<Stream>,
        #[serde(default)]
        format: Format,
    }
    /// ffprobe writes every number of the format as a string.
    #[derive(Deserialize, Default)]
    struct Format {
        bit_rate: Option<String>,
        duration: Option<String>,
        size: Option<String>,
    }
    #[derive(Deserialize)]
    struct Stream {
        codec_type: Option<String>,
        codec_name: Option<String>,
        height: Option<i64>,
        bit_rate: Option<String>,
        #[serde(default)]
        disposition: std::collections::HashMap<String, i64>,
    }
    let out: Out = serde_json::from_str(json).ok()?;
    let first = |kind: &str| {
        out.streams
            .iter()
            // cover art is a "video" stream with attached_pic set
            .filter(|s| s.disposition.get("attached_pic").copied().unwrap_or(0) == 0)
            .find(|s| s.codec_type.as_deref() == Some(kind))
    };
    let bps = |s: Option<&Stream>| s.and_then(|s| s.bit_rate.as_deref()?.parse::<u64>().ok());
    let number = |v: &Option<String>| v.as_deref().and_then(|v| v.parse::<f64>().ok());
    let format_bps = out
        .format
        .bit_rate
        .as_deref()
        .and_then(|v| v.parse::<u64>().ok())
        .or_else(|| {
            let (size, secs) = (number(&out.format.size)?, number(&out.format.duration)?);
            (secs > 0.).then(|| (size * 8. / secs) as u64)
        });
    Some(Streams {
        video: first("video").and_then(|s| s.codec_name.clone()),
        audio: first("audio").and_then(|s| s.codec_name.clone()),
        height: first("video").and_then(|s| s.height),
        video_bps: bps(first("video")),
        audio_bps: bps(first("audio")),
        format_bps,
        duration_us: number(&out.format.duration).map(|s| (s * 1e6) as u64),
    })
}

/// How far ffmpeg is, from one line of its `-progress` output: the share of `total_us` done.
pub fn progress_share(line: &str, total_us: u64) -> Option<f32> {
    // out_time_ms is in microseconds too (an old ffmpeg naming slip)
    let (key, value) = line.trim().split_once('=')?;
    if total_us == 0 || !matches!(key, "out_time_us" | "out_time_ms") {
        return None;
    }
    let at = value.parse::<u64>().ok()?;
    Some((at as f64 / total_us as f64).clamp(0., 1.) as f32)
}

/// What a finished file holds, for the format column: "1080p · HEVC · AAC · MP4".
pub fn describe(ext: &str, st: &Streams) -> String {
    let codec = |c: &str| match c {
        "h264" => "H.264".to_string(),
        "hevc" => "HEVC".to_string(),
        other => label(other),
    };
    st.height
        .filter(|_| st.video.is_some())
        .map(|h| format!("{h}p"))
        .into_iter()
        .chain(st.video.as_deref().map(codec))
        .chain(st.audio.as_deref().map(codec))
        .chain((!ext.is_empty()).then(|| ext.to_uppercase()))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Why a file may not play in the system players; `None` when it should play everywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unplayable {
    /// The codecs or container that are the trouble, for the hint ("AV1 video, Opus audio").
    pub parts: Vec<String>,
}

fn label(codec: &str) -> String {
    match codec {
        "av1" => "AV1".into(),
        "vp9" => "VP9".into(),
        "vp8" => "VP8".into(),
        "opus" => "Opus".into(),
        "vorbis" => "Vorbis".into(),
        "flac" => "FLAC".into(),
        other => other.to_uppercase(),
    }
}

/// Checks `ext` (without the dot) and `st` against what plays everywhere: H.264/HEVC video and
/// AAC/MP3/ALAC audio in MP4, MOV or M4A (or plain MP3).
pub fn unplayable(ext: &str, st: &Streams) -> Option<Unplayable> {
    let ext = ext.to_lowercase();
    let mut parts = vec![];
    let video_ok = |c: &str| matches!(c, "h264" | "hevc");
    let audio_ok = |c: &str| matches!(c, "aac" | "mp3" | "alac");
    if let Some(v) = &st.video
        && !video_ok(v)
    {
        parts.push(format!("{} video", label(v)));
    }
    if let Some(a) = &st.audio
        && !audio_ok(a)
    {
        parts.push(format!("{} audio", label(a)));
    }
    if !matches!(ext.as_str(), "mp4" | "m4v" | "mov" | "m4a" | "mp3") {
        parts.push(format!(".{ext}"));
    }
    (!parts.is_empty()).then_some(Unplayable { parts })
}

/// The hardware encoder for `codec` (`h264` or `hevc`) in `ffmpeg -encoders` output, if any.
pub fn hw_encoder(encoders: &str, codec: &str) -> Option<String> {
    ["videotoolbox", "nvenc", "qsv", "amf"]
        .iter()
        .map(|kind| format!("{codec}_{kind}"))
        .find(|name| {
            encoders
                .lines()
                .any(|l| l.split_whitespace().nth(1) == Some(name.as_str()))
        })
}

/// Quality arguments for an encoder: about "visually lossless" for H.264, "good" for HEVC.
fn quality(encoder: &str) -> Vec<&'static str> {
    match encoder {
        "libx264" => vec!["-crf", "20", "-preset", "medium"],
        "libx265" => vec!["-crf", "26", "-preset", "medium"],
        "h264_videotoolbox" => vec!["-q:v", "65"],
        "hevc_videotoolbox" => vec!["-q:v", "55"],
        "h264_nvenc" => vec!["-cq", "21", "-preset", "p5"],
        "hevc_nvenc" => vec!["-cq", "28", "-preset", "p5"],
        "h264_qsv" => vec!["-global_quality", "21"],
        "hevc_qsv" => vec!["-global_quality", "28"],
        "h264_amf" => vec!["-rc", "cqp", "-qp_i", "21", "-qp_p", "21"],
        "hevc_amf" => vec!["-rc", "cqp", "-qp_i", "28", "-qp_p", "28"],
        _ => vec![],
    }
}

/// The Smaller file fix's two choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmallerOpts {
    /// 100 keeps the quality of the download, lower gives a smaller file (50 to 100).
    pub quality: u8,
    /// Videos taller than this are scaled down to it; 0 keeps the size.
    pub max_height: u32,
}

/// The lowest quality percent Smaller file accepts.
pub const MIN_QUALITY: u8 = 50;

/// The lowest video bit rate Smaller file aims for (bit/s).
const MIN_BPS: u64 = 150_000;

/// `quality` kept inside 50 to 100.
pub fn clamp_quality(quality: u8) -> u8 {
    quality.clamp(MIN_QUALITY, 100)
}

impl SmallerOpts {
    /// The choices of a download: its site rule's where it set them, else `s`'s.
    pub fn resolve(s: &crate::Settings, quality: Option<u8>, height: Option<u32>) -> Self {
        Self {
            quality: clamp_quality(quality.unwrap_or(s.smaller_quality)),
            max_height: height.unwrap_or(s.smaller_height),
        }
    }
}

/// What HEVC needs for the picture of a `codec` source, as a share of the source's bit rate.
fn codec_factor(codec: &str) -> f64 {
    match codec {
        "h264" | "vp8" => 0.6,
        "vp9" | "hevc" => 1.0,
        "av1" => 1.3,
        _ => 0.8,
    }
}

/// The height `st` is scaled to, when it is taller than the largest size.
pub fn scaled_height(st: &Streams, opts: SmallerOpts) -> Option<i64> {
    let max = i64::from(opts.max_height);
    st.height.filter(|h| max > 0 && *h > max).map(|_| max)
}

/// The video bit rate (bit/s) of the source: the stream's, else the file's less its audio.
pub fn source_bps(st: &Streams) -> Option<u64> {
    st.video_bps
        .or_else(|| {
            let audio = st.audio_bps.unwrap_or(128_000);
            st.format_bps.map(|f| f.saturating_sub(audio))
        })
        .filter(|b| *b > 0)
}

/// The video bit rate (bit/s) Smaller file aims for: the source's, times what HEVC needs for
/// its codec, the quality and the share of pixels kept; never above the source, never below a floor.
pub fn smaller_bitrate(st: &Streams, opts: SmallerOpts) -> Option<u64> {
    let source = source_bps(st)?;
    let factor = codec_factor(st.video.as_deref().unwrap_or(""));
    let share = match (scaled_height(st, opts), st.height) {
        (Some(to), Some(from)) => (to as f64 / from as f64).powi(2),
        _ => 1.,
    };
    let target = source as f64 * factor * f64::from(clamp_quality(opts.quality)) / 100. * share;
    Some((target.round() as u64).max(MIN_BPS).min(source))
}

/// Whether Smaller file would re-encode the video for (almost) nothing: no scaling, and a
/// target within 5% of the source's bit rate (HEVC at 100%, AV1 at about 77% and up). The video
/// is then kept as it is. `false` when the source's bit rate is unknown.
pub fn smaller_gains_nothing(st: &Streams, opts: SmallerOpts) -> bool {
    match (
        st.video.is_some(),
        scaled_height(st, opts),
        source_bps(st),
        smaller_bitrate(st, opts),
    ) {
        (true, None, Some(source), Some(target)) => target * 100 >= source * 95,
        _ => false,
    }
}

/// Encoder arguments for HEVC at video bit rate `bps`: maxrate 1.5 times it, buffer twice.
fn hevc_rate(encoder: &str, bps: u64) -> Vec<String> {
    let k = |n: u64| format!("{}k", n / 1000);
    let (b, max, buf) = (k(bps), k(bps * 3 / 2), k(bps * 2));
    let words = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let mut out = match encoder {
        "hevc_nvenc" => words(&["-rc", "vbr"]),
        "hevc_amf" => words(&["-rc", "vbr_peak"]),
        _ => vec![],
    };
    out.extend(words(&["-b:v"]));
    out.extend([b, "-maxrate".into(), max]);
    if !matches!(encoder, "hevc_amf" | "hevc_videotoolbox") {
        out.extend(["-bufsize".into(), buf]);
    }
    match encoder {
        "hevc_nvenc" => out.extend(words(&["-preset", "p5"])),
        "libx265" => out.extend(words(&["-preset", "medium"])),
        _ => {}
    }
    out
}

/// Constant-quality HEVC arguments for when ffprobe gave no bit rate: the level is nudged
/// worse by (100 - quality) / 5.
fn hevc_constant(encoder: &str, quality: u8) -> Vec<String> {
    let bump = i32::from(100 - clamp_quality(quality)) / 5;
    let mut out: Vec<String> = self::quality(encoder)
        .iter()
        .map(|s| s.to_string())
        .collect();
    // videotoolbox's -q:v grows with quality, the others shrink
    let sign = if encoder == "hevc_videotoolbox" {
        -1
    } else {
        1
    };
    for at in 1..out.len() {
        if matches!(
            out[at - 1].as_str(),
            "-crf" | "-cq" | "-global_quality" | "-qp_i" | "-qp_p" | "-q:v"
        ) && let Ok(n) = out[at].parse::<i32>()
        {
            out[at] = (n + sign * bump).max(1).to_string();
        }
    }
    out
}

/// One ffmpeg run: arguments before `-i <file>`, arguments after it, and the result's extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub input: Vec<String>,
    pub output: Vec<String>,
    pub ext: String,
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The audio encoder that keeps `ext` valid.
fn audio_for(ext: &str) -> Vec<&'static str> {
    match ext {
        "mp3" => vec!["-c:a", "libmp3lame", "-q:a", "2"],
        "opus" | "webm" | "ogg" => vec!["-c:a", "libopus", "-b:a", "160k"],
        _ => vec!["-c:a", "aac", "-b:a", "192k"],
    }
}

/// The ffmpeg run for the single `fix`; see [`plan_chain`].
pub fn plan(
    fix: Fix,
    ext: &str,
    st: &Streams,
    h264: &str,
    hevc: &str,
    macos: bool,
    smaller: SmallerOpts,
) -> Plan {
    plan_chain(fix.into(), ext, st, h264, hevc, macos, smaller)
}

/// The one ffmpeg run for `chain` on a file with extension `ext` and streams `st`. `h264`/`hevc`
/// are the encoders to use when video must be re-encoded (a hardware one or libx264/libx265);
/// `macos` picks ProRes for Edit-friendly; `smaller` only matters to [`Fix::Smaller`].
pub fn plan_chain(
    chain: FixChain,
    ext: &str,
    st: &Streams,
    h264: &str,
    hevc: &str,
    macos: bool,
    smaller: SmallerOpts,
) -> Plan {
    let ext = ext.to_lowercase();
    let audio_only = st.video.is_none();
    let aac = st.audio.as_deref() == Some("aac");
    let input = match chain.repair {
        true => strings(&["-fflags", "+genpts"]),
        false => vec![],
    };
    let copy_audio_if = |ok: bool| {
        if ok {
            vec!["-c:a", "copy"]
        } else {
            vec!["-c:a", "aac", "-b:a", "192k"]
        }
    };
    // the video kind that applies: audio-only files only take Compatible (to m4a)
    let video = chain.video.filter(|f| !audio_only || *f == Fix::Compatible);
    let prores = macos && video == Some(Fix::EditFriendly);
    let mut out: Vec<&str> = vec![];
    let scale;
    let mut tail: Vec<String> = vec![];
    let mut ext = ext;
    match video {
        None if chain.normalize => out.extend(["-map", "0", "-c:v", "copy", "-c:s", "copy"]),
        None => out.extend(["-map", "0", "-c", "copy"]),
        Some(Fix::Compatible) if audio_only => {
            out.extend(["-map", "0:a:0", "-vn"]);
            ext = "m4a".into();
        }
        Some(Fix::Compatible) => {
            out.extend(["-map", "0:v:0", "-map", "0:a:0?"]);
            if st.video.as_deref() == Some("h264") {
                out.extend(["-c:v", "copy"]);
            } else {
                out.extend(["-c:v", h264, "-pix_fmt", "yuv420p"]);
                out.extend(quality(h264));
            }
            ext = "mp4".into();
        }
        Some(Fix::EditFriendly) if prores => {
            out.extend(["-map", "0:v:0", "-map", "0:a:0?"]);
            out.extend(["-c:v", "prores_videotoolbox", "-profile:v", "2"]);
            ext = "mov".into();
        }
        Some(Fix::EditFriendly) => {
            out.extend(["-map", "0:v:0", "-map", "0:a:0?", "-c:v", h264]);
            out.extend(["-pix_fmt", "yuv420p"]);
            out.extend(quality(h264));
            // constant frame rate and a keyframe every second: editors scrub and cut cleanly
            out.extend([
                "-fps_mode",
                "cfr",
                "-force_key_frames",
                "expr:gte(t,n_forced*1)",
            ]);
            ext = "mp4".into();
        }
        Some(_) => {
            out.extend(["-map", "0:v:0", "-map", "0:a:0?"]);
            if let Some(h) = scaled_height(st, smaller) {
                // -2 keeps the width even, which HEVC needs
                scale = format!("scale=-2:{h}");
                out.extend(["-vf", scale.as_str()]);
            }
            out.extend(["-c:v", hevc, "-tag:v", "hvc1"]);
            tail = match smaller_bitrate(st, smaller) {
                Some(bps) => hevc_rate(hevc, bps),
                None => hevc_constant(hevc, smaller.quality),
            };
            ext = "mp4".into();
        }
    }
    let mut out = strings(&out);
    out.extend(tail);
    let container_mp4 = matches!(ext.as_str(), "mp4" | "m4a" | "m4v");
    if chain.normalize {
        out.extend(strings(&["-af", "loudnorm=I=-14:TP=-1.5:LRA=11"]));
        out.extend(strings(&match (prores, container_mp4) {
            (true, _) => vec!["-c:a", "pcm_s16le"],
            (_, true) => vec!["-c:a", "aac", "-b:a", "192k"],
            _ => audio_for(&ext),
        }));
    } else {
        match video {
            // the plain copy has `-c copy` already
            None => {}
            Some(Fix::EditFriendly) if prores => out.extend(strings(&["-c:a", "pcm_s16le"])),
            Some(Fix::EditFriendly) => out.extend(strings(&["-c:a", "aac", "-b:a", "192k"])),
            Some(_) => out.extend(strings(&copy_audio_if(aac))),
        }
    }
    if video.is_some() && ext == "mp4" {
        out.extend(strings(&["-movflags", "+faststart"]));
    }
    Plan {
        input,
        output: out,
        ext,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(v: Option<&str>, a: Option<&str>) -> Streams {
        Streams {
            video: v.map(String::from),
            audio: a.map(String::from),
            ..Default::default()
        }
    }

    const OPTS: SmallerOpts = SmallerOpts {
        quality: 80,
        max_height: 0,
    };

    fn video(codec: &str, height: i64, bps: Option<u64>) -> Streams {
        Streams {
            height: Some(height),
            video_bps: bps,
            ..st(Some(codec), Some("aac"))
        }
    }

    #[test]
    fn smaller_keeps_a_video_it_cannot_shrink() {
        let at = |quality, max_height| SmallerOpts {
            quality,
            max_height,
        };
        let h264 = video("h264", 720, Some(3_000_000));
        let hevc = video("hevc", 720, Some(2_000_000));
        let av1 = video("av1", 720, Some(2_000_000));
        assert!(
            !smaller_gains_nothing(&h264, at(80, 720)),
            "H.264 to HEVC saves"
        );
        assert!(
            !smaller_gains_nothing(&hevc, at(80, 720)),
            "80% of HEVC saves 20%"
        );
        assert!(smaller_gains_nothing(&hevc, at(100, 720)));
        assert!(
            smaller_gains_nothing(&av1, at(80, 720)),
            "AV1 at 80% is capped at the source"
        );
        assert!(!smaller_gains_nothing(&av1, at(60, 720)));
        assert!(
            !smaller_gains_nothing(&hevc, at(100, 480)),
            "scaling down always runs"
        );
        assert!(
            !smaller_gains_nothing(&video("hevc", 720, None), at(100, 0)),
            "unknown rate"
        );
        assert!(!smaller_gains_nothing(&st(None, Some("aac")), at(100, 0)));
    }

    #[test]
    fn leaves_the_temp_folder() {
        use std::path::Path;
        assert_eq!(
            out_of_temp(Path::new("D:/out/.ytdlpd-tmp/Uploader")),
            Some(Path::new("D:/out/Uploader").to_path_buf())
        );
        assert_eq!(
            out_of_temp(Path::new("D:/out/.ytdlpd-tmp")),
            Some(Path::new("D:/out").to_path_buf())
        );
        assert_eq!(out_of_temp(Path::new("D:/out/Uploader")), None);
    }

    #[test]
    fn reads_ffmpeg_progress() {
        assert_eq!(progress_share("out_time_us=5000000", 10_000_000), Some(0.5));
        assert_eq!(
            progress_share(
                "out_time_ms=20000000
",
                10_000_000
            ),
            Some(1.0)
        );
        assert_eq!(progress_share("out_time_us=N/A", 10_000_000), None);
        assert_eq!(progress_share("frame=12", 10_000_000), None);
        assert_eq!(progress_share("out_time_us=5", 0), None);
    }

    fn arg(p: &Plan, flag: &str) -> Option<String> {
        p.output
            .windows(2)
            .find(|w| w[0] == flag)
            .map(|w| w[1].clone())
    }

    fn smaller(st: &Streams, quality: u8, max_height: u32) -> Plan {
        plan(
            Fix::Smaller,
            "mp4",
            st,
            "libx264",
            "libx265",
            false,
            SmallerOpts {
                quality,
                max_height,
            },
        )
    }

    fn bps(st: &Streams, quality: u8, max_height: u32) -> u64 {
        smaller_bitrate(
            st,
            SmallerOpts {
                quality,
                max_height,
            },
        )
        .unwrap()
    }

    #[test]
    fn bitrate_follows_the_source_codec() {
        // each codec factor at 100%: h264 and vp8 0.6, vp9 and hevc 1, av1 capped at the source, other 0.8
        for (codec, want) in [
            ("h264", 6_000_000),
            ("vp8", 6_000_000),
            ("vp9", 10_000_000),
            ("hevc", 10_000_000),
            ("av1", 10_000_000),
            ("mpeg4", 8_000_000),
        ] {
            assert_eq!(
                bps(&video(codec, 1080, Some(10_000_000)), 100, 0),
                want,
                "{codec}"
            );
        }
        // av1's 1.3 shows under the cap only when the percent is low enough
        assert_eq!(bps(&video("av1", 1080, Some(10_000_000)), 50, 0), 6_500_000);
        assert_eq!(bps(&video("vp9", 1080, Some(5_000_000)), 80, 0), 4_000_000);
    }

    #[test]
    fn bitrate_has_a_floor_and_a_cap() {
        assert_eq!(bps(&video("h264", 1080, Some(200_000)), 50, 0), 150_000);
        // a source below the floor is never made larger
        assert_eq!(bps(&video("h264", 1080, Some(100_000)), 50, 0), 100_000);
        // the quality is clamped to 50..=100
        assert_eq!(bps(&video("vp9", 1080, Some(4_000_000)), 10, 0), 2_000_000);
        assert_eq!(bps(&video("vp9", 1080, Some(4_000_000)), 250, 0), 4_000_000);
    }

    #[test]
    fn scaling_down_shares_the_pixels() {
        let s = video("vp9", 2160, Some(8_000_000));
        // 1080 of 2160 keeps a quarter of the pixels
        assert_eq!(bps(&s, 100, 1080), 2_000_000);
        let p = smaller(&s, 100, 1080);
        assert_eq!(arg(&p, "-vf").as_deref(), Some("scale=-2:1080"));
        // never upscaled, nothing to scale
        for max in [0, 2160, 4320] {
            assert_eq!(arg(&smaller(&s, 100, max), "-vf"), None, "{max}");
            assert_eq!(bps(&s, 100, max), 8_000_000);
        }
    }

    #[test]
    fn bitrate_comes_from_the_stream_else_the_format() {
        let mut s = video("vp9", 720, None);
        s.format_bps = Some(1_128_000);
        s.audio_bps = Some(128_000);
        assert_eq!(source_bps(&s), Some(1_000_000));
        s.audio_bps = None;
        assert_eq!(source_bps(&s), Some(1_000_000));
        s.video_bps = Some(900_000);
        assert_eq!(source_bps(&s), Some(900_000));
        assert_eq!(source_bps(&Streams::default()), None);
    }

    #[test]
    fn encoders_get_their_rate_arguments() {
        let s = video("vp9", 1080, Some(4_000_000));
        let has = |enc: &str, want: &[&str]| {
            let p = plan(Fix::Smaller, "mp4", &s, "libx264", enc, false, OPTS);
            let hit = p
                .output
                .windows(want.len())
                .any(|w| w.iter().zip(want).all(|(a, b)| a == b));
            assert!(hit, "{enc}: {:?}", p.output);
            assert!(p.output.windows(2).any(|w| w == ["-tag:v", "hvc1"]));
        };
        // 4000k x 0.8 = 3200k, maxrate 4800k, bufsize 6400k
        has(
            "libx265",
            &[
                "-b:v", "3200k", "-maxrate", "4800k", "-bufsize", "6400k", "-preset", "medium",
            ],
        );
        has(
            "hevc_nvenc",
            &[
                "-rc", "vbr", "-b:v", "3200k", "-maxrate", "4800k", "-bufsize", "6400k", "-preset",
                "p5",
            ],
        );
        has(
            "hevc_qsv",
            &["-b:v", "3200k", "-maxrate", "4800k", "-bufsize", "6400k"],
        );
        has(
            "hevc_amf",
            &["-rc", "vbr_peak", "-b:v", "3200k", "-maxrate", "4800k"],
        );
        has("hevc_videotoolbox", &["-b:v", "3200k", "-maxrate", "4800k"]);
    }

    #[test]
    fn no_bitrate_falls_back_to_constant_quality() {
        let s = video("av1", 1080, None);
        let p = smaller(&s, 100, 0);
        assert_eq!(arg(&p, "-crf").as_deref(), Some("26"));
        assert!(arg(&p, "-b:v").is_none());
        // 50% is 10 worse
        assert_eq!(arg(&smaller(&s, 50, 0), "-crf").as_deref(), Some("36"));
    }

    #[test]
    fn reads_bit_rates_from_ffprobe() {
        let json = r#"{"streams":[{"codec_type":"video","codec_name":"av1","height":2160,"bit_rate":"4100000"},
            {"codec_type":"audio","codec_name":"opus","bit_rate":"130000"}],
            "format":{"bit_rate":"4300000","duration":"10.0","size":"5375000"}}"#;
        let got = parse_ffprobe(json).unwrap();
        assert_eq!(
            (got.video_bps, got.audio_bps, got.format_bps),
            (Some(4_100_000), Some(130_000), Some(4_300_000))
        );
        // no bit rates at all: the file's size over its duration
        let json = r#"{"streams":[{"codec_type":"video","codec_name":"vp9"}],
            "format":{"duration":"8.0","size":"1000000"}}"#;
        let got = parse_ffprobe(json).unwrap();
        assert_eq!((got.video_bps, got.format_bps), (None, Some(1_000_000)));
    }

    #[test]
    fn describes_what_the_file_holds() {
        let mut s = st(Some("hevc"), Some("aac"));
        s.height = Some(1080);
        assert_eq!(describe("mp4", &s), "1080p · HEVC · AAC · MP4");
        assert_eq!(describe("m4a", &st(None, Some("opus"))), "Opus · M4A");
        assert_eq!(describe("webm", &st(Some("av1"), None)), "AV1 · WEBM");
    }

    #[test]
    fn reads_ffprobe_and_skips_cover_art() {
        let json = r#"{"streams":[{"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}},
            {"codec_type":"audio","codec_name":"opus"},{"codec_type":"video","codec_name":"av1"}]}"#;
        assert_eq!(parse_ffprobe(json), Some(st(Some("av1"), Some("opus"))));
        assert_eq!(parse_ffprobe("nope"), None);
    }

    #[test]
    fn what_plays_everywhere() {
        assert!(unplayable("mp4", &st(Some("h264"), Some("aac"))).is_none());
        assert!(unplayable("mp3", &st(None, Some("mp3"))).is_none());
        let bad = unplayable("mp4", &st(Some("av1"), Some("opus"))).unwrap();
        assert_eq!(bad.parts, ["AV1 video", "Opus audio"]);
        assert_eq!(
            unplayable("webm", &st(Some("vp9"), Some("opus")))
                .unwrap()
                .parts
                .len(),
            3
        );
    }

    #[test]
    fn hardware_encoders_are_found_by_name() {
        let list = " V....D libx264  x\n V....D h264_videotoolbox  VT\n V....D hevc_nvenc  NV\n";
        assert_eq!(
            hw_encoder(list, "h264").as_deref(),
            Some("h264_videotoolbox")
        );
        assert_eq!(hw_encoder(list, "hevc").as_deref(), Some("hevc_nvenc"));
        assert_eq!(hw_encoder(" V....D libx264 x\n", "h264"), None);
    }

    #[test]
    fn compatible_copies_what_already_plays() {
        let p = plan(
            Fix::Compatible,
            "mp4",
            &st(Some("h264"), Some("opus")),
            "libx264",
            "libx265",
            false,
            OPTS,
        );
        assert!(p.output.windows(2).any(|w| w == ["-c:v", "copy"]));
        assert!(p.output.windows(2).any(|w| w == ["-c:a", "aac"]));
        assert_eq!(p.ext, "mp4");
        let p = plan(
            Fix::Compatible,
            "webm",
            &st(Some("vp9"), Some("aac")),
            "h264_videotoolbox",
            "x",
            false,
            OPTS,
        );
        assert!(
            p.output
                .windows(2)
                .any(|w| w == ["-c:v", "h264_videotoolbox"])
        );
        assert!(p.output.windows(2).any(|w| w == ["-c:a", "copy"]));
        let p = plan(
            Fix::Compatible,
            "opus",
            &st(None, Some("opus")),
            "libx264",
            "libx265",
            false,
            OPTS,
        );
        assert_eq!(p.ext, "m4a");
    }

    #[test]
    fn other_fixes() {
        let p = plan(
            Fix::Repair,
            "mp4",
            &st(Some("h264"), Some("aac")),
            "libx264",
            "libx265",
            false,
            OPTS,
        );
        assert_eq!((p.input[1].as_str(), p.ext.as_str()), ("+genpts", "mp4"));
        let p = plan(
            Fix::Normalize,
            "mp3",
            &st(None, Some("mp3")),
            "libx264",
            "libx265",
            false,
            OPTS,
        );
        assert!(p.output.contains(&"libmp3lame".to_string()) && p.ext == "mp3");
        let p = plan(
            Fix::EditFriendly,
            "mp4",
            &st(Some("av1"), Some("opus")),
            "libx264",
            "libx265",
            true,
            OPTS,
        );
        assert_eq!(p.ext, "mov");
        let p = plan(
            Fix::Smaller,
            "mp4",
            &st(Some("h264"), Some("aac")),
            "libx264",
            "libx265",
            false,
            OPTS,
        );
        assert!(
            p.output.contains(&"hvc1".to_string()) && p.output.contains(&"libx265".to_string())
        );
    }

    fn chain(video: Option<Fix>, normalize: bool, repair: bool) -> FixChain {
        FixChain {
            video,
            normalize,
            repair,
        }
    }

    fn run_chain(c: FixChain, ext: &str, s: &Streams, macos: bool) -> Plan {
        plan_chain(c, ext, s, "libx264", "libx265", macos, OPTS)
    }

    fn has(p: &Plan, want: &[&str]) -> bool {
        p.output.windows(want.len()).any(|w| w == want)
    }

    #[test]
    fn a_single_fix_is_its_own_chain() {
        let s = st(Some("vp9"), Some("opus"));
        for fix in Fix::ALL {
            let one = plan(fix, "webm", &s, "libx264", "libx265", false, OPTS);
            let c = run_chain(fix.into(), "webm", &s, false);
            assert_eq!(one, c, "{fix:?}");
        }
        // Normalize on its own keeps the video and the container
        let p = run_chain(Fix::Normalize.into(), "webm", &s, false);
        assert!(has(&p, &["-c:v", "copy", "-c:s", "copy"]) && has(&p, &["-c:a", "libopus"]));
        assert_eq!(p.ext, "webm");
    }

    #[test]
    fn smaller_normalize_repair_is_one_pass() {
        let s = video("vp9", 1080, Some(4_000_000));
        let p = run_chain(chain(Some(Fix::Smaller), true, true), "webm", &s, false);
        assert_eq!(p.input, ["-fflags", "+genpts"]);
        assert!(has(&p, &["-c:v", "libx265", "-tag:v", "hvc1"]));
        assert!(has(
            &p,
            &["-af", "loudnorm=I=-14:TP=-1.5:LRA=11", "-c:a", "aac"]
        ));
        assert!(!has(&p, &["-c:a", "copy"]));
        assert_eq!(p.output.last().map(String::as_str), Some("+faststart"));
        assert_eq!(p.ext, "mp4");
    }

    #[test]
    fn keep_video_with_normalize_or_repair() {
        let s = st(Some("av1"), Some("opus"));
        let p = run_chain(chain(None, true, false), "mkv", &s, false);
        assert!(p.input.is_empty() && has(&p, &["-map", "0", "-c:v", "copy", "-c:s", "copy"]));
        assert!(has(&p, &["-af", "loudnorm=I=-14:TP=-1.5:LRA=11"]) && p.ext == "mkv");
        let p = run_chain(chain(None, false, true), "mkv", &s, false);
        assert_eq!(p.input, ["-fflags", "+genpts"]);
        assert_eq!(p.output, ["-map", "0", "-c", "copy"]);
    }

    #[test]
    fn audio_only_ignores_the_video_pick() {
        let s = st(None, Some("opus"));
        let p = run_chain(chain(Some(Fix::Compatible), true, false), "opus", &s, false);
        assert_eq!(p.ext, "m4a");
        assert!(
            has(&p, &["-vn"]) && has(&p, &["-af", "loudnorm=I=-14:TP=-1.5:LRA=11", "-c:a", "aac"])
        );
        let p = run_chain(chain(Some(Fix::Smaller), true, true), "mp3", &s, false);
        assert_eq!((p.ext.as_str(), p.input.len()), ("mp3", 2));
        assert!(has(&p, &["-c:a", "libmp3lame"]) && !has(&p, &["-c:v", "libx265"]));
    }

    #[test]
    fn edit_friendly_on_macos_normalizes_to_pcm() {
        let s = st(Some("av1"), Some("opus"));
        let p = run_chain(chain(Some(Fix::EditFriendly), true, false), "mp4", &s, true);
        assert_eq!(p.ext, "mov");
        assert!(has(&p, &["-c:v", "prores_videotoolbox"]));
        assert!(has(
            &p,
            &["-af", "loudnorm=I=-14:TP=-1.5:LRA=11", "-c:a", "pcm_s16le"]
        ));
    }

    #[test]
    fn fixes_list_follows_the_chain() {
        assert_eq!(
            chain(Some(Fix::Smaller), true, true).fixes(),
            [Fix::Smaller, Fix::Normalize, Fix::Repair]
        );
        assert!(FixChain::default().is_empty() && FixChain::from(Fix::Repair).repair);
    }
}
