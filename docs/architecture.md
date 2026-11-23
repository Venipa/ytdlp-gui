# ytdlpd architecture

## 1. Crates and dependency direction
```
ytdlpd (bin) -> views -> state -> {engine, tools, storage, domain}
ytdlpd, views, state -> {ui, router, input, i18n, icons}
router, input -> ui -> {i18n, icons} -> gpui
cli <- ytdlpd (args + wire)
domain: no deps on anything below; engine, tools, storage, state depend on domain
engine, tools, storage, domain: no gpui, no ui (headless, unit-testable)
```
| Crate | Responsibility |
|---|---|
| `ytdlpd` | main, window and titlebar frame, actions, assets and fonts, http shim (reqwest to gpui HttpClient for thumbnails, from maple), tray (tray-icon win/mac, ksni linux), single instance (interprocess), deep link, notifications, logging, flags (`--data-dir`, `--window-size`, `--demo`, `--self-test`, `--frame-stats`, `--engine-worker`), build.rs runtime copy. Feature `real-engine` (default on) |
| `cli` | clap args (`add <url>...`, `--minimized`), wire protocol to the running instance (port of maple `cli/wire.rs`) |
| `views` | root (Destination match), Workspace shell, title-bar nav, screens (welcome, downloads, logs, settings/*), shared (confirm dialog, menus, row cells) |
| `state` | gpui entities: `App` global; `AppSettings` (settings.json + notify hot reload); `Downloads` (queue, scheduler, lifecycle, persistence); `Probes`; `Tools`; `Logs`; `Clipboard`; `Toasts`; `Io` (copied from maple: 4 workers, 8 blocking); `fake` (`FakeBackend`) |
| `domain` | contract crate: `DownloadId`, `DownloadSummary`, `DownloadStatus`, `JobSpec`, `Preset`, `ProbeResult`, `EngineError`/`ErrorCode`, `ToolStatus`, filter AST and token parser, `transition()`, backoff, `Clock` trait |
| `storage` | sqlx `SqlitePool`, embedded migrations, typed repo fns, `paths`, `app_dir_name()` (`ytdlpd` / `ytdlpd.dev`) |
| `engine` | `Backend` trait; embedded CPython via PyO3 (`runtime`, `probe`); `--engine-worker` host and worker side (`worker`, hooks as `#[pyclass]`); `spec` (presets to yt-dlp opts, pure); `classify` (pure); `partials`; fallback `worker/worker.py` |
| `tools` | `Tool` enum, detect, fetch matrix, streaming download + sha256, unpack (zip, tar.xz, tar.gz), version probe, PyPI wheel fetch/verify/install, Python runtime repair |
| `ui`, `router`, `input`, `i18n`, `icons`, `embed` | ported from maple; `ui` gains `ProgressRule`, `Thumbnail`, `Chip`, `TokenInput`, `SideNav`, `CallNumber`, `MONO` |
Dropped from maple: rusqlite, audio, music and webview crates.

## 2. Workspace dependencies (all pre-declared in Phase 0a; re-verify versions with `cargo add` then)
From maple (same versions): gpui and gpui_platform (path), tokio 1.54 (rt-multi-thread, net, io-util, macros, process, sync, time), reqwest 0.13 (no default; json, query, rustls-no-provider, stream) + rustls 0.23 (ring, std), serde 1, serde_json 1, anyhow 1, log 0.4, env_logger 0.11, futures 0.3, dirs 7, notify 8.2, fluent-bundle 0.16, unic-langid 0.9, sys-locale 0.3, jiff 0.2, sha2 0.11, tempfile 3.27, open 5.4, interprocess 2.4, clap 4 (+ clap_complete 4.6), image 0.25 (jpeg, png, webp), resvg 0.49, tray-icon 0.26, ksni 0.3, zbus 6, windows 0.62 / windows-sys 0.61, objc2 family 0.3/0.6, unicode-bidi, unicode-segmentation, qrcodegen, ashpd 0.14.
New: `sqlx` 0.9.1 first (`default-features = false`, features `runtime-tokio`, `sqlite`, `migrate`, `macros`; 0.9 feature names must be verified), fall back to 0.8.6 (cached locally) if 0.9 does not resolve or build; never both (D-004). `pyo3 = "0.30"` (0.30.1 verified; no auto-initialize, no abi3; embedding API is `Python::attach` / `Python::detach`). `zip` (deflate only), `tar` 0.4, `flate2` 1, `liblzma` 0.4 (static), `notify-rust` 4.19, `url` 2. No semver crate (yt-dlp versions are dates; compare as strings).
libsqlite3-sys: gpui declares 0.30.1 bundled in workspace deps but no gpui crate uses it; sqlx-sqlite 0.9.1 accepts `>=0.30.1,<0.41`; no `links = "sqlite3"` conflict once rusqlite is dropped.

## 3. Data flow
```
intake text --(state::Probes)--> Io pool --> engine::probe (in-process, warm interpreter)
  --> ProbeResult (typed) --> result card --> preset/formats --> state::Downloads.enqueue(JobSpec)
  --> storage insert (queued) --> scheduler pump --> engine::Backend::download --> worker child
  --> JSON lines --> JobShared atomics (latest wins) + JobEvent channel
  --> Downloads entity (apply events, DB write on phase change and <= every 5 s) --> cx.notify on a 250 ms tick (only while jobs run)
```
Tokio pattern (maple): pool created and used inside the `Io` runtime; never `block_on` on the gpui thread; `Io::load` returns a Task stored in a field, never detached. `sqlx::migrate!("../../migrations")` from `crates/storage` with a `build.rs` emitting `cargo:rerun-if-changed=../../migrations`.

## 4. Engine (hybrid, D-007)
Owner's ask is honoured: yt-dlp runs in an embedded CPython inside our own executable; no yt-dlp.exe, no stdout parsing, typed hooks.

- **Probes** (`extract_info(download=False)`) run in the main app process on a warm interpreter, with a timeout (45 s in the UI). A stuck probe is abandoned, not killed; abandoned probes are counted and logged. Python initialises lazily on its own thread right after the first frame (home exe-relative, isolated, no signal handlers, stdout and stderr replaced by a Rust `LogWriter`), then `PyEval_SaveThread` so no thread holds the GIL idle.
- **Downloads** run in a child of our own exe: `ytdlpd --engine-worker`, same binary, same engine crate, embedded Python. One child per active job. Handled at the top of `main` before gpui or window init. Cancel and pause kill the process tree.
- **Fallback** (decided at G1, recorded as D-007): the worker is the bundled `python` running `worker.py` with the identical IPC protocol, `python -I -X utf8 -u -B worker.py`; the worker dups the real stdout fd for IPC and sets `sys.stdout = sys.stderr`; site dirs added with `sys.path.insert`. No PyO3 in that case for the worker; probes then also go to a warm worker. The `Backend` trait hides the choice; state and views never change.
- **Backend trait**: `probe(url, opts) -> ProbeResult`, `download(JobSpec) -> (JobHandle, Stream<JobEvent>)`, `kill(JobHandle)`. `FakeBackend` in `state::fake` implements the same.
- **Worker env**: children spawned with `CREATE_NO_WINDOW` on Windows; exe is GUI subsystem in release (verify piped stdin/stdout work in a GUI-subsystem exe at G1). Per-job env (proxy etc.) set by the host.
- **Python search path** order: `data/python-site/<active>` (runtime-updated yt-dlp + yt-dlp-ejs) then baked `python/site` then stdlib. PyO3 initialises through `Py_InitializeEx`; set `PYTHONHOME`/`PYTHONNOUSERSITE` before the first attach, or use unsafe `pyo3::ffi` PyConfig / PEP 731 `PyInitConfig` (added in PyO3 0.30). Spike picks one.
- **Hooks (worker)**: `Hooks` `#[pyclass]` with `__call__(d)` registered as `progress_hooks`, `postprocessor_hooks`, `post_hooks`; they never raise into yt-dlp. JSON lines are written by Rust (`ipc::emit`) to the real stdout; `sys.stdout`/`sys.stderr` are `LogWriter` objects that turn stray prints into `log` events, so no fd dup is needed. Progress throttled to at most 4 per second (the last progress of a file is always sent).
- **Startup (G1, D-068)**: `PyInitConfig` (PEP 741, PyO3 0.30 ffi) with `isolated`, `use_environment=0`, `site_import=0`, `install_signal_handlers=0`, `write_bytecode=0`, `utf8_mode=1`, explicit `module_search_paths` = site updates, `python/site`, `Lib`, `DLLs`; then `PyEval_SaveThread`. Measured: init 6 to 8 ms, `import yt_dlp` 250 to 330 ms with .pyc present. `ready` is sent after `import yt_dlp`.
- **Host API** (`engine`): `EngineBackend` implements `domain::Backend` (probe on a thread with the 45 s timeout, one worker per `start`, `kill` = kill tree); `host::spawn_worker` / `TreeChild` underneath; `runtime::self_test` backs `ytdlpd --self-test [--site <dir>]`. The app `main` dispatches `--engine-worker` to `engine::worker::run_worker()` before gpui.
- **Final path**: from `post_hooks(filename)` cross-checked with `info["requested_downloads"][0]["filepath"]`; if the hook fires twice keep the last; never guessed; success is never an error because the extension differs from the probe's `ext`.
- **Opts mapping** (`engine::spec`, pure, unit-tested): Video N: `format="bv*+ba/b"`, `format_sort=["res:N"]`, `merge_output_format="mp4/mkv"`; Video best: `format="bv*+ba/b"`; Audio: `format="ba/b"` + `FFmpegExtractAudio{preferredcodec: mp3|m4a|opus, preferredquality: 0}` (best keeps native). Always: `quiet`, `noprogress`, warnings to logger, `ffmpeg_location`, `js_runtimes` (a dict: `{"deno": {"path": "C:/.../deno.exe"}}`, keys deno, node, bun, quickjs), `cachedir=cache/yt-dlp`, `paths`, `outtmpl`, `updatetime=false`, `continuedl=true`. `remote_components` left empty. Optional: `writesubtitles`/`subtitleslangs`, `EmbedThumbnail`/`FFmpegMetadata` (+ chapters), `SponsorBlock` + `ModifyChapters`, `cookiefile`/`cookiesfrombrowser`, `proxy`, `ratelimit`, `retries`. Extra user arguments are stored as `Vec<String>` (Settings text goes through `spec::split_extra_args`, shlex, quotes honoured); `to_opts` carries them in the out-of-band key `_extra_args`, the worker pops it and merges `yt_dlp.parse_options(['--ignore-config', *args])` over the opts (only keys that differ from the parse defaults, post-processors appended; an unknown flag fails the job). Pasted cookie text or an imported file is written to `<data>/cookies/<sha256-prefix>.txt` by `engine::cookies` (0600 / owner-only ACL via icacls) and referenced as `cookiefile`. `engine::preview_filename(spec, env, info)` runs the real `YoutubeDL.prepare_filename` in-process for the Settings template preview. Windows: `windowsfilenames` is yt-dlp's default; `trim_file_name` set; `overwrites=false` with `[id]` in the template.
- **Default templates**: `%(title).180B [%(id)s].%(ext)s`; playlist `%(playlist_title).80B/%(playlist_index)03d %(title).160B [%(id)s].%(ext)s`.

### IPC schema (JSON lines, D-029)
Host to worker, one line at start: `{"cmd":"download","url":"...","spec":{...yt-dlp opts as plain JSON...}}` (`spec::to_opts(JobSpec, Env)`). Later commands: `{"cmd":"exit"}` (optional 1 s graceful exit on app shutdown, then kill). No cancel command: cancel is a kill.
Worker to host:
| Line | Meaning |
|---|---|
| `{"ev":"ready"}` | interpreter up |
| `{"ev":"progress","done":..,"total":..,"speed":..,"eta":..,"frag":[i,n]}` | at most 4 per second |
| `{"ev":"phase","name":"downloading"\|"merging"\|"postprocess:<name>"}` | phase change |
| `{"ev":"path","path":..}` | final path candidate (last one wins) |
| `{"ev":"files","paths":[..]}` | partials and intermediates created (`*.part`, `*.ytdl`, `*.temp.*`, `*.fNNN.*`); extension to dev's schema, needed for cleanup |
| `{"ev":"log","level":..,"msg":..}` | yt-dlp logger lines and stray stdout/stderr; level is debug, info, warning or error. The host adds `stdout`/`stderr` levels for unparsable lines and the worker's stderr |
| `{"ev":"result","ok":true,"path":..,"paths":[..],"info":{...}}` | success. `path` is the verified final path (`requested_downloads[].filepath`, the last hook path only as fallback, a mismatch is logged); `paths` lists all outputs of a playlist; `info` is a small summary (id, title, ext, extractor_key, webpage_url, duration), never the full dict. The host maps it to `JobEvent::Path` + `Finished` |
| `{"ev":"error","code":..,"message":..,"retryable":..,"retry_after":..}` | failure, classified; `retry_after` = seconds from a 429 `Retry-After` header (absent otherwise) |
Opts that need Python objects (postprocessors, `js_runtimes`, `format_sort`) are plain JSON, converted inside the worker.

### Cancel and pause (D-020)
Kill the tree immediately. Windows: one Job Object per worker with `KILL_ON_JOB_CLOSE` (nested jobs work on Win 8+, one tree can be killed alone, and a GUI crash closes the handles so workers die too). Unix: child in its own process group (`process_group(0)`), `killpg`; Linux also `prctl(PR_SET_PDEATHSIG)`; macOS: worker exits on stdin EOF. Then Rust deletes the job's known partials from `files` events (cancel) or keeps them (pause). Resume re-runs the same `JobSpec` with `continuedl=true` (HTTP Range and `.ytdl` fragment state); a kill during an ffmpeg merge leaves `.fNNN` streams that are kept and reused. Test: spawn a child that spawns a child, kill, assert both dead (win, linux, mac CI).

### Shutdown, restart
Shutdown: send `exit`, wait up to 1 s, kill; rows become `paused`; `PRAGMA wal_checkpoint(TRUNCATE)`; must exit in < 5 s. Restart recovery: rows in probing, downloading, processing become `paused` with `status_reason='interrupted'`; optional setting `resume_on_start`.

### Wheel updates (D-011, D-013)
yt-dlp and yt-dlp-ejs update as a pair. Read the yt-dlp wheel `METADATA` `Requires-Dist: yt-dlp-ejs==X` and fetch exactly that (ejs script version is checked against yt-dlp's expected major.minor). Fetch from `https://pypi.org/pypi/<name>/json`, verify `digests.sha256`, unzip into `data/python-site/<yt-dlp-version>.tmp`, rename, run `ytdlpd --self-test --site <dir>` in a throwaway process, only then switch the `active` pointer in `app_state`; keep the previous version; an in-process import check cannot validate a new version. New version applies at the next worker spawn; probes use it after the app restarts the probe interpreter (Tools page says "Restart to use for link previews" until then; open item to confirm at G1 whether a restart is needed). First run: a yt-dlp + ejs pair baked at build time sits in `python/site` so the app works offline (D-012). Native wheels (certifi, requests, urllib3, websockets, mutagen, brotli, pycryptodomex) are baked and never updated at runtime (macOS library validation, ABI binding).

## 5. Job state machine (D-022)
Pure function `transition(status, event) -> Result<(status, effects), IllegalTransition>` in `domain`, no gpui. Statuses: `queued`, `probing`, `downloading`, `processing`, `paused`, `completed`, `failed`, `cancelled`. `probing` = worker spawned and extracting, until it reports `phase downloading`.
Events: `Start` (scheduler), `PhaseDownloading`, `PhaseProcessing`, `Finished`, `FailRetryable` (attempts below max), `FailFinal`, `Pause`, `Resume`, `Cancel`, `Retry`, `Interrupted` (startup recovery).
| From \ Event | Start | PhaseDownloading | PhaseProcessing | Finished | FailRetryable | FailFinal | Pause | Resume | Cancel | Retry | Interrupted |
|---|---|---|---|---|---|---|---|---|---|---|---|
| queued | probing | err | err | err | err | err | paused | err | cancelled | err | queued |
| probing | err | downloading | processing | completed | queued (attempts+1, next_attempt_at) | failed | paused | err | cancelled | err | paused |
| downloading | err | downloading | processing | completed | queued (same) | failed | paused | err | cancelled | err | paused |
| processing | err | downloading | processing | completed | queued (same) | failed | paused | err | cancelled | err | paused |
| paused | err | err | err | err | err | err | paused | queued | cancelled | err | paused |
| completed | err | err | err | err | err | err | err | err | err | err | completed |
| failed | err | err | err | err | err | err | err | err | err | queued (attempts+1) | failed |
| cancelled | err | err | err | err | err | err | err | err | err | queued (attempts+1) | cancelled |
Notes: `err` returns `Err`, never panics, and the table test covers all 88 pairs. Same-state entries that are legal no-ops are listed as the same status. Cancel from `queued` removes the job from the scheduler immediately (< 100 ms, never reaches the engine). Cancel from `paused` deletes known partials. "Download again" on a completed row creates a new row (not a transition). `Delete entry` is not a transition: an active row must be cancelled first. Manual Retry resets the backoff gate but keeps counting `attempts`.

## 6. Error codes and golden table (D-023)
`classify(exc_type, msg, http_status) -> EngineError{code, message_key, retryable}` is a pure function in `engine`; unknown text maps to `unknown`, retryable false. Golden file `crates/engine/tests/errors.tsv` has at least 30 real yt-dlp messages (QA adds real captures; the rows below are seeds from well-known yt-dlp wording and must be confirmed against captured output).
| Code | Retryable | Seed text (verify) | UI copy key target |
|---|---|---|---|
| `private` | no | "Private video. Sign in if you've been granted access" | "This video is private." |
| `geo` | no | "not made this video available in your country" | "Not available in your country." |
| `login` | no | "Sign in to confirm you're not a bot" | "Sign in needed. Add cookies in Settings." |
| `age_restricted` | no | "Sign in to confirm your age" | same action as login |
| `unavailable` | no | "Video unavailable" | "This video is no longer available." |
| `unsupported_url` | no | "Unsupported URL" | "This link isn't supported." |
| `format_unavailable` | no | "Requested format is not available" | "That format isn't available for this video." |
| `rate_limited` | yes | "HTTP Error 429" | "Too many requests. Trying again in N seconds." |
| `network` | yes | DNS failure, timeouts, connection reset, HTTP 5xx | "No connection. Try again." |
| `ssl` | no | "CERTIFICATE_VERIFY_FAILED" | "Secure connection failed." |
| `ffmpeg` | no | "ffmpeg not found", "ffprobe and ffmpeg not found" | "ffmpeg is missing." |
| `js_runtime_missing` | no | no supported JavaScript runtime warning | "A JavaScript runtime is needed for YouTube." |
| `disk_full` | no | OSError 28 "No space left on device" | "The disk is full." |
| `permission` | no | OSError 13 "Permission denied" | "Can't write to this folder." |
| `cookies_locked` | no | browser cookie DB locked while the browser is open | "Close the browser and try again." |
| `cancelled` | no | host-initiated | - |
| `unknown` | no | anything else | "Something went wrong. See the log." |

## 7. Backoff and attempts (D-024)
`backoff(attempt n, retry_after) = min(60, 15 * 2^(n-1))` seconds with +-20% jitter from an injected RNG; maximum 5 attempts; `Retry-After` overrides when present; all on an injected `Clock` trait so tests run in microseconds. Only retryable codes retry (`network`, `rate_limited`); `login`, `private`, `geo` never. Retry keeps the same row: status back to `queued` with `next_attempt_at` set so the wait survives a restart. A 429 puts that domain in a penalty box: its slots drop to 1 for 5 minutes (D-027); other domains are unaffected.

## 8. Python runtime layout (D-008, D-009, D-010)
Runtime: python-build-standalone (PBS) CPython 3.14.10, `install_only_stripped`, release tag 20261110 (verified, with `SHA256SUMS`). File `cpython-3.14.10+20261110-<triple>-install_only_stripped.tar.gz`, triples `x86_64|aarch64-pc-windows-msvc`, `x86_64|aarch64-apple-darwin`, `x86_64|aarch64-unknown-linux-gnu`. `setup.ts` resolves the tag through `releases/latest` plus `SHA256SUMS` and pins tag + sha in `runtime.lock.json` for reproducible CI.
| OS | Layout beside the app |
|---|---|
| Windows | `python314.dll` and `python3.dll` next to `ytdlpd.exe` (app directory is first in the DLL search; no /DELAYLOAD); `python/{Lib,DLLs,site}` |
| Linux | rpath `$ORIGIN/python/lib` (dev: absolute rpath to `runtime/lib` via build.rs `cargo:rustc-link-arg`); install layout `../lib/ytdlpd/python` |
| macOS | `Contents/Frameworks` + rpath `@executable_path/../Frameworks`; PBS install name is already `@rpath/libpython3.14.dylib` (verified, D-121), no install_name_tool step; codesign the runtime dir for notarisation |
Linked as full libpython (not abi3) through `PYO3_CONFIG_FILE` written by `setup.ts` (keys: `implementation=CPython`, `version=3.14`, `shared=true`, `abi3=false`, `lib_name=python314` on Windows or `python3.14` on unix, `lib_dir=<abs>` (`<runtime>/libs` on Windows, `<runtime>/lib` on unix), `executable=<abs python>`, `pointer_width=64`, `build_flags=`, `suppress_build_script_link_lines=false`). `apps/desktop/.cargo/config.toml`: `[env] PYO3_CONFIG_FILE = { value = "runtime/pyo3.cfg", relative = true }`.
Pruning at setup (lists in setup.ts): `Lib/test`, `Lib/*/tests`, `idlelib`, `tkinter`, `turtledemo`, `turtle.py`, `ensurepip`, `pydoc_data`, `venv`, `tcl/`, `DLLs/_tkinter.pyd tcl86t.dll tk86t.dll`, `include/`, `share/`, pip and setuptools, `_test*.pyd`, `_ctypes_test`. `__pycache__` is kept and `compileall` runs once, because `write_bytecode` is off at runtime and without .pyc every worker start recompiles yt-dlp (+0.6 s). Keep `_decimal`. A test imports every module yt-dlp needs after pruning (ssl, zlib, bz2, lzma, ctypes, json, xml, http, unicodedata, hashlib). `libs/*.lib` are needed to link (embedding), so they stay in the dev runtime and are dropped from the shipped bundle. Runtime size (setup.ts records `size_mb` per triple in `runtime.lock.json`):
| Triple | PBS download (release asset) | Installed, pruned, with baked deps | Compressed tar.gz of the shipped tree |
|---|---|---|---|
| x86_64-pc-windows-msvc | 22.6 MB | 80 MB (about 53 MB without .pyc; yt_dlp alone is 25 MB of it) | 28.4 MB (measured) |
| aarch64-pc-windows-msvc | 21.6 MB | not measured (needs a win-arm64 runner) | not measured |
| aarch64-apple-darwin | 25.7 MB | not measured (needs a macOS runner) | not measured |
| x86_64-apple-darwin | 25.9 MB | not measured | not measured |
| x86_64-unknown-linux-gnu | 35.2 MB | not measured (needs a Linux runner) | not measured |
| aarch64-unknown-linux-gnu | 29.4 MB | not measured | not measured |
Download sizes are the asset sizes of tag 20261110 (the earlier 51.5 MB figure was wrong). Run `bun scripts/setup.ts --ship` on each CI runner to fill the rest.
Baked `python/site` (pip at build): certifi (mandatory: PBS has no OS cert store hookup), requests, urllib3, websockets, mutagen, brotli, pycryptodomex, pinned yt-dlp and yt-dlp-ejs. Later: curl_cffi (abi3, about 15 MB).
Dev setup: `bun run setup` once (`apps/desktop/package.json` script `setup` = `bun scripts/setup.ts`) downloads, verifies, prunes, pip-installs, writes `runtime/pyo3.cfg`; the bin `build.rs` copies the Python DLLs into `target/<profile>/` (unix: dev rpath); the engine crate's `build.rs` does the same for its own tests and examples and bakes the dev runtime path (`YTDLPD_DEV_PYTHON_HOME`). `.cargo/config.toml` sets `PYO3_CONFIG_FILE=runtime/pyo3.cfg` (relative). `runtime/` is gitignored. `bun run dev` without setup works with `--no-default-features` (fake backend).
`tools` can repair a missing or corrupt runtime by downloading from PBS (D-010); bundled stays the default.

## 9. Tool fetch matrix (D-015, D-016, D-017, D-018)
Resolution per tool: settings path, then PATH, then well-known dirs (macOS `/opt/homebrew/bin`, `/usr/local/bin` since Finder-launched apps lack the shell PATH; Windows `%LOCALAPPDATA%\Microsoft\WinGet\Links`, scoop shims), then managed install in `data/tools/<tool>/<version>/`. Every candidate is version-probed (`-version`) before use; the DB `tools` row is a startup cache and is re-probed.
| Tool | OS/arch | Source and asset pattern | Archive | Verify |
|---|---|---|---|---|
| ffmpeg + ffprobe | win x64, win arm64, linux x64, linux arm64 | BtbN/FFmpeg-Builds release tag `latest`; list release assets and match `^ffmpeg-n(\d+)\.(\d+)-latest-(win64\|winarm64\|linux64\|linuxarm64)-gpl-\d+\.\d+\.(zip\|tar\.xz)$` (the `-gpl-shared-` variants have an extra token and are excluded); choose the highest n.m (n8.1 and n9.0 coexist); static builds hold `bin/ffmpeg(.exe)` and `ffprobe` | zip (win), tar.xz (linux) | `checksums.sha256` in the same release (`<sha>  <asset>`) |
| ffmpeg, ffprobe | macOS arm64, x64 | `https://ffmpeg.martin-riedl.de/redirect/latest/macos/{arm64\|amd64}/release/{ffmpeg\|ffprobe}.zip`, 307 to `/download/macos/<arch>/<epoch>_<ver>/<tool>.zip` | zip, two separate downloads | follow the redirect first, then fetch `<final>.sha256` (the redirect URL's `.sha256` is 404) |
| deno | win x64, win arm64, mac x64/arm64, linux x64/arm64 | `https://github.com/denoland/deno/releases/latest/download/deno-<triple>.zip` with triples `{x86_64,aarch64}-pc-windows-msvc` (aarch64 exists), `{x86_64,aarch64}-apple-darwin`, `{x86_64,aarch64}-unknown-linux-gnu` | zip | `<asset>.zip.sha256sum` |
| JS runtime (any) | all | detect first: deno >= 2.3, node >= 22, bun, quickjs on PATH, passed as `js_runtimes` | - | version probe |
| yt-dlp, yt-dlp-ejs | all | PyPI JSON `https://pypi.org/pypi/<name>/json` | wheel (zip) | `digests.sha256` |
| Python runtime | all | PBS as section 8 | tar.gz | `SHA256SUMS` |
Fetch mechanics (pattern from maple `updates::download`): stream to `cache/tools/` computing sha256, verify, unpack to a temp sibling dir, atomic rename into place with retry (5 tries, 200 ms backoff for Defender locks), set the exec bit on unix, best-effort `xattr -d com.apple.quarantine` on macOS, never run the tool during unpack. Interrupted downloads are discarded, not resumed. Mirror base URLs overridable with `YTDLPD_TOOLS_MIRROR` for tests (D-050). License: gpl builds by default (app is GPL); `lgpl` selectable.

## 10. First-run flow
1. Launch: first frame in < 800 ms with no Python yet. `app_state.first_run_done` absent means Welcome ("Getting ready").
2. In parallel: Python init on its thread (bundled; corrupt runtime shows an error with repair through `tools`), `tools::detect` for all tools.
3. List every tool with its status; missing ones download with progress; "Use my own ffmpeg..." opens a path picker (non-ffmpeg executables rejected).
4. yt-dlp is baked, so Continue works offline for yt-dlp; ffmpeg missing offline gives "No connection. Try again" and an explicit "Continue without ffmpeg" (merge and convert presets then disabled with a reason). The runtime update check for yt-dlp is not on the critical path.
5. Continue goes to Downloads with intake focused. Later launches skip Welcome; missing tools show a `Notice` on Downloads.

## 11. Database (D-004, D-032, D-033)
Connect options: `journal_mode(Wal)`, `synchronous(Normal)`, `foreign_keys(true)`, `busy_timeout(5000)`, `create_if_missing`; pool max 4. Queries are runtime-checked `sqlx::query_as::<_, Row>()` with `#[derive(FromRow)]` (no `query!`, no `DATABASE_URL`, no offline mode). Enums live in `domain`, mapped with `sqlx::Type` and `rename_all`. Forward-only migrations; pre-1.0 `0001` may be edited freely (documented); after the 1.0 tag shipped migrations are never edited. Before migrating, copy the DB to `ytdlpd.db.bak-<app version>` once per version bump. Tests: migrate empty in-memory DB, and the previous release's snapshot.
`migrations/0001_init.sql`:
```sql
CREATE TABLE playlists (
  id INTEGER PRIMARY KEY,
  url TEXT NOT NULL,
  extractor TEXT,
  remote_id TEXT,                      -- extractor's playlist id
  title TEXT, uploader TEXT,
  entry_count INTEGER,
  created_at INTEGER NOT NULL
);

CREATE TABLE downloads (
  id INTEGER PRIMARY KEY,              -- also the call number shown in the UI
  url TEXT NOT NULL,                   -- as entered
  webpage_url TEXT,
  extractor TEXT,                      -- yt-dlp extractor_key
  video_id TEXT,
  domain TEXT NOT NULL,                -- normalised host, per-domain concurrency
  title TEXT, uploader TEXT, duration_s REAL,
  thumbnail_url TEXT, thumbnail_path TEXT,
  kind TEXT NOT NULL CHECK (kind IN ('video','audio')),
  preset TEXT NOT NULL,                -- 'video:1080' | 'audio:mp3' | 'custom'
  format_spec TEXT NOT NULL,
  format_label TEXT,                   -- what the row shows, e.g. '1080p mp4'
  options_json TEXT NOT NULL,          -- full JobSpec snapshot; retry reproduces it
  status TEXT NOT NULL CHECK (status IN ('queued','probing','downloading','processing','paused','completed','failed','cancelled')),
  status_reason TEXT,                  -- e.g. 'interrupted'
  phase TEXT,                          -- 'merging', 'postprocess:ExtractAudio', ...
  progress REAL NOT NULL DEFAULT 0,
  bytes_done INTEGER, bytes_total INTEGER,
  output_dir TEXT,                     -- final directory at download time
  file_path TEXT, file_size INTEGER,
  file_missing INTEGER NOT NULL DEFAULT 0,
  error_code TEXT, error_message TEXT,
  attempts INTEGER NOT NULL DEFAULT 0,
  next_attempt_at INTEGER,             -- backoff gate, survives restart
  playlist_id INTEGER REFERENCES playlists(id) ON DELETE SET NULL,
  playlist_index INTEGER,
  position REAL NOT NULL,              -- queue order, reorderable
  created_at INTEGER NOT NULL, started_at INTEGER, finished_at INTEGER, updated_at INTEGER NOT NULL
);
CREATE INDEX downloads_created ON downloads (created_at DESC);
CREATE INDEX downloads_queue   ON downloads (status, position);
CREATE INDEX downloads_media   ON downloads (extractor, video_id);  -- "already downloaded" hint

CREATE TABLE download_meta (
  download_id INTEGER PRIMARY KEY REFERENCES downloads(id) ON DELETE CASCADE,
  info_json TEXT NOT NULL              -- trimmed info dict: no heatmap or thumbnails, WITH formats (re-opens Choose formats)
);

CREATE TABLE download_logs (
  id INTEGER PRIMARY KEY,
  download_id INTEGER NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
  at INTEGER NOT NULL, level TEXT NOT NULL, message TEXT NOT NULL
);
CREATE INDEX download_logs_job ON download_logs (download_id, id);   -- trimmed to the last 500 per job when the job finishes

CREATE TABLE tools (
  name TEXT PRIMARY KEY,
  source TEXT NOT NULL CHECK (source IN ('managed','path','custom')),
  path TEXT NOT NULL, version TEXT, sha256 TEXT, installed_at INTEGER, checked_at INTEGER
);

CREATE TABLE app_state (key TEXT PRIMARY KEY, value TEXT NOT NULL);   -- first_run_done, ytdlp_active_site, ...
```
Six tables. `list_summaries` never selects `options_json` or `info_json` (a test asserts it). Filter and search run in memory on `DownloadSummary` with precomputed lowercase `title_lc` and `uploader_lc` (about 10 MB at 50k rows; no FTS5, D-005). Settings are in settings.json, not the DB. Cookie files are copied into `data/cookies/`.

## 12. settings.json (hot reload, maple pattern; D-006)
Located in the config dir; `notify` watcher with debounced save; invalid JSON keeps the last good value and logs. Field names are snake_case (plan values, owner may rename before 1.0). Every field must be referenced outside the settings crate (`tests/settings_used.rs`, allowlist file for deliberate future ones).
| Key | Default |
|---|---|
| `theme` | `"dark"` (`system`, `dark`, `light`, or a maple theme id) |
| `language` | `"en-US"` |
| `compact_mode` | `false` |
| `font_size` | `13` |
| `output_dir` | OS Videos dir joined with `ytdlpd` |
| `outtmpl` | `%(title).180B [%(id)s].%(ext)s` |
| `playlist_outtmpl` | `%(playlist_title).80B/%(playlist_index)03d %(title).160B [%(id)s].%(ext)s` |
| `concurrent_global` / `concurrent_per_domain` | `3` / `2` |
| `max_attempts` | `5` |
| `resume_on_start` | `false` |
| `last_preset` | `"video:best"` |
| `embed_thumbnail` / `embed_metadata` / `embed_chapters` | `false` / `true` / `true` |
| `subtitles` | `{enabled:false, langs:["en"], embed:false}` |
| `sponsorblock` | `{mode:"off", categories:[]}` (`off`, `mark`, `remove`) |
| `proxy` / `rate_limit` | `null` / `null` |
| `cookies` | `{mode:"none", file:null, browser:null}` (`none`, `file`, `browser`) |
| `tool_paths` | `{ffmpeg:null, ffprobe:null, deno:null}` |
| `ffmpeg_flavor` | `"gpl"` (`lgpl` selectable) |
| `ytdlp_auto_check` | `true` (daily check, manual button always) |
| `clipboard_watch` / `clipboard_auto_add` | `true` / `false` |
| `notifications` | `true` |
| `close_to_tray` / `start_minimized` | `false` / `false` |
| `window` | `{w,h,x,y}` remembered |
| `detail_panel_width` | `360` |

## 13. Data directories
Dev builds use `ytdlpd.dev`. Never literal paths: `storage::app_dir_name()`.
| Kind | Windows | macOS | Linux |
|---|---|---|---|
| Config (settings.json) | `%APPDATA%\ytdlpd` | `~/Library/Application Support/ytdlpd` | `$XDG_CONFIG_HOME/ytdlpd` |
| Data (`ytdlpd.db`, `tools/`, `python-site/`, `cookies/`, `logs/`) | `%LOCALAPPDATA%\ytdlpd` | `~/Library/Application Support/ytdlpd` | `$XDG_DATA_HOME/ytdlpd` |
| Cache (`tools/` downloads, `yt-dlp/`, thumbnails) | `%LOCALAPPDATA%\ytdlpd\cache` | `~/Library/Caches/ytdlpd` | `$XDG_CACHE_HOME/ytdlpd` |
`--data-dir <dir>` overrides all three for tests and screenshots.

## 14. Logging
`log` + `env_logger` (as maple), subsystem-prefixed lines; app log `logs/ytdlpd.log` rotated 5 x 2 MB (confirm in lane D); `RUST_LOG=ytdlpd=debug,engine=trace`. yt-dlp lines are forwarded through the engine logger with the job id into `download_logs`. Startup logs `startup: first_frame ms=...` and Python init timing.

## 15. Security (D-018, D-031)
- Cookies are secrets: cookie files are copied to `data/cookies/` with a restricted ACL (`icacls /inheritance:r` plus the user on Windows, mode 0600 on unix), never stored in settings.json, never logged. Log scrubbing removes `Cookie:` and `Authorization` values and URL query values for keys like `token`, `key`, `sig` before lines reach `download_logs` or the log file (test: values never appear).
- Chrome on Windows: app-bound encryption breaks Chrome cookie extraction since Chrome 127; the UI recommends Firefox or a cookies file (copy rule in design.md). Cookies from a running browser may be locked: `cookies_locked`.
- Archive hardening: reject entries with `..` or absolute paths (zip-slip), reject unpacked size beyond the declared entry sizes, extract to a temp dir first.
- Checksums fail closed: on mismatch delete the temp file, show expected and actual prefixes, never retry silently more than once, never fall back to an unverified binary; when the checksum file is missing (for example BtbN `latest` being republished) fail with a Try again button.
- Deep link and CLI input: only http and https URLs accepted; `javascript:`, `file:`, `ftp:` rejected; a deep link never starts a download without the normal intake validation.
- Windows: long paths (exe manifest `longPathAware`, Tools page notice if registry `LongPathsEnabled` is off), GUI subsystem, all children `CREATE_NO_WINDOW`, unsigned-exe SmartScreen/AV warnings documented, signing later.
- App-level `certifi` is the only cert source for yt-dlp in the embedded runtime; `nocheckcertificate` is not exposed in v1.
