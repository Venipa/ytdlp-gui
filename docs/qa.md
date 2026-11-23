# ytdlpd QA

Per-phase exit checklists live in `plan.md` (they are the acceptance gates). This file holds the strategy, fixtures, regression table, matrix, live smoke list and perf methods. Primary platform: Windows 11 x64 (the dev box). macOS and Linux: CI unit tests and compile checks, and owner-run passes before a release candidate (D-059).

## 1. Strategy
| Layer | What | Where |
|---|---|---|
| Unit (pure) | `engine::spec` opts mapping (golden `serde_json::Value`), `engine::classify` golden table, `domain` transition table (all 88 status x event pairs), backoff on injected `Clock`, filter token parser, `tools` matrix selection and checksum parsing, storage migrations on in-memory DB, `split_urls`, deep link parser, outtmpl preview | each crate |
| Engine integration (deterministic) | Rust fixture server plus yt-dlp's generic extractor: progress, final path, cancel mid-download, pause and resume by Range, merge, HLS, AES HLS, 500-entry playlist through a test-only info extractor | `crates/engine/tests` |
| State | fake engine behind the `Backend` trait; scheduler with fake clock; one real-engine integration test | `crates/state/tests` |
| UI headless | gpui `test-support` `TestAppContext`: roles, behaviour, a11y tree (extend maple `ui/tests/a11y_tree.rs`); no pixels | `ui`, `views` |
| UI visual | `scripts/screenshot.ps1` plus `--demo <scene>`; lead reviews against `design.md` | manual |
| Live smoke | opt-in, replaceable URLs | section 5 |
| Manual per phase | lead script from `plan.md` checklists | Windows |
CI (D-052, adapted from maple `ci.yml`): ubuntu `fmt` + `clippy --workspace --all-targets --locked -- -D warnings` and `cargo nextest run --workspace --locked` (dev profile opt-level 0 for ui/views); windows-latest nextest (not clippy only: primary platform and Python embedding are OS specific); macos-15 clippy. Live smoke only through a weekly `workflow_dispatch` job allowed to fail.

## 2. Fixture server and fixtures (D-051)
`python -m http.server` is not enough (no Range support). The fixture server is Rust (`std::net::TcpListener` or `tiny_http` as dev-dependency), `cargo run -p engine --example fixture_server`, owned by Lane B. Routes and switches:
- static files with Range and 206; `Content-Type: video/mp4`, `application/vnd.apple.mpegurl`, `video/mp2t`
- `Retry-After` 429 for the first N requests
- per-request throttle (sleep per 8 KB chunk) so cancel and pause have a window
- kill-the-connection after X bytes
- `Content-Disposition` and redirect routes
- `slow_extract` route (manifest stalling 30 s)
- header log (echoes `Cookie:` so cookie tests can assert)
- recording HTTP proxy mode (asserts CONNECT or absolute-URI requests)
- mirror mode for `YTDLPD_TOOLS_MIRROR`
Verified against stock yt-dlp: a direct `.mp4` URL goes through the generic extractor (no localhost block); `.m3u8` gives `[hlsnative]`, a `hls_index.mp4` file and `[FixupM3u8]` via ffmpeg; fragment progress `(frag n/N)`; AES-128 works if the fixture adds `#EXT-X-KEY`.
Committed fixtures, total <= 300 KB in `crates/engine/tests/fixtures/`, with the generator `scripts/make-fixtures.ps1` and `.sh` (outputs committed so CI needs no ffmpeg for pure engine tests; merge and convert tests need ffmpeg: managed one or `YTDLPD_TEST_FFMPEG`):
```
ffmpeg -y -f lavfi -i testsrc=size=160x90:rate=10:duration=6 -f lavfi -i sine=frequency=440:duration=6 -c:v libx264 -preset veryfast -crf 35 -g 10 -pix_fmt yuv420p -c:a aac -b:a 24k -movflags +faststart sample.mp4   # about 30 KB
ffmpeg -y -i sample.mp4 -c copy -f hls -hls_time 2 -hls_playlist_type vod -hls_segment_filename 'hls/seg%d.ts' hls/index.m3u8   # 3 segments; -g 10 keeps -hls_time from collapsing to one segment
ffmpeg -y -i sample.mp4 -c copy -f hls -hls_time 2 -hls_key_info_file key.info ... aes/index.m3u8   # encrypted variant
ffmpeg -y -f lavfi -i sine=frequency=440:duration=6 -c:a libmp3lame -b:a 32k sample.mp3            # audio source for convert tests
ffmpeg -y -f lavfi -i testsrc=size=160x90:rate=10:duration=6 -an -c:v libvpx-vp9 -b:v 40k video_only.webm   # for merge tests
```
Merge test: serve video-only and audio-only files through a tiny local HLS master playlist or an info dict with two `requested_formats` through `YoutubeDL.process_ie_result`; assert `[Merger]` runs and the final path ends `.mp4` or `.mkv`. Large-file tests (resume, disk full, 429) are generated at test time under `F:\tmp` (not committed): a 40 MB zero-filled `application/octet-stream` served by the Rust server. 500-entry playlist: a test-only `ydl.add_info_extractor` (Python snippet embedded in the test) yields 500 url entries pointing at `/sample.mp4`; no network.

## 3. Old-app regression tests
Each is a named test; Phase in brackets.
| Old bug | Test (crate) | Assertion |
|---|---|---|
| Cancel while queued does nothing [3] | `state::cancel_queued_removes_immediately` | concurrency 1, job A running, cancel queued B: status `cancelled` in < 100 ms, DB row updated, B never reaches the engine (fake call counter == 1), `scheduler.pending_len()==0` |
| Waiters never removed, processing-URLs leak [3] | `state::scheduler_no_leaks` | 200 random enqueue/cancel/complete/fail ops (seeded); at the end running==0, pending==0, `in_flight_urls.is_empty()`; re-adding the same URL after fail or cancel is allowed |
| Final path guessed, wrong ext after merge or convert [1] | `engine::final_path_after_merge`, `engine::final_path_after_extract_audio` | video-only + audio-only fixtures give `.mp4`; mp3 fixture with `audio:opus` gives `.opus`; reported path exists and equals `requested_downloads[0].filepath`; a hook firing `path` twice keeps the last; success never marked error when ext differs from the probe's `ext` |
| Retry creates a new row [3] | `state::retry_same_row` | after fail, `retry(id)`: same id, `attempts==1`, row count unchanged, `options_json` identical, status `queued` |
| Sequential metadata extraction [2] | `state::probes_parallel_up_to_3` | 6 probes against a 300 ms delayed server: wall time < 900 ms, max in flight == 3 |
| Clipboard wipe [6] | `state::clipboard_never_writes` | the clipboard trait has no write method (compile time) plus manual: copy text, watcher triggers, text still on the clipboard |
| Clipboard stateful global regex, no validation, leaked listeners [6] | `state::clipboard_candidates` | same string checked twice gives the same result (the `/g` lastIndex bug); table: `https://a.b/c`, `http://x`, multi-line, trailing punctuation, `javascript:alert(1)`, `file:///c:/x`, `ftp://`; dedup against queued URLs; listener count == 1 after 100 focus events (test spy) |
| Re-render storm, whole-row pushes [3] | `state::notify_rate` | 3 jobs emit 10 000 progress calls in 2 s: `cx.notify` on `Downloads` <= 4 per second (<= 10 in 2 s); DB `UPDATE` count <= 1 per 5 s per job plus phase changes (statement counter via sqlx `after_connect`/log) |
| Full table reload per event [3] | `storage::summaries_no_blobs` | `list_summaries` SQL never selects `options_json` or `info_json`; 10 000 rows load in < 100 ms (release) |
| Dead settings [0b] | `settings_used` | walks every `AppSettings` field and asserts it is referenced by name outside the settings crate (allowlist file) |
| cliargs split on spaces [4] | `engine::spec::extra_args_shlex` | extra args stored as `Vec<String>`; `--user-agent "A B"` and `--output "C:\My Videos\%(title)s.%(ext)s"` keep spaces; empty string and unbalanced quote rejected with a typed error |
| Startup marks active rows cancelled [3] | `state::restart_recovery` | rows in probing/downloading/processing become `paused` with reason `interrupted`; completed untouched |
| Inverted updater condition [1] | `tools::needs_update` | installed older/equal/newer vs latest; unparsable gives no update and no panic |
| Insecure settings [4] | `state::cookies_stored_private` | cookie files copied to `data/cookies/`; not in settings.json; URL query `token=` and cookie values never appear in `download_logs` (log scrub test) |
| Textarea multi-URL [2] | `state::split_urls` | 3 lines with blanks and CRLF give 3 URLs, duplicates removed |
Additional named tests required by decisions: `domain::transition_table` (all 88 pairs), `engine::classify_golden` (>= 30 rows), `domain::backoff_spec` (fake clock, jitter bounds, `Retry-After`), `tools::checksum_fail_closed`, `tools::zip_slip_rejected`, `tools::wheel_rollback` (truncated zip, missing `yt_dlp/__init__.py`, syntax error: active pointer unchanged, error toast), `engine::kill_tree`, `engine::worker_isolation` (8 concurrent downloads + 3 probes x 20 runs, unique token per job in file name and progress), `state::shutdown_clean` (close while downloading: exit < 5 s, rows `paused`, `.part` present, no DB write after the closing transaction, WAL checkpointed), `state::domain_penalty_box` (429 reduces that domain to 1 slot for 5 minutes, others unaffected), `ui::contrast` (WCAG test over tokens), `views::catalog_keys` (every `t!` key exists in en-US).

## 4. Test matrix
| Axis | Cases | Where |
|---|---|---|
| First run | online; offline (yt-dlp baked so Continue works, ffmpeg shows "No connection. Try again", retry works after reconnect); behind proxy (`HTTPS_PROXY` with the recording proxy); PyPI blocked but GitHub reachable | manual + `tools` tests with fixture server as mirror (`YTDLPD_TOOLS_MIRROR`) |
| Tools | all pre-installed (nothing downloaded); none installed; only ffmpeg; PATH ffmpeg broken (exit 1); corrupt managed ffmpeg (re-download); custom path to a non-ffmpeg exe rejected | `tools` unit + manual |
| Paths and titles | `日本語のタイトル`, emoji, RTL, `CON`, `NUL`, `COM1.mp4`, trailing dot or space, `a:b`, 300-char title, MAX_PATH (output dir at 240 chars), UNC path, read-only output dir, non-ASCII Windows user name | fixture server serving `%(title)s` through a test extractor; assert file exists, DB path round-trips, Show in folder works; `windowsfilenames` default asserted; long-path manifest and `\\?\` handling |
| Failure | disk full (20 MB VHD via `diskpart`, tmpfs on linux; code `disk_full`, `.part` kept or cleaned per design, no DB corruption); network drop mid-download (server kills socket) then retry resumes with Range (bytes_done never regresses, final sha256 equals source); app killed (`taskkill /F`) then next start row `paused`, resume completes, `integrity_check` ok, WAL replayed | integration + manual |
| Scale | 500-entry playlist (UI > 30 fps, memory +< 50 MB, flat and lazy probe); 10k-row history (cold start list < 300 ms, filter keystroke < 50 ms, scroll fps in section 6) | seed via `cargo run -p storage --example seed -- 10000` |
| Limits | global 1, 3, 8; per-domain 1, 2; mixed domains; changing limits while running (lowering does not kill, raising starts waiting jobs); 429 backoff 15 s to 60 s on fake clock, domain throttled, other domains unaffected; `Retry-After` honoured | scheduler tests with injected clock |
| Engine | cancel during extraction (`slow_extract`) returns in < 4 s and kills the worker; kill during ffmpeg merge leaves `.fNNN`, resume reuses them; worker start from a GUI-subsystem exe with piped stdio | integration |

## 5. Live smoke (opt-in: `cargo test -p engine --test live -- --ignored`, env `YTDLPD_LIVE=1`; never in PR CI)
List lives in `crates/engine/tests/live_urls.toml`: one URL, expected `extractor_key`, minimum duration. A failing URL means "replace the URL" first, not a bug. Each: probe, assert extractor_key and non-empty title, download the smallest format (`format_sort res:144`), assert `file_path` exists, size > 0, ffprobe duration within 1 s of the probe.
| Type | Candidate (replaceable) |
|---|---|
| YouTube single | `https://www.youtube.com/watch?v=jNQXAC9IVRw` ("Me at the zoo", 19 s) |
| YouTube playlist | small public playlist (<= 5 videos) chosen by the owner; placeholder: a YouTube Music "Topic" album playlist |
| Vimeo | `https://vimeo.com/22439234` (yt-dlp's own test video) |
| SoundCloud | a short CC track from `_TESTS` in yt-dlp `soundcloud.py` |
| Direct file | `https://download.samplelib.com/mp4/sample-5s.mp4` (replaceable) |

## 6. Performance measurement (release build, win x64; budgets are in `product.md`)
| Metric | Method |
|---|---|
| First frame | log `startup: first_frame ms=` from an `Instant` at `main` to the first frame callback; script runs 10 times and takes the median; assert in log order that Python is not initialised yet |
| RSS idle | `Get-Process ytdlpd`, `WorkingSet64`, after 30 s idle on Downloads; private bytes via `typeperf "\Process(ytdlpd)\Private Bytes"` |
| RSS with 3 downloads | fixture server throttled to 1 MB/s x 3 for 60 s, sample every 5 s; check no growth trend over 10 min |
| CPU | `Get-Counter '\Process(ytdlpd)\% Processor Time' -SampleInterval 2 -MaxSamples 15` divided by core count; test: no `cx.notify` for 2 s when idle (tick timer cancelled with no jobs) |
| List scroll fps | `--frame-stats` prints frame time p50 and p95 each second (verify gpui exposes frame timing); scripted wheel events (SendInput from PowerShell) for 10 s; fallback PresentMon `PresentMon.exe --process_name ytdlpd.exe --timed 10` |
| DB writes | sqlx `log_statements` at trace plus a test counter (`notify_rate`); WAL size check |
| Download overhead | A/B script: engine vs `yt-dlp.exe` CLI on the 40 MB fixture over loopback, same server |
| Python init | log timing on the py-init thread plus frame stats during it |
| Worker cost (G1 spike) | spawn 3 workers, record RSS and time to `ready` |

## 7. Rules for the lead
An agent report "done" is a claim. Re-run the check commands (plan.md "Self-verification"), run the phase manual script, look at one fresh screenshot per UI change, and quote the output before marking an item `done`. Releases need an owner-run macOS and Linux pass.
