//! Reads a browser's cookies with yt-dlp's own reader, as cookies.txt text (Cookies page sets).
use domain::{EngineError, ErrorCode};
use pyo3::prelude::*;

use crate::runtime::{self, classify_pyerr};

/// Every cookie `browser` (yt-dlp key: firefox, chrome, safari, ...) holds, in Netscape format.
/// Blocking; Chrome may ask for keychain access on macOS. Needs [`runtime::init`].
pub fn read_browser(browser: &str) -> Result<String, EngineError> {
    runtime::with_python(|py| {
        let run = || -> PyResult<String> {
            let jar = py
                .import("yt_dlp.cookies")?
                .call_method1("extract_cookies_from_browser", (browser,))?;
            let buf = py.import("io")?.call_method0("StringIO")?;
            jar.call_method1("save", (&buf,))?;
            buf.call_method0("getvalue")?.extract()
        };
        run().map_err(|e| {
            let mut err = classify_pyerr(py, &e);
            if err.code == ErrorCode::Unknown {
                err.message = format!("cannot read the {browser} cookies: {}", err.message);
            }
            err
        })
    })
}
