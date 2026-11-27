//! Worker side (`<exe> --engine-worker`): one JSON job on stdin, JSON-lines events on stdout, hooks as `#[pyclass]`.
use std::{
    collections::HashSet,
    ffi::CStr,
    io::BufRead,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use domain::{EngineError, ErrorCode};
use pyo3::{prelude::*, types::PyDict};
use serde_json::{Value, json};

use crate::{
    classify::classify,
    ipc::{Command, Event, emit},
    runtime::{self, InitOptions, YdlLogger, classify_pyerr, json_to_py, py_to_json},
};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// How often an aria2c control file is read (aria2c saves it each second, see spec.rs).
const ARIA2_POLL: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Shared {
    last_progress: Mutex<Option<Instant>>,
    seen_files: Mutex<HashSet<String>>,
    paths: Mutex<Vec<String>>,
    downloading: Mutex<bool>,
}

impl Shared {
    /// Reports the paths not reported before, for cleanup.
    fn note_files(&self, paths: impl IntoIterator<Item = String>) {
        let mut seen = self.seen_files.lock().unwrap();
        let new: Vec<String> = paths
            .into_iter()
            .filter(|p| seen.insert(p.clone()))
            .collect();
        if !new.is_empty() {
            emit(&Event::Files { paths: new });
        }
    }

    /// Sends the `downloading` phase once.
    fn started(&self) {
        let mut started = self.downloading.lock().unwrap();
        if !std::mem::replace(&mut *started, true) {
            emit(&Event::Phase {
                name: "downloading".into(),
            });
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Progress,
    Postprocessor,
    Post,
}

/// Registered as `progress_hooks`, `postprocessor_hooks` and `post_hooks`; never raises into yt-dlp.
#[pyclass]
struct Hooks {
    kind: Kind,
    sh: Arc<Shared>,
}

fn get<'py>(d: &Bound<'py, PyAny>, k: &str) -> Option<Bound<'py, PyAny>> {
    d.get_item(k).ok().filter(|v| !v.is_none())
}
fn num(d: &Bound<'_, PyAny>, k: &str) -> Option<f64> {
    get(d, k)?.extract().ok()
}
fn text(d: &Bound<'_, PyAny>, k: &str) -> Option<String> {
    get(d, k)?.extract().ok()
}

impl Hooks {
    fn progress(&self, d: &Bound<'_, PyAny>) {
        let named = text(d, "filename").map(|f| [format!("{f}.part"), format!("{f}.ytdl"), f]);
        let tmp = text(d, "tmpfilename");
        self.sh.note_files(named.into_iter().flatten().chain(tmp));
        let status = text(d, "status").unwrap_or_default();
        if status != "downloading" && status != "finished" {
            return;
        }
        self.sh.started();
        if status == "downloading" {
            let mut last = self.sh.last_progress.lock().unwrap();
            if last.is_some_and(|t| t.elapsed() < PROGRESS_INTERVAL) {
                return;
            }
            *last = Some(Instant::now());
        }
        let total = num(d, "total_bytes")
            .or_else(|| num(d, "total_bytes_estimate"))
            .map(|n| n as u64);
        let frag = num(d, "fragment_index")
            .zip(num(d, "fragment_count"))
            .map(|(i, n)| (i as u32, n as u32));
        emit(&Event::Progress {
            done: num(d, "downloaded_bytes").unwrap_or(0.0) as u64,
            total,
            speed: num(d, "speed"),
            eta: num(d, "eta").map(|n| n as u64),
            frag,
        });
    }

    fn postprocessor(&self, d: &Bound<'_, PyAny>) {
        if text(d, "status").as_deref() != Some("started") {
            return;
        }
        let name = text(d, "postprocessor").unwrap_or_default();
        emit(&Event::Phase {
            name: if name == "Merger" {
                "merging".into()
            } else {
                format!("postprocess:{name}")
            },
        });
    }

    fn post(&self, f: &Bound<'_, PyAny>) {
        if let Ok(p) = f.extract::<String>() {
            self.sh.paths.lock().unwrap().push(p.clone());
            emit(&Event::Path { path: p });
        }
    }
}

#[pymethods]
impl Hooks {
    fn __call__(&self, d: &Bound<'_, PyAny>) {
        match self.kind {
            Kind::Progress => self.progress(d),
            Kind::Postprocessor => self.postprocessor(d),
            Kind::Post => self.post(d),
        }
    }
}

/// Stands in for the progress hooks yt-dlp never calls while aria2c runs: told when each aria2c
/// download starts and ends, it names its files for cleanup and reads progress from its control file.
#[pyclass]
struct Aria2Watch {
    sh: Arc<Shared>,
    stop: Mutex<Option<Arc<AtomicBool>>>,
}

#[pymethods]
impl Aria2Watch {
    fn start(&self, tmp: String) {
        let control = format!("{tmp}.aria2");
        self.sh.note_files([tmp, control.clone()]);
        self.sh.started();
        let stop = Arc::new(AtomicBool::new(false));
        if let Some(old) = self.stop.lock().unwrap().replace(stop.clone()) {
            old.store(true, Ordering::Relaxed);
        }
        std::thread::spawn(move || poll_aria2(&control, &stop));
    }

    fn stop(&self) {
        if let Some(stop) = self.stop.lock().unwrap().take() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

/// Sends progress from `control` each [`ARIA2_POLL`] until `stop`; speed spans the last change.
fn poll_aria2(control: &str, stop: &AtomicBool) {
    let (mut last, mut speed): (Option<(Instant, u64)>, Option<f64>) = (None, None);
    loop {
        std::thread::sleep(ARIA2_POLL);
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let Some((done, total)) = std::fs::read(control)
            .ok()
            .and_then(|b| crate::aria2::control_progress(&b))
        else {
            continue;
        };
        let now = Instant::now();
        match last {
            Some((_, d)) if d == done => {}
            Some((t, d)) => {
                speed = Some(done.saturating_sub(d) as f64 / now.duration_since(t).as_secs_f64());
                last = Some((now, done));
            }
            None => last = Some((now, done)),
        }
        emit(&Event::Progress {
            done,
            total: Some(total),
            speed,
            eta: speed
                .filter(|s| *s > 0.)
                .map(|s| ((total - done) as f64 / s) as u64),
            frag: None,
        });
    }
}

/// Wraps yt-dlp's aria2c downloader so `watch` hears when each aria2c download starts and ends.
const WATCH_ARIA2: &CStr = c"
def patch(watch):
    from yt_dlp.downloader.external import Aria2cFD
    real = Aria2cFD._call_downloader
    def call(self, tmpfilename, info_dict):
        watch.start(tmpfilename)
        try:
            return real(self, tmpfilename, info_dict)
        finally:
            watch.stop()
    Aria2cFD._call_downloader = call
";

fn ipc_sink(level: &str, msg: &str) {
    emit(&Event::Log {
        level: level.into(),
        msg: msg.into(),
    });
}

fn fail(e: EngineError) -> ! {
    emit(&Event::Error {
        code: e.code,
        message: e.message,
        retryable: e.retryable,
        retry_after: e.retry_after,
    });
    std::process::exit(1)
}

/// Final paths from a sanitized info dict: `requested_downloads[*].filepath`, entries included for playlists.
fn info_paths(info: &Value) -> Vec<String> {
    let own = info
        .get("requested_downloads")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| r.get("filepath").and_then(Value::as_str));
    let nested = info
        .get("entries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(info_paths);
    own.map(str::to_string).chain(nested).collect()
}

/// Merges CLI-style args into `opts`: only what differs from the defaults of `parse_options`, post-processors appended.
const MERGE_ARGS: &CStr = c"
def merge(opts, args):
    import yt_dlp
    try:
        base = yt_dlp.parse_options(['--ignore-config'])[3]
        new = yt_dlp.parse_options(['--ignore-config'] + list(args))[3]
    except SystemExit:
        raise ValueError('invalid extra arguments: ' + ' '.join(args))
    for k, v in new.items():
        if k in base and base[k] == v:
            continue
        if k == 'postprocessors':
            opts.setdefault(k, []).extend(p for p in v if p not in base.get(k, []))
        else:
            opts[k] = v
";

fn download(py: Python<'_>, url: &str, spec: &Value, sh: &Arc<Shared>) -> PyResult<Value> {
    let opts = json_to_py(py, spec)?;
    let extra = opts.call_method1("pop", (crate::spec::EXTRA_ARGS_KEY, py.None()))?;
    if !extra.is_none() {
        PyModule::from_code(py, MERGE_ARGS, c"merge_args.py", c"merge_args")?
            .getattr("merge")?
            .call1((&opts, extra))?;
    }
    let hook = |kind| {
        Py::new(
            py,
            Hooks {
                kind,
                sh: sh.clone(),
            },
        )
    };
    opts.set_item("progress_hooks", vec![hook(Kind::Progress)?])?;
    opts.set_item("postprocessor_hooks", vec![hook(Kind::Postprocessor)?])?;
    opts.set_item("post_hooks", vec![hook(Kind::Post)?])?;
    opts.set_item("logger", Py::new(py, YdlLogger)?)?;
    // one job per worker process, so the downloader class is patched once
    if get(&opts, "external_downloader").is_some() {
        let watch = Py::new(
            py,
            Aria2Watch {
                sh: sh.clone(),
                stop: Mutex::new(None),
            },
        )?;
        PyModule::from_code(py, WATCH_ARIA2, c"watch_aria2.py", c"watch_aria2")?
            .getattr("patch")?
            .call1((watch,))?;
    }
    if let Some(c) = get(&opts, "cookiesfrombrowser") {
        opts.set_item(
            "cookiesfrombrowser",
            py.import("builtins")?.getattr("tuple")?.call1((c,))?,
        )?; // JSON has no tuples
    }
    let ydl = py.import("yt_dlp")?.getattr("YoutubeDL")?.call1((opts,))?;
    let kw = PyDict::new(py);
    kw.set_item("download", true)?;
    let info = ydl.call_method("extract_info", (url,), Some(&kw))?;
    if info.is_none() {
        return Ok(Value::Null);
    }
    let json = py_to_json(py, &ydl.call_method1("sanitize_info", (info,))?)?;
    Ok(serde_json::from_str(&json).unwrap_or(Value::Null))
}

fn run_job(url: &str, spec: &Value) -> Event {
    let sh = Arc::new(Shared::default());
    let info = match runtime::with_python(|py| {
        download(py, url, spec, &sh).map_err(|e| classify_pyerr(py, &e))
    }) {
        Ok(i) => i,
        Err(e) => fail(e),
    };
    let from_info = info_paths(&info);
    let hooked = sh.paths.lock().unwrap().clone();
    // Cross-check (architecture.md "Final path"): info wins, the last hook path is the fallback, never guessed.
    let paths = if from_info.is_empty() {
        hooked.clone()
    } else {
        from_info
    };
    if let (Some(a), Some(b)) = (paths.last(), hooked.last())
        && a != b
    {
        ipc_sink("warning", &format!("final path differs: info={a} hook={b}"));
    }
    let Some(path) = paths.last().cloned() else {
        fail(classify(
            "NoOutput",
            "download finished without an output file",
            None,
        ));
    };
    let pick = |k: &str| info.get(k).cloned().unwrap_or(Value::Null);
    Event::Result {
        ok: true,
        path: Some(path),
        paths,
        info: json!({"id": pick("id"), "title": pick("title"), "ext": pick("ext"), "extractor_key": pick("extractor_key"), "webpage_url": pick("webpage_url"), "duration": pick("duration")}),
    }
}

/// Worker entry: runs one job then exits the process (no `Py_Finalize`, exit code 0 on success).
/// Env `YTDLPD_PYTHON_HOME` and `YTDLPD_SITE_UPDATES` are set by the host.
pub fn run_worker() -> ! {
    let opts = InitOptions {
        home: None,
        site_updates: std::env::var_os("YTDLPD_SITE_UPDATES").map(Into::into),
        sink: ipc_sink,
    };
    let rt = match runtime::init(opts) {
        Ok(rt) => rt,
        Err(e) => fail(EngineError::new(ErrorCode::Unknown, e.to_string())),
    };
    // "ready" means usable: yt-dlp is imported (the bulk of the start cost) before it is sent.
    let t = Instant::now();
    if let Err(e) = runtime::with_python(|py| {
        py.import("yt_dlp")
            .map(|_| ())
            .map_err(|e| classify_pyerr(py, &e))
    }) {
        fail(e);
    }
    ipc_sink(
        "debug",
        &format!(
            "python init {} ms, import yt_dlp {} ms",
            rt.init_time.as_millis(),
            t.elapsed().as_millis()
        ),
    );
    emit(&Event::Ready);
    let mut first = String::new();
    let _ = std::io::stdin().read_line(&mut first);
    let Ok(cmd) = serde_json::from_str::<Command>(&first) else {
        fail(classify(
            "ProtocolError",
            &format!("bad first line: {first}"),
            None,
        ));
    };
    let Command::Download { url, spec } = cmd else {
        std::process::exit(0)
    };
    // Parent gone (EOF) or `exit` command: stop at once, children die with the job object / process group.
    std::thread::spawn(move || {
        for l in std::io::stdin().lock().lines().map_while(Result::ok) {
            if matches!(serde_json::from_str::<Command>(&l), Ok(Command::Exit)) {
                std::process::exit(0);
            }
        }
        std::process::exit(3);
    });
    let ev = run_job(&url, &spec);
    emit(&ev);
    std::process::exit(0)
}
