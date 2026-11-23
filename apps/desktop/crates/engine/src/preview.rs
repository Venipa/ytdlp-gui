//! Output template preview: the real `YoutubeDL.prepare_filename`, in-process (Settings preview).
use domain::{EngineError, ErrorCode, JobSpec};
use pyo3::prelude::*;
use serde_json::{Value, json};

use crate::{
    runtime::{self, classify_pyerr, json_to_py},
    spec::{Env, to_opts},
};

/// The path yt-dlp would write for `info` (a yt-dlp info dict: `id`, `title`, `ext`, ...) under `spec`'s
/// folder and template. Needs [`runtime::init`] to have succeeded.
pub fn preview_filename(spec: &JobSpec, env: &Env, info: &Value) -> Result<String, EngineError> {
    let full = to_opts(spec, env);
    let mut opts = json!({});
    for k in ["outtmpl", "paths", "trim_file_name"] {
        opts[k] = full[k].clone();
    }
    runtime::with_python(|py| {
        let run = || -> PyResult<String> {
            let ydl = py
                .import("yt_dlp")?
                .getattr("YoutubeDL")?
                .call1((json_to_py(py, &opts)?,))?;
            ydl.call_method1("prepare_filename", (json_to_py(py, info)?,))?
                .extract()
        };
        run().map_err(|e| {
            let mut err = classify_pyerr(py, &e);
            if err.code == ErrorCode::Unknown {
                err.message = format!("cannot preview the file name: {}", err.message);
            }
            err
        })
    })
}
