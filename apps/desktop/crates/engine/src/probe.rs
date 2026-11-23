//! In-process metadata probe on the warm interpreter, with a timeout. A stuck probe is abandoned, not killed.
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use domain::{EngineError, ErrorCode, FormatInfo, PlaylistEntry, ProbeResult};
use pyo3::{prelude::*, types::PyDict};
use serde_json::Value;

use crate::{
    classify::classify,
    runtime::{self, YdlLogger, classify_pyerr, json_to_py, py_to_json},
};

static ABANDONED: AtomicUsize = AtomicUsize::new(0);

/// How many probes hit their timeout and were left running.
pub fn abandoned_probes() -> usize {
    ABANDONED.load(Ordering::Relaxed)
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}
fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

/// Keys dropped from the info dict stored in `download_meta` (size, not needed later).
const TRIM: [&str; 5] = [
    "heatmap",
    "thumbnails",
    "automatic_captions",
    "subtitles",
    "entries",
];

/// Build the typed result from a sanitized yt-dlp info dict. Tolerant: missing or oddly typed
/// fields become `None`. `url` is what the user typed.
pub fn from_info(url: &str, v: &Value) -> ProbeResult {
    let formats = v
        .get("formats")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|x| FormatInfo {
                    id: s(x, "format_id").unwrap_or_default(),
                    ext: s(x, "ext"),
                    width: f(x, "width").map(|n| n as u32),
                    height: f(x, "height").map(|n| n as u32),
                    fps: f(x, "fps"),
                    vcodec: s(x, "vcodec"),
                    acodec: s(x, "acodec"),
                    abr: f(x, "abr"),
                    tbr: f(x, "tbr"),
                    filesize: f(x, "filesize")
                        .or_else(|| f(x, "filesize_approx"))
                        .map(|n| n as u64),
                    note: s(x, "format_note"),
                    protocol: s(x, "protocol"),
                })
                .collect()
        })
        .unwrap_or_default();
    let entries = (v.get("_type").and_then(Value::as_str) == Some("playlist")).then(|| {
        v.get("entries")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|x| x.is_object())
            .enumerate()
            .map(|(i, x)| PlaylistEntry {
                index: x
                    .get("playlist_index")
                    .and_then(Value::as_u64)
                    .map_or(i as u32 + 1, |n| n as u32),
                id: s(x, "id"),
                url: s(x, "url")
                    .or_else(|| s(x, "webpage_url"))
                    .unwrap_or_default(),
                title: s(x, "title"),
                duration_s: f(x, "duration"),
                thumbnail_url: s(x, "thumbnail").or_else(|| {
                    x.get("thumbnails")?
                        .as_array()?
                        .last()?
                        .get("url")?
                        .as_str()
                        .map(str::to_string)
                }),
            })
            .collect()
    });
    let mut trimmed = v.clone();
    if let Some(o) = trimmed.as_object_mut() {
        TRIM.iter().for_each(|k| {
            o.remove(*k);
        });
    }
    ProbeResult {
        url: url.to_string(),
        webpage_url: s(v, "webpage_url"),
        extractor: s(v, "extractor_key"),
        id: s(v, "id"),
        title: s(v, "title"),
        uploader: s(v, "uploader"),
        duration_s: f(v, "duration"),
        thumbnail_url: s(v, "thumbnail"),
        formats,
        entries,
        info_json: Some(trimmed.to_string()),
    }
}

fn extract(url: &str, opts: &Value) -> Result<ProbeResult, EngineError> {
    runtime::with_python(|py| {
        let run = || -> PyResult<String> {
            let o = json_to_py(py, opts)?;
            o.set_item("logger", Py::new(py, YdlLogger)?)?;
            let ydl = py.import("yt_dlp")?.getattr("YoutubeDL")?.call1((o,))?;
            let kw = PyDict::new(py);
            kw.set_item("download", false)?;
            let info = ydl.call_method("extract_info", (url,), Some(&kw))?;
            if info.is_none() {
                return Ok("{}".into());
            }
            py_to_json(py, &ydl.call_method1("sanitize_info", (info,))?)
        };
        match run() {
            Ok(j) => serde_json::from_str::<Value>(&j)
                .map(|v| from_info(url, &v))
                .map_err(|e| classify("ParseError", &e.to_string(), None)),
            Err(e) => Err(classify_pyerr(py, &e)),
        }
    })
}

/// Probe `url` with yt-dlp options `opts` (see `spec::probe_opts`). Blocks the caller up to
/// `timeout` (call from the `Io` pool, never from the UI thread). On timeout the Python call keeps
/// running on its thread (counted in [`abandoned_probes`]) and a retryable `Network` error returns.
pub fn probe(url: &str, opts: &Value, timeout: Duration) -> Result<ProbeResult, EngineError> {
    if runtime::get().is_none() {
        return Err(classify(
            "InitError",
            "python runtime is not initialised",
            None,
        ));
    }
    let (tx, rx) = mpsc::channel();
    let (url, opts) = (url.to_string(), opts.clone());
    std::thread::Builder::new()
        .name("probe".into())
        .spawn(move || {
            let _ = tx.send(extract(&url, &opts));
        })
        .map_err(|e| classify("SpawnError", &e.to_string(), None))?;
    rx.recv_timeout(timeout).unwrap_or_else(|_| {
        ABANDONED.fetch_add(1, Ordering::Relaxed);
        log::warn!(target: "engine::probe", "probe timed out after {timeout:?}, abandoned (total {})", abandoned_probes());
        Err(EngineError::new(ErrorCode::Network, "probe timed out"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn from_info_single_and_flat_playlist() {
        let one = from_info(
            "u",
            &json!({"id":"a","title":"T","duration":6.0,"formats":[{"format_id":"1","height":90,"filesize_approx":10.5,"ext":"mp4","protocol":"https"}],"extractor_key":"Generic","heatmap":[1]}),
        );
        assert_eq!(
            (one.id.as_deref(), one.title.as_deref(), one.is_playlist()),
            (Some("a"), Some("T"), false)
        );
        assert_eq!(
            (one.formats[0].height, one.formats[0].filesize),
            (Some(90), Some(10))
        );
        assert_eq!(one.formats[0].protocol.as_deref(), Some("https"));
        assert!(!one.info_json.unwrap().contains("heatmap"));
        let pl = from_info(
            "u",
            &json!({"id":"p","_type":"playlist","entries":[{"id":"1","url":"u1"},null,{"id":"2","title":"x"}]}),
        );
        let e = pl.entries.unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].index, e[0].url.as_str()), (1, "u1"));
    }
}
