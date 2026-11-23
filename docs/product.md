# ytdlpd product

## What
A native desktop downloader for anything yt-dlp supports: paste a link, pick a preset, get a correctly named file. Rust and GPUI, one window, no browser engine. It uses yt-dlp through its Python API (embedded interpreter), not through the yt-dlp executable or stdout parsing.

## Who
People who archive video, music and podcasts locally and want a fast, quiet tool instead of a command line. First user: the owner, replacing the Electron app `ytdlp-desktop`.

## Why rewrite
The old app parses stdout, guesses final paths (wrong extension after merge or convert is reported as an error), leaks queue waiters, wipes the clipboard, re-renders on every progress tick, ships dead settings and splits extra arguments on spaces. Each of these is a named regression test in `qa.md`.

## v1 feature list
Ordered must first. "Must" ships first in the phase order; "depth" follows (Phase 5 and 6). All of it is v1 (D-041).

Must
1. Getting ready: yt-dlp (baked, updatable), ffmpeg and ffprobe (settings path, then PATH, then download for this OS and architecture), JavaScript runtime (detect deno, node, bun, quickjs; else download deno).
2. Intake: paste or type a link, Ctrl+V anywhere on Downloads, multi-line paste, http and https only.
3. Probe: title, thumbnail, uploader, duration, formats; up to 3 probes in parallel; inline error with one action.
4. Presets: Video best, 2160, 1440, 1080, 720, 480; Audio best, mp3, m4a, opus with real conversion; last choice remembered.
5. Queue: global concurrency (default 3) and per-domain (default 2); pause, resume, cancel, retry on the same row; backoff on retryable errors; restart recovery to paused.
6. Unified Downloads list (active plus history, virtualised): filter chips All, Active, Done, Failed with counts; search with `type:` and `status:` tokens.
7. Detail panel: thumbnail, title, link, file path, format line, options used, attempts, error text, last 20 log lines.
8. Open file, show in folder, delete entry (undo toast), delete file (confirm).
9. Settings: output folder and filename template with live preview, subtitles, embed thumbnail, metadata and chapters, proxy, rate limit, retries, cookies file.
10. Tools page: versions, source (Managed, Found at path, Custom), check for update, re-download, choose file.
11. Logs view.
12. Persistence in SQLite via sqlx; settings.json with hot reload; dark and light themes.

Depth (still v1)
13. Playlists and channels expanded into selectable entries; Download N.
14. Choose formats: pick a video and an audio stream.
15. Batch card for multi-URL paste; duplicates flagged.
16. Bulk select; `size:` (`< <= > >= =`) and `site:` tokens.
17. SponsorBlock (mark or remove); cookies from browser.
18. Drag and drop of links and `.txt`/`.url` files.
19. Single instance, CLI `ytdlpd add <url>`, deep link `ytdlpd://add?url=`.
20. Tray and minimise to tray; OS notifications on finish and failure.
21. Clipboard watch (read only; prompts; auto-add is opt-in).
22. maple JSON themes; compact mode.
23. Installers: Windows (Inno), macOS (dmg), Linux (AppImage).

## v2
App self-updater, ytdlp-ejs in-process JavaScript provider, curl_cffi impersonation, scheduled and subscribed channels, per-site and user-defined presets, browser extension, cobalt API backend (extend site support beyond yt-dlp).

## Non-goals for v1
cobalt port, browser extension, built-in player, self-updater, mobile, transcoding beyond yt-dlp postprocessors, Chrome cookie extraction guarantees on Windows (see D-031).

## Rust advantages (honest)
Where it matters
- Typed hooks: progress, postprocessor and final-path events come from yt-dlp's hooks as typed messages, not parsed from stdout. The final path comes from `post_hooks` and `requested_downloads[0].filepath`, never guessed.
- Cancel that works: each download is its own process; cancel or pause kills the process tree (worker, ffmpeg, deno) instantly, including during extraction and merges. A thread inside one interpreter cannot be killed cleanly.
- Crash isolation: a native-extension crash in a download takes down one worker, not the app. A GUI crash takes the workers down with it (Job Object, process group).
- Explicit job state machine (enum, exhaustive match); no leaked waiters.
- Bounded render cost: progress is latest-wins atomics read on a 250 ms tick, the worker emits at most 4 events per second, the list is a GPU-drawn `uniform_list`.
- One binary does tray, deep link, single instance and notifications without an IPC bridge.
- yt-dlp updates without an app release, and take effect at the next worker spawn.

Where it does not
- Download speed is network bound; extraction speed is still Python; merging is still an ffmpeg subprocess; site support is identical to yt-dlp.
- Disk size is dominated by Python, deno and ffmpeg, not Rust. Measured PBS 3.14.10 compressed downloads: win x64 51.5 MB, win arm64 48.4 MB, mac arm64 27.1 MB, linux x64 76.4 MB before pruning; the pruned size is measured in Phase 1 and recorded, not promised.
- Costs: embedding and per-OS packaging, about 60 to 90 MB RSS per worker (3 workers about 250 MB), 0.4 to 0.8 s worker start (more under Defender), long compile times, GPUI maturity (mitigated by the maple kit).

## Performance budgets (release build, win x64, mid laptop; pass/fail)
| Metric | Budget |
|---|---|
| First frame | < 800 ms cold, < 400 ms warm; Python not yet initialised |
| RSS idle (30 s on Downloads) | < 150 MB (target 120) |
| RSS with 3 downloads | < 220 MB, delta from idle < 70 MB, no growth trend over 10 min |
| CPU idle | < 1% average; < 5% of one core while 3 downloads tick at 4 Hz; no `cx.notify` for 2 s when idle |
| List scroll at 10k rows | >= 60 fps (p95 frame < 16.7 ms) |
| DB writes | <= 1 write transaction per second per job; 0 when idle; 10k-row cold load < 300 ms; WAL < 4 MB |
| Download overhead | within 10% of the yt-dlp CLI on the 40 MB fixture over loopback |
| Python init in the app process | < 700 ms on its own thread, no UI frame > 50 ms during it |
| Filter keystroke at 10k rows | result < 50 ms |
Methods are in `qa.md`. The first Phase 2 measurement may re-baseline once.

## Identity
Name `ytdlpd`, app id `net.venipa.ytdlpd`, GPL-3.0-or-later (same as maple; yt-dlp is Unlicense; ffmpeg is downloaded, not bundled). Data directories `ytdlpd` and `ytdlpd.dev` (D-054).
