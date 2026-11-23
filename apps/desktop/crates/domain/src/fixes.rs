//! The ffmpeg fixes as pure plans: what ffprobe says about a file, whether it is likely to play
//! everywhere (QuickTime, Photos, Windows, TVs), and the ffmpeg arguments of each [`Fix`].

use serde::Deserialize;

use crate::Fix;

/// What ffprobe reported about a file: its first video and audio codec.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Streams {
    pub video: Option<String>,
    pub audio: Option<String>,
}

/// Reads `ffprobe -print_format json -show_streams` output.
pub fn parse_ffprobe(json: &str) -> Option<Streams> {
    #[derive(Deserialize)]
    struct Out {
        #[serde(default)]
        streams: Vec<Stream>,
    }
    #[derive(Deserialize)]
    struct Stream {
        codec_type: Option<String>,
        codec_name: Option<String>,
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
            .and_then(|s| s.codec_name.clone())
    };
    Some(Streams {
        video: first("video"),
        audio: first("audio"),
    })
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

/// The ffmpeg run for `fix` on a file with extension `ext` and streams `st`. `h264`/`hevc` are
/// the encoders to use when video must be re-encoded (a hardware one or libx264/libx265);
/// `macos` picks ProRes for Edit-friendly.
pub fn plan(fix: Fix, ext: &str, st: &Streams, h264: &str, hevc: &str, macos: bool) -> Plan {
    let ext = ext.to_lowercase();
    let audio_only = st.video.is_none();
    let copy_audio_if = |ok: bool| {
        if ok {
            vec!["-c:a", "copy"]
        } else {
            vec!["-c:a", "aac", "-b:a", "192k"]
        }
    };
    match fix {
        Fix::Repair => Plan {
            input: strings(&["-fflags", "+genpts"]),
            output: strings(&["-map", "0", "-c", "copy"]),
            ext,
        },
        Fix::Normalize => {
            let mut out = vec!["-map", "0", "-c:v", "copy", "-c:s", "copy"];
            out.extend(["-af", "loudnorm=I=-14:TP=-1.5:LRA=11"]);
            out.extend(audio_for(&ext));
            Plan {
                input: vec![],
                output: strings(&out),
                ext,
            }
        }
        Fix::Compatible if audio_only => {
            let aac = st.audio.as_deref() == Some("aac");
            let mut out = vec!["-map", "0:a:0", "-vn"];
            out.extend(copy_audio_if(aac));
            Plan {
                input: vec![],
                output: strings(&out),
                ext: "m4a".into(),
            }
        }
        Fix::Compatible => {
            let mut out = vec!["-map", "0:v:0", "-map", "0:a:0?"];
            if st.video.as_deref() == Some("h264") {
                out.extend(["-c:v", "copy"]);
            } else {
                out.extend(["-c:v", h264, "-pix_fmt", "yuv420p"]);
                out.extend(quality(h264));
            }
            out.extend(copy_audio_if(st.audio.as_deref() == Some("aac")));
            out.extend(["-movflags", "+faststart"]);
            Plan {
                input: vec![],
                output: strings(&out),
                ext: "mp4".into(),
            }
        }
        Fix::EditFriendly if macos && !audio_only => Plan {
            input: vec![],
            output: strings(&[
                "-map",
                "0:v:0",
                "-map",
                "0:a:0?",
                "-c:v",
                "prores_videotoolbox",
                "-profile:v",
                "2",
                "-c:a",
                "pcm_s16le",
            ]),
            ext: "mov".into(),
        },
        Fix::EditFriendly | Fix::Smaller if audio_only => Plan {
            // nothing to re-encode: a plain copy keeps the file as it is
            input: vec![],
            output: strings(&["-map", "0", "-c", "copy"]),
            ext,
        },
        Fix::EditFriendly => {
            let mut out = vec![
                "-map", "0:v:0", "-map", "0:a:0?", "-c:v", h264, "-pix_fmt", "yuv420p",
            ];
            out.extend(quality(h264));
            // constant frame rate and a keyframe every second: editors scrub and cut cleanly
            out.extend([
                "-fps_mode",
                "cfr",
                "-force_key_frames",
                "expr:gte(t,n_forced*1)",
            ]);
            out.extend(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]);
            Plan {
                input: vec![],
                output: strings(&out),
                ext: "mp4".into(),
            }
        }
        Fix::Smaller => {
            let mut out = vec![
                "-map", "0:v:0", "-map", "0:a:0?", "-c:v", hevc, "-tag:v", "hvc1",
            ];
            out.extend(quality(hevc));
            out.extend(copy_audio_if(st.audio.as_deref() == Some("aac")));
            out.extend(["-movflags", "+faststart"]);
            Plan {
                input: vec![],
                output: strings(&out),
                ext: "mp4".into(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(v: Option<&str>, a: Option<&str>) -> Streams {
        Streams {
            video: v.map(String::from),
            audio: a.map(String::from),
        }
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
        );
        assert_eq!((p.input[1].as_str(), p.ext.as_str()), ("+genpts", "mp4"));
        let p = plan(
            Fix::Normalize,
            "mp3",
            &st(None, Some("mp3")),
            "libx264",
            "libx265",
            false,
        );
        assert!(p.output.contains(&"libmp3lame".to_string()) && p.ext == "mp3");
        let p = plan(
            Fix::EditFriendly,
            "mp4",
            &st(Some("av1"), Some("opus")),
            "libx264",
            "libx265",
            true,
        );
        assert_eq!(p.ext, "mov");
        let p = plan(
            Fix::Smaller,
            "mp4",
            &st(Some("h264"), Some("aac")),
            "libx264",
            "libx265",
            false,
        );
        assert!(
            p.output.contains(&"hvc1".to_string()) && p.output.contains(&"libx265".to_string())
        );
    }
}
