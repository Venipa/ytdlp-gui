# Lane C contract: `domain` + `storage` (for Lane A review)

Location: `F:\tmp\ytdlpd-rs\pre\c\{domain,storage,migrations}`. Lead moves to `apps/desktop/crates/{domain,storage}` and `apps/desktop/migrations`; `sqlx::migrate!("../migrations")` in `storage/src/lib.rs` and `storage/build.rs` (`rerun-if-changed=../migrations`) need the relative path adjusted (architecture.md says `../../migrations`).
Verified: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (domain 11 tests, storage 9). sqlx **0.9.1** works (features `runtime-tokio`, `sqlite`, `migrate`, `macros`, no default); no fallback needed. 0.9 note: `query*` only accept `&'static str` (no `&format!(..)`); dynamic SQL needs `AssertSqlSafe`. The summary column list is a `concat!` macro for that reason.
External deps: domain = serde, serde_json, futures, url, dirs, anyhow; storage = domain, sqlx, tokio, anyhow, dirs, serde_json (dev: tempfile).

## domain (no gpui)
Modules re-exported flat from the crate root.

**ids/enums** (all `as_str()`, `FromStr`, `Display`, serde as string, `ALL`): `DownloadId(i64)` (serde transparent), `DownloadStatus` (+`is_active`, `is_terminal`), `MediaKind{Video,Audio}`, `ErrorCode` (17 codes, `retryable()`), `VideoQuality{Best,2160..480}`, `AudioFormat{Best,Mp3,M4a,Opus}`, `ToolKind{Ffmpeg,Ffprobe,Deno,YtDlp,YtDlpEjs,Python}`, `ToolSource{Managed,Path,Custom}`.
**Preset** `Video(VideoQuality) | Audio(AudioFormat) | Custom`, string form `video:1080` / `audio:mp3` / `custom`, `Preset::all()` (10), `Preset::DEFAULT`, `.kind()`.
**EngineError{code, message, retryable}** (`new` derives retryable), `std::error::Error`.
**JobSpec** (stored as `options_json`): url, preset, format_spec, output_dir, outtmpl, playlist_index, embed_thumbnail/metadata/chapters, subtitles, sponsorblock, cookies, proxy, rate_limit, retries, ffmpeg_location, js_runtime `(key, path)`, extra_args `Vec<String>`.
**DownloadSummary**: all list columns (no blobs) + precomputed `title_lc`, `uploader_lc`; `refresh_lc()`.
**ProbeResult**: url, webpage_url, extractor, id, title, uploader, duration_s, thumbnail_url, `formats: Vec<FormatInfo>`, `entries: Option<Vec<PlaylistEntry>>` (`Some` = playlist), `info_json`; `is_playlist()`. `FormatInfo{id,ext,width,height,fps,vcodec,acodec,abr,tbr,filesize,note,protocol}`, `PlaylistEntry{index,id,url,title,duration_s,thumbnail_url}`.
**ToolStatus{kind, state: ToolState, source, path, version}**, `ToolState{Checking,Missing,Ready,Downloading{done,total},Failed(EngineError)}`.

**State machine** `transition(DownloadStatus, StateEvent) -> Result<(DownloadStatus, Effects), IllegalTransition>`; `StateEvent{Start,PhaseDownloading,PhaseProcessing,Finished,FailRetryable,FailFinal,Pause,Resume,Cancel,Retry,Interrupted}` (`ALL`). `Effects{attempts_inc, schedule_retry, kill_worker, unqueue, delete_partials, clear_backoff}`. Test `full_table_88_pairs` is the architecture table verbatim. Choices beyond the table: Pause from `queued` also sets `unqueue`; Cancel from an active status = `kill_worker + delete_partials`; Pause from active = `kill_worker` only.
**Backoff**: `Clock{now_ms}` (`SystemClock`, `ManualClock`), `Jitter{unit()->[0,1)}` (`XorShift`), `backoff_secs(attempt, retry_after_s, &mut jitter)`, `next_attempt_at(&clock, attempt, retry_after_s, &mut jitter) -> unix ms`, `should_retry(code, attempts, max)`, `MAX_ATTEMPTS=5`. Per-domain 429 penalty box is scheduler work (state crate), not here.
**Filter**: `Filter::parse(&str)` never fails; `Term::{Type, Status, Size(Cmp,bytes), Site, Text, Invalid}`; `filter.matches(&DownloadSummary)`, `is_empty()`, `has_invalid()`. `type:` / `status:` take comma lists; status also `active` (probing|downloading|processing), `done`; sizes `k|kb|m|mb|g|gb|t|tb` are 1024-based, ops `< <= > >= =`; size uses `file_size` else `bytes_total`, absent size never matches; free text matches title, uploader, url; `Invalid` terms match everything (half-typed `type:`). `Chip{All,Active,Done,Failed}.matches(status)` (Active = non-terminal incl. paused; Failed includes cancelled; tell me if you want it otherwise).
**URLs**: `split_urls(text) -> SplitUrls{urls, invalid: Vec<(line, token)>}` (http/https with host only, whitespace tokens, strips `<>"',`, dedupes exact); `domain_of(url)` (lowercase, no `www.`, no port).
**Settings** (in domain, serde only): `Settings` with every key and default from architecture section 12 (unknown keys ignored, missing keys default), `Settings::load(path, &last_good) -> Loaded{settings, error}` (missing file = defaults; invalid JSON = last good + error), `settings.save(path)` (tmp file + rename). Sub-structs `SubtitleSettings`, `SponsorBlockSettings{mode: Off|Mark|Remove}`, `CookiesSettings{mode: None|File|Browser}`, `ToolPaths`, `WindowState`. The gpui watcher/debounce lives in `state`.

**Backend traits** (object-safe, `BoxFuture`/`BoxStream`, `Send + Sync`):
```rust
pub trait Backend: Send + Sync {
    fn probe(&self, url: String, opts: ProbeOpts) -> BoxFuture<'static, Result<ProbeResult, EngineError>>;
    fn start(&self, id: DownloadId, spec: JobSpec)
        -> BoxFuture<'static, Result<(JobHandle, BoxStream<'static, JobEvent>), EngineError>>;
    fn kill(&self, handle: JobHandle) -> BoxFuture<'static, ()>;   // pause and cancel are both kill
}
pub trait ToolsApi: Send + Sync {
    fn status(&self) -> BoxFuture<'static, Vec<ToolStatus>>;
    fn ensure(&self, tool: ToolKind) -> BoxStream<'static, ToolEvent>;  // ends with Done | Failed
    fn set_custom_path(&self, tool: ToolKind, path: String) -> BoxFuture<'static, Result<ToolStatus, EngineError>>;
}
```
`JobHandle(u64)`, `ProbeOpts{timeout_s, flat_playlist, cookies, proxy}`, `JobEvent{Ready, Progress{done,total,speed,eta,frag}, Phase(String), Path(String), Files(Vec<String>), Log{level,message}, Finished{info_json, info_path}, Failed(EngineError)}` (1:1 with the IPC lines), `ToolEvent{Progress{done,total}, Done(ToolStatus), Failed(EngineError)}`. `app_dir`/data paths are not part of the traits.

## storage
`Db::open(path, app_version)` (parent dir created; pool 4, WAL, FK, busy 5000, sync NORMAL; if the file existed and migrations are pending: `wal_checkpoint(TRUNCATE)` then copy to `ytdlpd.db.bak-<app_version>` once; then migrate), `Db::memory()` (1 connection), `db.pool()`, `db.close()` (checkpoint + close). `MIGRATOR` static.
Repo fns (async, on `Db`, timestamps unix ms taken internally): `insert_download(&NewDownload)->DownloadId` (queued, position = max+1, meta in same tx; `NewDownload::from_spec(&JobSpec)`), `set_status(id, status, reason)` (stamps started_at on probing, finished_at on terminal), `set_phase`, `set_progress(id, progress, bytes_done, bytes_total)`, `set_final_path(id, path, size)`, `set_file_missing`, `set_error(id, Option<(ErrorCode,&str)>)`, `set_retry(id, attempts, next_attempt_at)`, `set_meta(title, uploader, thumbnail_path)`, `list_summaries()` (newest first, no blobs), `get_detail(id)` (+format_spec, options_json, output_dir, webpage_url, info_json), `delete_download(id)`, `recover_interrupted()` (active -> paused/`interrupted`, returns count), `append_log(id, level, msg)` (cap `LOG_CAP`=500 per job, trimmed on every append) / `list_logs(id)`, `state_get/state_set`, `upsert_tool(&ToolRow)/list_tools()`, `insert_playlist/get_playlist`.
`Paths::resolve(Option<&Path>)` -> `{config, data, cache}` (`--data-dir X` gives `X/config`, `X/data`, `X/cache`), `.settings_file()`, `.db_file()`, `.logs()`, `.ensure()`; `app_dir_name()` = `ytdlpd` release / `ytdlpd.dev` debug (cfg debug_assertions).
Example: `cargo run --release -p storage --example seed -- 10000 [db]` (inserts 10k in ~5.4 s, `list_summaries` of 10k rows in ~58 ms, release).

## Deviations / notes
- architecture.md says "Seven tables" but `0001_init.sql` defines six (playlists, downloads, download_meta, download_logs, tools, app_state); test asserts six. Doc should say six.
- Domain does not depend on sqlx: enums map by string (`as_str`/`FromStr`) and storage converts via a private `SummaryRow`, instead of `sqlx::Type` on domain enums (keeps domain headless and dep-light).
- The state-machine event enum is `StateEvent` (the name `JobEvent` is the worker event, per the IPC schema).
- Logs trimmed per append, not only when the job finishes (spec text allowed either).
- Not done (needs the skeleton): `state` crate (Io, AppSettings watcher, FakeBackend), `settings_used.rs`, `contract-1` tag.
