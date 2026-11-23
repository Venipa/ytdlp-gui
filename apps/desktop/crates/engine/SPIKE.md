# G1 engine spike report (Lane B, Windows 11 x64, 2026-11-23)

## Decision: PyO3 embedding PASSES G1. No worker.py fallback built.
All four G1 failure criteria checked on Windows x64; none failed.

| G1 criterion | Result |
|---|---|
| link/load of PBS libpython via `PYO3_CONFIG_FILE` (pyo3 0.30.1, no `auto-initialize`) | OK. `libs/python314.lib` links, `python314.dll` copied next to exes by `build.rs` (profile dir, `deps/`, `examples/`) |
| hook stability | OK. `progress_hooks`, `postprocessor_hooks`, `post_hooks` as `#[pyclass]` `Hooks`, never raise into yt-dlp; progress throttled to 4/s (seen: events at ~330 ms spacing); final progress always sent |
| kill-tree | OK. Test `kill_tree_kills_grandchild` (python child that starts a python grandchild; both dead after kill). `cancel_mid_download_kills_worker_and_cleans_partials`: kill < 2 s, worker dead, `.part` removed, out dir empty |
| worker from a GUI-subsystem parent with piped stdio | OK. `examples/gui_parent.rs` is PE subsystem 2 (GUI) and is also the worker (re-exec `--engine-worker`); full download with events through pipes, exit 0 |

Also verified: in-process `probe` (single URL, timeout + abandoned counter), `runtime::init` second call returns the same handle, mp4 download (bytes equal to fixture, `result.path` == file on disk), HLS download via ffmpeg (`frag n/3` progress, `[FixupM3u8]`), HTTP 404 classified `unavailable`.

## Measurements (release build unless noted; one run each, this box, Defender on)
| Metric | Value |
|---|---|
| Python init (`Py_InitializeFromInitConfig` + stdio swap) | 6 to 8 ms |
| `import yt_dlp` (with .pyc present) | 250 to 330 ms release, 330 to 450 ms debug. Without .pyc: about 900 ms |
| first in-process probe (incl. import) / second probe | 546 ms / 30 ms (local fixture) |
| worker time to `ready` (init + `import yt_dlp`) | 1 worker: 266 ms. 3 parallel: 264 / 554 / 552 ms |
| RSS per active worker | working set 66 MB, private 52 MB. 3 workers: 200 MB working set total |
| GUI/host process | after init 11 MB working set; after `import yt_dlp` + 2 probes 67 MB (in-process probes cost the GUI about 55 MB) |
| runtime on disk, win x64, pruned, libs/*.lib dropped | 80 MB (python 20 Lib + 19 DLLs, 35 site of which yt_dlp 25, of which .pyc 26.8). Dev runtime with `libs/` 80.5 MB. Without any .pyc about 53 MB |
| compressed (tar.gz of the shipped tree) | 28.4 MB (PBS download alone is 23.7 MB for win x64; notes' 51.5 MB figure was wrong) |

.pyc files are kept on purpose (`setup.ts` runs `compileall`): `write_bytecode=0` at runtime, so without them every worker recompiles yt-dlp (+0.6 s per start).

## What was built (future layout: `apps/desktop/crates/engine`, `apps/desktop/scripts`, `apps/desktop/runtime.lock.json`)
- `scripts/setup.ts` (bun): resolves PBS tag (pinned in lock, else `releases/latest`), asset `cpython-3.14.10+<tag>-<triple>-install_only_stripped.tar.gz` (names verified against the release API; 6-triple table), SHA256SUMS check (fail closed, also against the pinned lock hash), unpack, `pip install --target python/site` for yt-dlp (+ exact `yt-dlp-ejs` from the wheel METADATA, D-104), certifi, requests, urllib3, websockets, mutagen, brotli, pycryptodomex, compileall, prune list, import smoke before and after pruning, `runtime/pyo3.cfg`, `runtime.lock.json` (tag, asset sha256, installed versions, size). Args: `--root` (or env `YTDLPD_ROOT`, default parent of `scripts/`), `--triple`, `--force`, `--ship` (drops `libs/`). Only the Windows x64 triple was actually run.
- `engine` crate: `runtime` (init/LogWriter/YdlLogger), `probe`, `worker` (`run_worker() -> !`), `host` (`spawn_worker`, `WorkerHandle`, `TreeChild` kill-tree), `ipc`, `spec`, `classify` (+ `tests/errors.tsv`, 39 rows), `partials`.
- Examples `fixture_server`, `fetch`, `gui_parent`, `measure`; fixtures 105 KB (`sample.mp4`, `hls/`, `sample.mp3`) + `scripts/make-fixtures.sh`.

## IPC (architecture.md schema, extensions marked)
Host to worker, one line: `{"cmd":"download","url":"...","spec":{yt-dlp opts}}` (**extension: `url` beside `spec`**); `{"cmd":"exit"}`. Worker to host: `ready` (**sent after `import yt_dlp`, so "ready" = usable**), `progress` (`done,total,speed,eta,frag`), `phase` (`downloading`, `merging`, `postprocess:<Name>`), `path`, `files` (`.part`, `.ytdl`, `filename`, `tmpfilename`, deduped), `log` (yt-dlp logger and stray stdout/stderr; **levels debug/info/warning/error**), `result` (**extension: `path` = verified final path (from `requested_downloads[].filepath`, hook path only as fallback, mismatch logged), `paths` for playlists, small `info` summary instead of the full dict**), `error` (`code,message,retryable`). Host-side extra: stderr lines of the worker arrive as `log` with level `stderr`; unparsable stdout lines as `log` level `stdout`. Worker exits on stdin EOF (parent gone, exit 3), `exit` (0), process exit without `Py_Finalize`.

## Deviations and decisions for the lead
1. **Job Object per worker, not one for the app.** Nested jobs work on Win 8+, a single tree can be killed on its own, and KILL_ON_JOB_CLOSE still reaps all workers when the GUI dies. Known gap (comment in code): grandchild forked before the assignment microseconds after spawn would escape; workers only fork after Python is up.
2. Worker re-exec needs the app `main` to call `engine::worker::run_worker()` first thing when argv[1] == `--engine-worker` (before gpui). Env `YTDLPD_PYTHON_HOME` / `YTDLPD_SITE_UPDATES` carry the runtime and update dir (also `WorkerConfig`).
3. `build.rs` (DLL copy + rpath + `YTDLPD_DEV_PYTHON_HOME`) lives in the engine crate; the final app layout (`python/` beside the exe, DLLs beside the exe) must be done by the `ytdlpd` bin `build.rs`/packager. Any crate depending on `engine` needs `PYO3_CONFIG_FILE` set to link, so `real-engine` must gate that dependency (fake backend path builds without Python).
4. Types `JobSpec`, `Preset`, `ProbeResult`, `ErrorCode`, `EngineError` live in `engine` for now; they move to `domain` at `contract-1`.
5. `rustfmt.toml` in the crate is a copy of maple's (edition 2024); drop it when the workspace one exists.
6. `JobSpec.extra_args` is stored but not applied (needs `yt_dlp.parse_options`; marked `shortcut:` in code).

## Not verified / open
- macOS and Linux: `cargo check --target x86_64-unknown-linux-gnu` is clean (unix kill-tree, PDEATHSIG, rpath code type-checks) but nothing ran there. macOS PBS `libpython3.14.dylib` install name still unverified; `setup.ts` has no `install_name_tool` step yet.
- Not covered by tests: pause/resume via Range (`continuedl`), AES HLS, 500-entry playlist, merge (`[Merger]`), SponsorBlock, cookies; progress hook for HLS in `--hls-prefer-ffmpeg` mode.
- `--self-test` (ytdlpd crate) not written; `runtime::init` + `py.import("yt_dlp")` + `yt_dlp_ejs` is all it needs (`site_updates` = `--site`).
- Measurements are single runs without a warm/cold split of the Defender scan; repeat in CI.
- Probe in the GUI process adds about 55 MB resident; if that matters, the architecture already allows moving probes to a warm worker (same protocol would need a `probe` command, not implemented).
