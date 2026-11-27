//! The check of custom yt-dlp argument lines with yt-dlp's own option parser (the pure line
//! handling lives in `domain::args`).
use std::ffi::CStr;

use domain::ArgCheck;
use pyo3::{prelude::*, types::PyList};

/// Options yt-dlp gets from ytdlpd itself: a line that changes one is left out.
const YTDLP_OWN: [&str; 18] = [
    "format",
    "format_sort",
    "merge_output_format",
    "outtmpl",
    "paths",
    "cookiefile",
    "cookiesfrombrowser",
    "proxy",
    "ratelimit",
    "ffmpeg_location",
    "js_runtimes",
    "external_downloader",
    "external_downloader_args",
    "noplaylist",
    "playlist_items",
    "quiet",
    "noprogress",
    "cachedir",
];

/// Parses each line alone, as `yt-dlp --ignore-config <line>` would: `("ok"|"own"|"bad", message,
/// close option)`. Help and version end the parser, and a bare word would be a link to download.
const CHECK: &CStr = c"
import contextlib, difflib, io, optparse

def check(lines, own):
    import yt_dlp
    from yt_dlp.options import create_parser
    names = sorted({o for opt in create_parser()._get_all_options() for o in opt._long_opts})
    base = yt_dlp.parse_options(['--ignore-config'])[3]
    out = []
    for args in lines:
        sink = io.StringIO()
        try:
            with contextlib.redirect_stdout(sink), contextlib.redirect_stderr(sink):
                parsed = yt_dlp.parse_options(['--ignore-config', *args])
        except (optparse.OptParseError, SystemExit) as err:
            text = str(err).strip() if isinstance(err, optparse.OptParseError) else ''
            message = text.splitlines()[-1].split('error: ', 1)[-1] if text else 'not for downloads'
            bad = next((a.split('=', 1)[0] for a in args
                        if a.startswith('--') and a.split('=', 1)[0] not in names), None)
            close = difflib.get_close_matches(bad, names, 1) if bad else []
            out.append(('bad', message, close[0] if close else None))
            continue
        if parsed[2]:
            out.append(('bad', 'not an option: ' + ' '.join(parsed[2]), None))
            continue
        changed = [k for k, v in parsed[3].items() if k not in base or base[k] != v]
        out.append(('own' if any(k in own for k in changed) else 'ok', None, None))
    return out
";

/// One answer per line from yt-dlp's parser, on the in-process interpreter: needs
/// [`crate::runtime::init`]. A failure of the check itself takes every line rather than block
/// downloads.
pub fn check_ytdlp(lines: &[Vec<String>]) -> Vec<ArgCheck> {
    crate::runtime::with_python(|py| answers(py, lines))
}

/// What the Python check returns per line: kind, message, close option.
type Found = (String, Option<String>, Option<String>);

fn answers(py: Python<'_>, lines: &[Vec<String>]) -> Vec<ArgCheck> {
    let run = || -> PyResult<Vec<Found>> {
        let lines = PyList::new(py, lines)?;
        let own = PyList::new(py, YTDLP_OWN)?;
        PyModule::from_code(py, CHECK, c"check_args.py", c"check_args")?
            .getattr("check")?
            .call1((lines, own))?
            .extract()
    };
    match run() {
        Ok(found) => found
            .into_iter()
            .map(|(kind, message, suggestion)| match kind.as_str() {
                "own" => ArgCheck::Reserved,
                "bad" => ArgCheck::Invalid {
                    message: message.unwrap_or_default(),
                    suggestion,
                },
                _ => ArgCheck::Ok,
            })
            .collect(),
        Err(error) => {
            log::warn!("engine: cannot check yt-dlp arguments: {error}");
            vec![ArgCheck::Ok; lines.len()]
        }
    }
}
