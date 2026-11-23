//! Embedded CPython startup: isolated PyInitConfig, no signal handlers, explicit sys.path, stdout/stderr to Rust.
use std::{
    ffi::CString,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use pyo3::{ffi, prelude::*};

use crate::classify::classify;
use domain::EngineError;

/// Receives interpreter text: `(level, message)`, one call per line.
pub type LogSink = fn(level: &str, msg: &str);

/// [`init`] parameters.
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// Python home (the dir holding `Lib`). `None`: env `YTDLPD_PYTHON_HOME`, then `<exe dir>/python`, then the dev runtime.
    pub home: Option<PathBuf>,
    /// Runtime-updated yt-dlp + ejs dir; first on `sys.path`.
    pub site_updates: Option<PathBuf>,
    /// Where interpreter stdout/stderr and yt-dlp logs go.
    pub sink: LogSink,
}

/// Typed startup failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitError {
    /// No usable Python home; holds the first location tried.
    HomeNotFound(PathBuf),
    /// CPython refused to start.
    Python(String),
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HomeNotFound(p) => write!(f, "python runtime not found at {}", p.display()),
            Self::Python(m) => write!(f, "python failed to start: {m}"),
        }
    }
}
impl std::error::Error for InitError {}

/// Handle to the one process-wide interpreter.
#[derive(Debug)]
pub struct Runtime {
    /// Resolved Python home.
    pub home: PathBuf,
    /// Time spent starting CPython plus stdio setup.
    pub init_time: Duration,
}

static RUNTIME: OnceLock<Result<Runtime, InitError>> = OnceLock::new();
static SINK: OnceLock<LogSink> = OnceLock::new();

/// Default sink: the `log` crate, target `py`.
pub fn log_sink(level: &str, msg: &str) {
    match level {
        "error" => log::error!(target: "py", "{msg}"),
        "warning" => log::warn!(target: "py", "{msg}"),
        "debug" => log::debug!(target: "py", "{msg}"),
        _ => log::info!(target: "py", "{msg}"),
    }
}

pub(crate) fn sink(level: &str, msg: &str) {
    SINK.get().copied().unwrap_or(log_sink)(level, msg);
}

/// The initialised runtime, if [`init`] succeeded.
pub fn get() -> Option<&'static Runtime> {
    RUNTIME.get().and_then(|r| r.as_ref().ok())
}

fn is_home(p: &Path) -> bool {
    p.join("Lib").join("os.py").is_file()
        || p.join("lib").join("python3.14").join("os.py").is_file()
}

/// Candidate homes in priority order: explicit, env, `<exe dir>/python`, dev runtime baked by build.rs.
pub fn resolve_home(explicit: Option<&Path>) -> Result<PathBuf, InitError> {
    let mut tried: Vec<PathBuf> = vec![];
    tried.extend(explicit.map(Path::to_path_buf));
    tried.extend(std::env::var_os("YTDLPD_PYTHON_HOME").map(PathBuf::from));
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        tried.push(d.join("python"));
    }
    tried.extend(option_env!("YTDLPD_DEV_PYTHON_HOME").map(PathBuf::from));
    tried
        .iter()
        .find(|p| is_home(p))
        .cloned()
        .ok_or_else(|| InitError::HomeNotFound(tried.into_iter().next().unwrap_or_default()))
}

/// Start the interpreter once; later calls return the same handle (or the same error). Does not hold the GIL on return.
pub fn init(opts: InitOptions) -> Result<&'static Runtime, InitError> {
    RUNTIME
        .get_or_init(|| {
            let t = Instant::now();
            let _ = SINK.set(opts.sink);
            let home = resolve_home(opts.home.as_deref())?;
            start(&home, opts.site_updates.as_deref()).map_err(InitError::Python)?;
            Ok(Runtime {
                home,
                init_time: t.elapsed(),
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn cstr(s: &str) -> CString {
    CString::new(s).expect("no NUL in config string")
}

fn start(home: &Path, updates: Option<&Path>) -> Result<(), String> {
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let mut paths: Vec<String> = updates.map(s).into_iter().collect();
    paths.push(s(&home.join("site")));
    if cfg!(windows) {
        paths.extend([s(&home.join("Lib")), s(&home.join("DLLs"))]);
    } else {
        paths.extend([
            s(&home.join("lib/python3.14")),
            s(&home.join("lib/python3.14/lib-dynload")),
        ]);
    }
    let exe = if cfg!(windows) {
        home.join("python.exe")
    } else {
        home.join("bin/python3")
    };
    // SAFETY: plain C API calls on a config we own; Py_InitializeFromInitConfig runs once (guarded by the OnceLock in `init`).
    unsafe {
        let cfg = ffi::PyInitConfig_Create();
        let check = |rc: i32, what: &str| -> Result<(), String> {
            if rc >= 0 {
                return Ok(());
            }
            let mut m: *const std::os::raw::c_char = std::ptr::null();
            ffi::PyInitConfig_GetError(cfg, &mut m);
            let msg = if m.is_null() {
                "unknown".to_string()
            } else {
                std::ffi::CStr::from_ptr(m).to_string_lossy().into_owned()
            };
            Err(format!("{what}: {msg}"))
        };
        let run = || -> Result<(), String> {
            for (k, v) in [
                ("isolated", 1u64),
                ("use_environment", 0),
                ("user_site_directory", 0),
                ("site_import", 0),
                ("install_signal_handlers", 0),
                ("write_bytecode", 0),
                ("utf8_mode", 1),
                ("parse_argv", 0),
                ("pathconfig_warnings", 0),
                ("buffered_stdio", 0),
            ] {
                check(ffi::PyInitConfig_SetInt(cfg, cstr(k).as_ptr(), v), k)?;
            }
            check(
                ffi::PyInitConfig_SetStr(cfg, cstr("home").as_ptr(), cstr(&s(home)).as_ptr()),
                "home",
            )?;
            check(
                ffi::PyInitConfig_SetStr(cfg, cstr("executable").as_ptr(), cstr(&s(&exe)).as_ptr()),
                "executable",
            )?;
            let owned: Vec<CString> = paths.iter().map(|p| cstr(p)).collect();
            let mut ptrs: Vec<*const std::os::raw::c_char> =
                owned.iter().map(|c| c.as_ptr()).collect();
            check(
                ffi::PyInitConfig_SetStrList(
                    cfg,
                    cstr("module_search_paths").as_ptr(),
                    ptrs.len(),
                    ptrs.as_mut_ptr(),
                ),
                "module_search_paths",
            )?;
            check(
                ffi::Py_InitializeFromInitConfig(cfg),
                "Py_InitializeFromInitConfig",
            )
        };
        let res = run();
        ffi::PyInitConfig_Free(cfg);
        res?;
        // The GIL is held by this thread now.
        let py = Python::assume_attached();
        let setup = (|| -> PyResult<()> {
            let sys = py.import("sys")?;
            sys.setattr("stdout", Py::new(py, LogWriter::new("info"))?)?;
            sys.setattr("stderr", Py::new(py, LogWriter::new("warning"))?)?;
            Ok(())
        })();
        let _ = ffi::PyEval_SaveThread(); // release the GIL: no thread holds it idle
        setup.map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// `sys.stdout`/`sys.stderr` replacement: complete lines go to the sink.
#[pyclass]
pub struct LogWriter {
    level: &'static str,
    buf: Mutex<String>,
}

impl LogWriter {
    fn new(level: &'static str) -> Self {
        Self {
            level,
            buf: Mutex::new(String::new()),
        }
    }
}

#[pymethods]
impl LogWriter {
    fn write(&self, s: &str) -> usize {
        let mut b = self.buf.lock().unwrap();
        b.push_str(s);
        while let Some(i) = b.find('\n') {
            let line: String = b.drain(..=i).collect();
            let line = line.trim_end();
            if !line.is_empty() {
                sink(self.level, line);
            }
        }
        s.chars().count()
    }
    fn flush(&self) {
        let mut b = self.buf.lock().unwrap();
        if !b.trim().is_empty() {
            sink(self.level, b.trim_end());
        }
        b.clear();
    }
    fn isatty(&self) -> bool {
        false
    }
    fn writable(&self) -> bool {
        true
    }
    #[getter]
    fn encoding(&self) -> &'static str {
        "utf-8"
    }
}

/// yt-dlp `logger=` object: `debug` carries normal output and `[debug]` lines.
#[pyclass]
pub struct YdlLogger;

#[pymethods]
impl YdlLogger {
    fn debug(&self, m: &str) {
        sink(
            if m.starts_with("[debug] ") {
                "debug"
            } else {
                "info"
            },
            m,
        );
    }
    fn info(&self, m: &str) {
        sink("info", m);
    }
    fn warning(&self, m: &str) {
        sink("warning", m);
    }
    fn error(&self, m: &str) {
        sink("error", m);
    }
}

/// Turn a Python exception into a classified error (looks through the `exc_info` of yt-dlp errors for the HTTP status).
pub(crate) fn classify_pyerr(py: Python<'_>, e: &PyErr) -> EngineError {
    let val = e.value(py);
    let ty = val
        .get_type()
        .name()
        .map(|n| n.to_string())
        .unwrap_or_default();
    let msg = val
        .str()
        .map(|s| s.to_string())
        .unwrap_or_else(|_| e.to_string());
    let status = |o: &Bound<'_, PyAny>| {
        ["status", "code"]
            .iter()
            .find_map(|k| o.getattr(*k).ok()?.extract::<u16>().ok())
    };
    let inner = val
        .getattr("exc_info")
        .ok()
        .and_then(|t| t.get_item(1).ok())
        .filter(|i| !i.is_none());
    let mut err = classify(
        &ty,
        &msg,
        status(val.as_any()).or_else(|| inner.as_ref().and_then(status)),
    );
    if err.code == domain::ErrorCode::RateLimited {
        // HTTPError.response.headers (yt-dlp), or .headers (urllib), possibly on the wrapped exception
        let hdr = |o: &Bound<'_, PyAny>| -> Option<u64> {
            let h = o
                .getattr("response")
                .and_then(|r| r.getattr("headers"))
                .or_else(|_| o.getattr("headers"))
                .ok()?;
            let v: String = h
                .call_method1("get", ("Retry-After",))
                .ok()?
                .extract()
                .ok()?;
            crate::classify::parse_retry_after(&v)
        };
        err.retry_after = hdr(val.as_any()).or_else(|| inner.as_ref().and_then(hdr));
    }
    err
}

/// Run `f` with the GIL. Panics only if [`init`] has not succeeded.
pub fn with_python<R>(f: impl FnOnce(Python<'_>) -> R) -> R {
    assert!(get().is_some(), "engine::runtime::init must succeed first");
    Python::attach(f)
}

/// Python object from a JSON value (via `json.loads`).
pub(crate) fn json_to_py<'py>(
    py: Python<'py>,
    v: &serde_json::Value,
) -> PyResult<Bound<'py, PyAny>> {
    py.import("json")?.call_method1("loads", (v.to_string(),))
}

/// JSON text from a Python object, stringifying what JSON cannot hold.
pub(crate) fn py_to_json(py: Python<'_>, o: &Bound<'_, PyAny>) -> PyResult<String> {
    let kw = pyo3::types::PyDict::new(py);
    kw.set_item("default", py.import("builtins")?.getattr("str")?)?;
    py.import("json")?
        .call_method("dumps", (o,), Some(&kw))?
        .extract()
}

/// What [`self_test`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfTest {
    /// Python version.
    pub python: String,
    /// yt-dlp version.
    pub yt_dlp: String,
    /// yt-dlp-ejs version.
    pub yt_dlp_ejs: String,
    /// Where `yt_dlp` was imported from (shows whether a `--site` overlay won).
    pub yt_dlp_path: String,
    /// Interpreter start time.
    pub init_ms: u128,
}

/// Start the interpreter and import yt-dlp and yt-dlp-ejs. Backs `ytdlpd --self-test [--site <dir>]`.
pub fn self_test(site_updates: Option<PathBuf>) -> Result<SelfTest, String> {
    if let Some(d) = &site_updates
        && !d.is_dir()
    {
        return Err(format!("site dir {} does not exist", d.display()));
    }
    let rt = init(InitOptions {
        home: None,
        site_updates: site_updates.clone(),
        sink: log_sink,
    })
    .map_err(|e| e.to_string())?;
    with_python(|py| {
        let run = || -> PyResult<SelfTest> {
            let ydl = py.import("yt_dlp")?;
            py.import("yt_dlp_ejs")?;
            let meta = py.import("importlib.metadata")?;
            Ok(SelfTest {
                python: py
                    .import("platform")?
                    .call_method0("python_version")?
                    .extract()?,
                yt_dlp: py
                    .import("yt_dlp.version")?
                    .getattr("__version__")?
                    .extract()?,
                yt_dlp_ejs: meta.call_method1("version", ("yt-dlp-ejs",))?.extract()?,
                yt_dlp_path: ydl.getattr("__file__")?.extract()?,
                init_ms: rt.init_time.as_millis(),
            })
        };
        let t = run().map_err(|e| e.to_string())?;
        match &site_updates {
            Some(d) if !std::path::Path::new(&t.yt_dlp_path).starts_with(d) => Err(format!(
                "yt-dlp was not loaded from {} but from {}",
                d.display(),
                t.yt_dlp_path
            )),
            _ => Ok(t),
        }
    })
}
