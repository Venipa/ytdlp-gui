# ytdlpd plan

Status: planning complete, nothing built. Last updated: 2026-11-23. Current phase: none started (next: Phase 0a).
Decisions are in `decisions.md` (D-nnn). Product in `product.md`, look in `design.md`, internals in `architecture.md`, tests in `qa.md`.

## How to update this file
- Status values: `open`, `in progress`, `done`. Nothing else.
- A lane sets its own items to `in progress` when it starts and writes `review` in Ref when it asks for review. Only the lead sets `done`, after re-running the phase checks and looking at fresh screenshots (see "Self-verification").
- Ref column: commit sha (`abc1234`) and/or path of the file that proves it (test name, example, screenshot). Planned paths are shown in `code` until real.
- When a phase is done, tick its exit checklist in place (`[x]`) and add the date. Do not delete rows; add new rows at the end of the phase table and say why in Ref.
- A change in behaviour updates the matching doc section in the same commit. Commit messages: Conventional Commits, no body.
- Every UI pick made during the build is recorded in `decisions.md` with the screenshot path; at the end of the build the lead produces one screenshot review page for the owner.

## Gates
| Gate | After | Passes when | Status | Ref |
|---|---|---|---|---|
| G0 | Phase 0a | `bun run dev` opens a window with real fonts and tokens; fmt, clippy, tests green; git initialised; stubs for every crate exist | in progress | review: shot F:/tmp/ytdlpd-rs/shots/g0.png, commits 6bbcd21..ca9453a |
| G1 | Phase 0b | `domain` frozen as tag `contract-1`; engine spike (Windows): fixture file downloaded through the embedded interpreter in a `--engine-worker` child with progress and kill-tree cancel, time-boxed to 3 working days; `tools` ensure works. Failure on any of link/load, hook stability, kill-tree cancel, or worker start from a GUI-subsystem exe means switching to the worker.py fallback (D-007) and recording it | open | - |
| G2 | Phase 2 | Vertical slice on real backends: paste, probe, download, open file; history survives restart | open | - |
| G3 | Phase 4 | All "must" features work end to end on Windows; this is the 1.0-must line | open | - |
| G4 | Phase 6 | Platform features and polish done; perf budgets measured | open | - |
| G5 | Phase 7 | Installers build in CI on all 6 targets and pass `--self-test` on a clean runner | open | - |

No phase starts before the previous gate passes, except lane work explicitly marked parallel.

## Phase 0a: skeleton (lead plus Lane A, about 1 to 2 sessions, mechanical copy)
| Item | Status | Ref |
|---|---|---|
| `git init`, `.gitignore` (runtime/, target/, node_modules), LICENSE GPL-3.0-or-later, root `package.json` (bun workspaces `apps/*`, `packages/*`; scripts dev/build/check/setup/dev:hot forward to `@ytdlpd/desktop`) | in progress | `package.json`, review |
| `packages/gpui` = copy of maple `packages/gpui` (sonorahq fork @17c64ae, own workspace + toolchain) | in progress | `packages/gpui`, review |
| `apps/desktop/Cargo.toml`: resolver 3, edition 2024, members `crates/*`, ALL workspace deps pre-declared (architecture.md section 2), lints and profiles copied from maple, `rust-toolchain.toml` 1.98.1 | in progress | `apps/desktop/Cargo.toml`, review |
| Copy clean crates verbatim: `ui`, `embed`, `icons`, `i18n`; keep `common.ftl`/`menus.ftl` and the keys `ui` reads; fix `ui/examples/icons.rs` font path and `ui` brand/shader id text | in progress | `crates/{ui,embed,icons,i18n}`, review |
| `router`: keep `navigation.rs`, `link.rs`; rewrite `Destination` (Welcome, Downloads, Logs, Settings(SettingsPage::{General, Downloads, Formats, Network, Tools, Appearance, About})), frozen at end of 0a | in progress | `crates/router`, review |
| `input`: keep text-input parts, rename action namespace to `ytdlpd`, replace action list and `shortcuts.rs` tables with the keyboard map in design.md | in progress | `crates/input`, review |
| Empty stub crates so every lane starts from one tree: `domain`, `storage`, `state`, `engine`, `tools`, `cli`, `views`, `ytdlpd` (bin, window, assets, Plex fonts, env_logger, no tray) | in progress | `crates/*`, review |
| `ytdlpd` bin: `--data-dir`, `--window-size` flags (D-049); `fn build_app(cx) -> Backend` hook exposed for lane C | in progress | `crates/ytdlpd/src/main.rs`, review |
| `views`: Workspace shell with title-bar nav and three empty pages using `Vacancy` | in progress | `crates/views`, review |
| `.github/workflows/ci.yml` adapted from maple: ubuntu fmt + clippy + nextest, windows nextest (primary platform), macos clippy | in progress | `.github/workflows/ci.yml`, review |
| `apps/desktop/CLAUDE.md` (maple rules adapted, dep direction, lanes, handoff rule, commands), root `README.md`, `CHANGELOG.md` | in progress | `apps/desktop/CLAUDE.md`, review |
| docs/*.md committed (these files) | in progress | `docs/` d0c5c50, review |

Exit checklist (G0):
- [ ] `bun run check` clean; `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo nextest run --workspace --locked`
- [ ] `bun run dev` shows a window; screenshot shows Plex Sans (not Inter), nav switches 3 pages
- [ ] ported `ui/tests/a11y_tree.rs` passes; `Destination` match is exhaustive
- [ ] no Python present on the machine and the workspace still builds (`--no-default-features` on the bin, D-043)

## Phase 0b: contract, themes, spikes (all four lanes in parallel after G0)
| Item | Status | Ref |
|---|---|---|
| Lane C: `domain` crate: `DownloadId`, `DownloadSummary`, `DownloadStatus`, `JobSpec`, `Preset`, `ProbeResult`, `EngineError`, `ToolStatus`, filter AST, `transition()` with its full table test | review | `crates/domain` |
| Lane C: `state::fake` `FakeBackend` implementing the backend traits, plus `--demo <scene>` scene list file (the A/C contract) | review | `crates/state/src/fake.rs` |
| Lane C: tag `contract-1` after Lane A review | in progress (tag set on lane/c; Lane A review pending) | git tag |
| Lane C: `storage`: sqlx pool (4 connections, WAL, FK on, busy 5000), `migrations/0001_init.sql` (final SQL in architecture.md), `paths`, `app_dir_name()`, backup-before-migrate | review | `crates/storage`, `migrations/` |
| Lane C: `state::Io` (copy of maple), `AppSettings` + settings.json hot reload, theme apply, `tests/settings_used.rs` | review | `crates/state` |
| Lane A: dark + light themes as `ui::Theme::dark()/light()` from the design.md token table, WCAG contrast unit test, radius 2, check where `ui` reads the focus colour and point it at the ink-blue field | open | `crates/ui/src/theme.rs` |
| Lane A: verify gpui dashed border support for the ledger rule; if absent, build the rule as a custom element | open | spike note in `docs/design.md` |
| Lane A: new `ui` parts: `ProgressRule`, `Thumbnail`, `Chip`, `TokenInput`, `SideNav`, `CallNumber`, `MONO` const | open | `crates/ui` |
| Lane A: Getting ready screen against `FakeBackend` | open | `crates/views/src/screens/welcome` |
| Lane B: engine spike (G1): `scripts/setup.ts`, `runtime.lock.json`, PyO3 embed, `--engine-worker`, kill-tree, `cargo run -p engine --example fetch -- <url>` on the fixture server | in progress | review: `crates/engine`, `scripts/setup.ts` |
| Lane B: `engine::Backend` trait, `engine::spec` (presets to yt-dlp opts, pure), `engine::classify` with `errors.tsv` seed | in progress | review: `crates/engine` |
| Lane B: fixture server example + fixtures + generator script (<= 300 KB committed) | in progress | review: `crates/engine/examples/fixture_server.rs`, `tests/fixtures/` |
| Lane D: `tools` skeleton: `Tool` enum, detect, fetch matrix, streaming download with sha256, unpack with zip-slip guard, `YTDLPD_TOOLS_MIRROR` override | open | `crates/tools` |
| Lane D: `cargo run -p tools --example ensure` works for ffmpeg, ffprobe, deno on Windows | open | `crates/tools/examples/ensure.rs` |
| Lane D: hidden flags `--demo`, `--self-test`, `--frame-stats` (debug), `scripts/screenshot.ps1` | in progress | review: `--demo` pass-through, `--frame-stats`, `scripts/screenshot.ps1` (`--self-test` is Lane B) |

Exit checklist (G1):
- [ ] migration 0001 applies on `sqlite::memory:` and a temp file; `PRAGMA journal_mode=wal`, `foreign_keys=1`, all 6 tables, CHECKs reject bad status/kind
- [ ] settings.json round trip + hot reload < 1 s; invalid JSON keeps last good and logs; `settings_used` green
- [ ] contrast test green for dark and light
- [ ] engine: fixture mp4 download returns `Finished{path}` equal to the file on disk; kill-tree test (child spawns child, kill, both dead) green on Windows
- [ ] `--engine-worker` runs from a GUI-subsystem build with piped stdin/stdout (verify, D-007)
- [ ] spike report appended to `decisions.md` D-007 (measured RSS and time to first `ready` for 3 workers; Python init time)
- [ ] `contract-1` tagged

## Phase 1: engine and tools headless
| Item | Status | Ref |
|---|---|---|
| Lane B: `engine::runtime::init` (home exe-relative, isolated, no signal handlers, sys.path order, stdout/stderr to `LogWriter`), second call returns same handle, typed failure | in progress | review: `crates/engine/src/runtime.rs` |
| Lane B: in-process `probe` with timeout, abandoned-probe counter; `ProbeResult` typed; flat playlist probe | in progress | review: `crates/engine/src/probe.rs` |
| Lane B: `--engine-worker` mode, JSON-lines IPC (architecture.md section 4), progress throttled to 4/s, `files` events, kill-tree (Job Object, process group, PDEATHSIG, stdin EOF) | in progress | review: `crates/engine/src/worker.rs` |
| Lane B: fallback `worker.py` with same protocol, behind the same trait; built out only if G1 failed or a target OS fails to link | open | `crates/engine/worker/worker.py` |
| Lane B: cleanup of partials on cancel by known basenames; keep on pause | in progress | review: `crates/engine/src/partials.rs` |
| Lane B: error classification golden table >= 30 real messages | in progress | review: `crates/engine/tests/errors.tsv` |
| Lane B: runtime prune list + import-every-needed-module test; measure and record real runtime size per OS | in progress | review: `scripts/setup.ts` |
| Lane B: `ytdlpd --self-test [--site <dir>]` (init Python, import yt_dlp + yt_dlp_ejs, print versions, exit 0) | in progress | review: `crates/ytdlpd/src/selftest.rs` |
| Lane D: `tools` full: detect order, per-OS matrix, checksum fail closed, atomic rename, exec bit, quarantine strip, Defender retry (5 x 200 ms), version probe | open | `crates/tools` |
| Lane D: yt-dlp + yt-dlp-ejs pair update: PyPI JSON, METADATA `Requires-Dist` pin, unzip to `python-site/<ver>.tmp`, `--self-test --site` in a throwaway process, then pointer switch in `app_state`; rollback tests | open | `crates/tools/src/ytdlp.rs` |
| Lane C: `state::{Tools, Probes}` wrappers over real `tools` and `engine`; `Downloads` skeleton on `FakeBackend` | open | `crates/state` |
| Lane A: Getting ready wired to `state::Tools`; intake strip + result card on fakes | open | `crates/views` |

Exit checklist:
- [ ] HLS fixture gives `.mp4` path after FixupM3u8; progress events monotonic (bytes never decrease, percent monotonic across fragments)
- [ ] merge fixture (video-only + audio-only) gives `.mp4/.mkv` and runs `[Merger]`
- [ ] tools: matrix selection per OS/arch (pure test); checksum parser fixtures (BtbN `<hash> *<name>`, deno `.sha256sum`); tampered archive rejected and temp removed; interrupted stream leaves nothing in `data/tools`
- [ ] corrupt wheel cases (truncated zip, missing `yt_dlp/__init__.py`, injected syntax error) leave the active pointer unchanged and raise a toast
- [ ] `cargo run -p tools --example ensure` on clean `%LOCALAPPDATA%\ytdlpd` downloads; second run with ffmpeg on PATH downloads nothing
- [ ] first run: Getting ready lands on Downloads; offline run starts with the baked yt-dlp (D-012)
- [ ] 8 concurrent fixture downloads plus 3 probes, repeated 20 times: no cross-job path or progress mixups

## Phase 2: vertical slice (all lanes; G2)
| Item | Status | Ref |
|---|---|---|
| Intake strip, paste detection, URL validation (http/https only), split multi-line | open | `views/src/screens/downloads/intake.rs` |
| Probe through `state::Probes` (up to 3 parallel), result card (thumbnail, title, uploader, duration), preset chips, Download | open | `views/.../result_card.rs` |
| Downloads list on `uniform_list`: rows with call number, ledger rule, live progress, completed with real final path | open | `views/.../list.rs` |
| `state::Downloads` real: enqueue, scheduler pump, worker per job, 250 ms tick only while jobs run, DB writes on phase change and at most every 5 s | open | `crates/state/src/downloads.rs` |
| Show in folder / Open (Windows `explorer /select`) | open | - |
| Persistence and restart recovery (active rows become `paused`, reason Interrupted) | open | - |
| Empty state and probe error state | open | - |

Exit checklist (G2):
- [ ] paste fixture URL, Download, row completes with `file_path` non-empty, Show in folder opens Explorer on the file
- [ ] kill and reopen: history present; `taskkill /F` mid-download then restart: row `paused`, resume completes, `PRAGMA integrity_check` ok
- [ ] headless UI test: intake with fake probe renders the card; Download adds a row
- [ ] first screenshots reviewed by lead against design.md

## Phase 3: queue depth (must)
| Item | Status | Ref |
|---|---|---|
| Global and per-domain concurrency, injected `Clock`, 429 penalty box (domain slots to 1 for 5 min) | open | `state/src/scheduler.rs` |
| Pause, resume, cancel, retry on the same row; cancel while queued is immediate; backoff spec with jitter and `Retry-After` | open | - |
| Detail panel (right `Panel`) with log tail; context menu; copy title/link | open | - |
| Filter chips with counts, search with tokens `type:`, `status:` (`size:`, `site:` land in Phase 5) | open | `domain` filter AST |
| Delete entry (undo toast, no confirm) and delete file (confirm with filename); `file_missing` shown as "File moved or deleted" | open | - |
| Free-space check before download; `disk_full` and `permission` errors | open | - |

Exit checklist: all named regression tests in qa.md for queue pass (`cancel_queued_removes_immediately`, `scheduler_no_leaks`, `retry_same_row`, `restart_recovery`, `notify_rate`, `summaries_no_blobs`); transition table test over all (status x event); backoff test on fake clock; per-domain scheduler test.

## Phase 4: options, tools UI, logs (must; G3)
| Item | Status | Ref |
|---|---|---|
| Settings pages (General, Downloads, Formats, Network, Tools, Appearance, About) on `SideNav` | open | `views/.../settings/*` |
| Output folder + template with live preview (equals yt-dlp `prepare_filename` on the fixture) | in progress | engine side review: `engine::preview_filename`, `tests/options.rs`; UI open |
| Subtitles (langs, embed or file), embed thumbnail, metadata, chapters | in progress | engine opts golden tests (`spec.rs`) review; UI open |
| Proxy, rate limit, retries, cookies: file import, paste text field, "Paste from clipboard" (stored in `data/cookies/`, restricted ACL); Firefox recommended | in progress | engine side review: `engine::cookies`, `engine::spec::split_extra_args`, `tests/options.rs` (ratelimit, retries, proxy, cookies, extra_args); UI open |
| Tools page: versions, source, Check for update (yt-dlp + ejs pair), Re-download, Choose file | open | - |
| Logs view: level chips, job picker, Copy all | open | - |
| Tools-missing banner on Downloads; presets needing ffmpeg disabled with a reason | open | - |

Exit checklist (G3): every option lands in a job's `options_json` and changes yt-dlp behaviour: golden opts JSON tests per option, plus fixture-server proofs for ratelimit (4 MB at 1 MB/s takes >= 3 s), retries (server drops first N), proxy (recording proxy saw the request), cookies (server echoed `Cookie:`); outtmpl preview equals real result; `extra_args_shlex`; `cookies_stored_private`; Windows manual script run by the lead.

## Phase 5: depth features (nice tier, still v1)
| Item | Status | Ref |
|---|---|---|
| Playlist picker (select all/none, "In your downloads" marker, Download N), lazy `extract_flat` first | open | - |
| Choose formats modal (two `Table`s, result spec line) | open | - |
| Batch card for multi-URL paste (shared preset, per-URL status, invalid lines listed, duplicates flagged) | open | - |
| Bulk select, bulk delete and retry | open | - |
| Tokens `size:` (`< <= > >= =`) and `site:` | open | - |
| SponsorBlock (mark or remove), cookies from browser (Firefox recommended in copy; Chrome caveat) | open | - |
| Drag and drop of URLs and `.txt`/`.url` files onto the window | open | - |
| Compact mode setting | open | - |

Exit checklist: 500-entry playlist fixture expands lazily, UI above 30 fps, memory +< 50 MB; `split_urls` and filter grammar tests; Choose formats result spec equals the format string given to yt-dlp.

## Phase 6: platform and polish (G4)
| Item | Status | Ref |
|---|---|---|
| Single instance + CLI `ytdlpd add <url>` over the `cli` wire; second process exits < 2 s | open | `crates/cli` |
| Deep link `ytdlpd://add?url=` (parser table: double-encoded, missing url, `javascript:` and `file:` rejected) | in progress | review (lane D): `crates/cli/src/deeplink.rs`, `ytdlpd/src/platform/register.rs` |
| Tray + minimise to tray, OS notifications (trait-faked in tests) | in progress | review (lane D): `crates/ytdlpd/src/platform/{tray,notify}.rs` |
| Clipboard watch on window activation (never writes; prompt, auto-add opt-in) | open | - |
| maple JSON themes beyond dark/light | open | - |
| a11y tree tests for new components; prune music-only `ui` leaf modules (visualizer, waves, level_meter, response_graph, response_sketch, lattice, rail, brand) | open | - |
| Perf pass: all budgets in qa.md measured and recorded (may re-baseline once, recorded in decisions.md) | open | - |
| Window size and position remembered | open | - |

Exit checklist: perf budget table filled with pass/fail; `clipboard_never_writes`, `clipboard_candidates` green; shutdown test (close while downloading: exits < 5 s, rows `paused`, `.part` present, WAL checkpointed).

## Phase 7: packaging and release (G5)
| Item | Status | Ref |
|---|---|---|
| Inno installer (win x64 + arm64), dmg (mac arm64 + x64), AppImage (linux x64 + arm64), each with the python runtime | in progress | `scripts/package-*` |
| macOS: codesign the runtime dir, Frameworks/rpath layout, `install_name_tool` for libpython | in progress | - |
| Windows exe manifest `longPathAware`, GUI subsystem, deep link registration in installer | review | - |
| 6-target release matrix workflow, runtime hash pinned | review | `.github/workflows/release.yml` |
| `THIRD-PARTY.md` generated (yt-dlp Unlicense, PBS, ffmpeg GPL download notice, fonts OFL), CHANGELOG | review | - |

Exit checklist: silent install to a temp dir, `ytdlpd.exe --self-test` exits 0 on a runner with no Python or ffmpeg; THIRD-PARTY.md non-empty; owner-run mac and linux pass recorded (the lead cannot run them).

## Phase 8: after v1 (not scheduled)
App self-updater (port maple `updates/`), ytdlp-ejs in-process JS provider, curl_cffi impersonation, scheduled channels, user-defined presets, browser extension, cobalt backend.

## The 4 lanes

Execution model: up to 4 implementation subagents, one per lane, each in its own git worktree `lane/<x>` at `F:\tmp\ytdlpd-rs\wt\<x>`, with `CARGO_TARGET_DIR=F:\tmp\ytdlpd-rs\target\<x>` (first gpui build takes 10 to 20 minutes). Phase 0a is done by the lead plus Lane A alone.

| Lane | Owns (exclusive write) | Consumes |
|---|---|---|
| A UI | `crates/{ui,views,router,input,icons}`, `assets/{fonts,icons,themes}`, `assets/i18n/*/{common,downloads,settings,welcome}.ftl`, `views/src/root.rs` (UI_FONT), theme tokens | `domain`, `state` public API, `state::fake` |
| B engine | `crates/engine/**` (host side, worker, `worker/worker.py`), `scripts/setup.ts`, `runtime.lock.json`, `runtime/` layout, `.cargo/config.toml`, bin `build.rs` (runtime copy and rpath only), fixture server and fixtures | - |
| C data + state | `crates/{domain,storage,state}/**`, `migrations/**`, `assets/i18n/*/{toasts,errors}.ftl`, `state::fake`, demo scenes | engine and tools APIs |
| D tools + platform | `crates/{tools,cli}/**`, `crates/ytdlpd/src/**` (main, tray, single instance, deep link, notifications, http shim, flags), `scripts/{package-*,screenshot.ps1,inno}`, `.github/**`, `assets/{windows,macos,linux}`, `assets/i18n/*/tools.ftl`, root and `apps/desktop` `Cargo.toml`, docs structure, CHANGELOG | state API (tray actions) |

Shared-file owners
| File | Owner | Rule |
|---|---|---|
| root and `apps/desktop/Cargo.toml` | D | Phase 0a pre-declares all deps; others request additions; append-only blocks `# lane X`; rebase conflicts on those lines are resolved by D |
| `Cargo.lock` | whoever merges | regenerate, never hand-merge |
| `router::Destination` | A | frozen at end of 0a |
| `state` public API and `domain` | C | frozen at `contract-1`; changes use a `CONTRACT:` commit prefix plus a note in `handoff/C.md` and a version bump |
| `ytdlpd/src/main.rs` | D | exposes `build_app(cx) -> Backend`; C plugs state init in there; A never edits it |
| `.ftl` files | per lane (table above) | key prefixes `dl-`, `set-`, `welcome-` (A), `toast-`, `err-` (C), `tools-` (D); en-US only until Phase 6; CI check that every `t!("key")` exists |
| `docs/*` | each lane edits only the section of its area; D owns structure | behaviour change and its doc edit in the same commit |
| `docs/plan.md` | lanes flip own items to in progress; lead flips done | - |

Contract crate and fake backend: `domain` (headless, no gpui) is authored by C on day 1 of 0b, reviewed by A, frozen at `contract-1`. `state::fake::FakeBackend` implements the same traits as the real backend so A builds every screen without engine or tools. The bin has a Cargo feature `real-engine` (default on); `--no-default-features` builds UI and state against the fake with no Python (D-043).

Merge order per phase: B and D (leaf crates) first, then C, then A, then the lead's integration commit on `master`. Each merge must pass `cargo check --workspace`, clippy `-D warnings` and tests (CI on the PR); lanes rebase on `master` daily; lead pushes `origin master` after each merged phase step (owner continues on macOS); the lead only resolves conflicts.

Dependencies between lanes
| Needs | From | When |
|---|---|---|
| Backend traits and domain types | C | day 1 of 0b |
| Theme tokens, `ui` parts | A | 0b |
| `Backend` trait implementation (engine) | B | Phase 1 |
| Tool status and progress | D | Phase 1 |
| Tray and notification actions in state | C to D | Phase 6 |

## Self-verification (every subagent, before claiming done)
Run from `E:\git\ytdlpd-rs` (or the worktree). Set `TMP`, `TEMP` and `CARGO_TARGET_DIR` under `F:\tmp\ytdlpd-rs\`, never C:.
```
bun run check
cd apps/desktop
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked      # or cargo test if nextest is missing
cargo check -p <crate>                      # then cargo check --workspace for dependents
```
- Engine by hand: `cargo run -p engine --example fixture_server`, then `cargo run -p engine --example fetch -- http://127.0.0.1:<port>/sample.mp4`; tools: `cargo run -p tools --example ensure`; seed 10k rows: `cargo run -p storage --example seed -- 10000`.
- Screenshots: `scripts/screenshot.ps1` starts `target\debug\ytdlpd.exe --data-dir F:\tmp\ytdlpd-rs\qa-data --window-size 1280x800 [--demo <scene>]`, waits for the `ready` log line, captures the window with `PrintWindow(hwnd, hdc, PW_RENDERFULLCONTENT=2)` (fallback `CopyFromScreen` on the window rect if the image is black), writes `F:\tmp\ytdlpd-rs\shots\<name>.png`, stops the process. Read the PNG with the Read tool and compare it with the wireframe in design.md. `TestAppContext` has no pixels: use it for roles and behaviour only.
- Debug flags: `--data-dir <dir>`, `--window-size WxH`, `--demo <scene>` (debug builds only), `--self-test` (also in release), `--frame-stats` (debug; prints frame time p50/p95 per second), `RUST_LOG=ytdlpd=debug,engine=trace`.
- Logs: `%LOCALAPPDATA%\ytdlpd\logs\ytdlpd.log` (dev build uses `ytdlpd.dev`); per-job lines in `download_logs`; DB `%LOCALAPPDATA%\ytdlpd\ytdlpd.db` (inspect with `sqlite3`).
- An agent's "done" is a claim. The lead re-runs the checks and the phase manual script, looks at one fresh screenshot per UI change, and quotes the output before setting `done`.

## Context handoff rule
- Main session: prepare to compact at 300 to 320k. Subagents: at about 300k stop new work; never exceed 350k for one agent.
- Write `F:\tmp\ytdlpd-rs\handoff\<lane>.md` with: state (done, built, verified, committed or not, last commit sha, plan.md items flipped); open items and unverified guesses; files you own and shared hooks you changed; exact next steps with debug and test commands; pitfalls hit. Complete enough for a lower-effort agent of the same lane to continue from it alone. Then report and stop.
- A successor reads the handoff, `plan.md` and the lane's crates, and continues; it does not redo verified work.

## Platform verification policy
Windows is the dev box: every phase is verified on Windows. macOS and Linux get code paths and CI builds only; owner-run passes are required before a release candidate (D-059).

## UI/UX layout (owner picks 2026-11-23, mockup F:\tmp\ytdlpd-rs\.superpowers\brainstorm\28102-1795400101\content\ui-layout.html)
- Picks: 1A title-bar tabs; 2C paste shows result card with the remembered preset marked, Enter downloads with it, chips change it first; 3B compact single-line rows 32px, no thumbnail, site beside title, error text in status (ledger rows with thumbnails stay available as setting "Show thumbnails"); 4B "In progress" group pinned on top with header + count, history below; 5A right detail panel 360px; 6C Getting ready continues by itself when tools are ready, later missing tools show a banner on Downloads with a fix button; 7A settings side list of pages; 8A bottom status bar; 9 Ledger look confirmed.
- Status: in progress (UI wiring lanes A2/A3).

## UI style (owner picks 2026-11-23, mockup F:\tmp\ytdlpd-rs\.superpowers\brainstorm\28102-1795400101\content\ui-style.html)
- Picks: 1A Ledger palette; 2A IBM Plex Sans + Mono; 3A radius 2 flat hairlines; 4A ruled progress line; 5A amber only for activity; 6A Lucide line icons; 7A solid window background by default; 8C 12px dense base; 9B subtle motion (detail panel slide 160 ms, new rows fade in, result card unfolds, reduced motion respected); 10 Ledger light confirmed.
- Owner add-on: Settings > Appearance offers maple-style background mode (Solid default / Translucent / Blur = Mica/vibrancy) and text size steps (default 12px). Lane A3.
- Status: in progress (A3 settings; lead applies 12px default + motion after A2/A3 merge).
