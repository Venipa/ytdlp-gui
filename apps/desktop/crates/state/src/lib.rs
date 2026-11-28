//! gpui entities for ytdlpd: settings, downloads (queue, scheduler, lifecycle), probes, tools, logs, toasts.
//! The engine and tools are reached only through `domain::Backend` / `domain::ToolsApi`.

pub mod applog;
mod args;
pub mod cookies;
mod direct;
mod downloads;
pub mod fake;
pub mod fixes;
mod hooks;
mod intake;
mod logs;
mod probes;
mod realtools;
pub mod scheduler;
mod settings;
mod store;
mod toast;
mod tools;
pub mod updates;

pub use args::CustomArgs;
pub use cookies::CookieFiles;
pub use direct::AddDirectError;
pub use downloads::{Downloads, DownloadsEvent, Live, UPDATE_REASON};
pub use fake::{FakeBackend, FakeTools, Scene};
pub use fixes::{FixState, Fixes};
pub use hooks::{BrowserCookiesFn, Hooks, PreviewFn, VersionFn};
pub use intake::{Intake, IntakeEvent, sort_clipboard};
pub use logs::{LogEntry, Logs};
pub use probes::{ProbeState, Probes};
pub use realtools::RealTools;
pub use scheduler::{Limits, Scheduler};
pub use settings::AppSettings;
pub use store::Store;
pub use toast::{Toast, ToastKind, Toasts};
pub use tools::{Tools, ToolsEvent};
pub use updates::{Release, UpdateError, UpdateState, Updates};

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use domain::{Backend, Clock, SystemClock, ToolsApi};
use gpui::{App, AppContext as _, Context, Entity, Global, Task};
use storage::{Db, Paths};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

/// The tokio runtime all network and database work runs on (copied from maple).
#[derive(Clone)]
pub struct Io(Arc<Runtime>);

impl Global for Io {}

/// Worker threads: the work is network calls and sqlite, never a long computation.
const WORKERS: usize = 4;
/// Ceiling on blocking threads (file stats, partial cleanup, DNS).
const BLOCKING: usize = 8;
/// How long an idle blocking thread lingers.
const LINGER: Duration = Duration::from_secs(2);

impl Io {
    /// Builds the runtime.
    pub fn new() -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(WORKERS)
            .max_blocking_threads(BLOCKING)
            .thread_keep_alive(LINGER)
            .thread_name("ytdlpd-io")
            .enable_all()
            .build()?;
        Ok(Self(Arc::new(runtime)))
    }

    /// The runtime registered by `init`.
    pub fn global(cx: &App) -> Self {
        cx.global::<Self>().clone()
    }

    /// A handle for spawning from non-tokio threads.
    pub fn handle(&self) -> tokio::runtime::Handle {
        self.0.handle().clone()
    }

    /// Spawns on the runtime.
    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.0.spawn(future)
    }

    /// Loads on tokio and applies the result on the entity's thread. The caller retains the task.
    pub fn load<E: 'static, T: Send + 'static>(
        &self,
        work: impl Future<Output = Result<T>> + Send + 'static,
        apply: impl FnOnce(&mut E, Result<T>, &mut Context<E>) + 'static,
        cx: &mut Context<E>,
    ) -> Task<()> {
        let io = self.clone();
        cx.spawn(async move |this, cx| {
            let loaded = match io.spawn(work).await {
                Ok(result) => result,
                Err(error) => Err(error.into()),
            };
            this.update(cx, |this, cx| apply(this, loaded, cx)).ok();
        })
    }

    /// Spawns blocking work (file system).
    pub fn spawn_blocking<F, R>(&self, func: F) -> JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.0.spawn_blocking(func)
    }
}

/// The browser-extension bridge. `.0` is the running server, `None` while the setting is off or the
/// start failed; `.1` is why the last start failed (port taken, no free port), cleared when the
/// setting is switched off. Set by the `ytdlpd` binary; the Settings > Browser page reads the port,
/// pairing code and devices from `.0` and the error from `.1`.
pub struct BridgeHandle(pub Option<bridge::Bridge>, pub Option<String>);

impl Global for BridgeHandle {}

/// Everything the views read; entities are cheap handles. Registered as a global by `init`.
#[derive(Clone)]
pub struct AppState {
    pub io: Io,
    pub paths: Paths,
    pub settings: Entity<AppSettings>,
    pub downloads: Entity<Downloads>,
    pub probes: Entity<Probes>,
    pub tools: Entity<Tools>,
    pub logs: Entity<Logs>,
    pub toasts: Entity<Toasts>,
    pub intake: Entity<Intake>,
    pub updates: Entity<Updates>,
    pub fixes: Entity<Fixes>,
    /// Checks of the custom argument lines.
    pub args: Entity<CustomArgs>,
    pub cookies: CookieFiles,
    pub store: Store,
    /// `Some` for `--demo <scene>`.
    pub scene: Option<Scene>,
}

impl Global for AppState {}

impl AppState {
    /// The state registered by `init`.
    pub fn global(cx: &App) -> Self {
        cx.global::<Self>().clone()
    }

    /// A spec for a new download from settings; detected tool paths fill in what settings leave empty
    /// (ffmpeg location, JavaScript runtime).
    pub fn job_spec(
        &self,
        url: &str,
        preset: domain::Preset,
        format_spec: &str,
        playlist_index: Option<u32>,
        cx: &App,
    ) -> domain::JobSpec {
        let mut spec = self
            .settings
            .read(cx)
            .job_spec(url, preset, format_spec, playlist_index);
        let tools = self.tools.read(cx);
        let found = |kind| tools.get(kind).and_then(|s| s.path.clone());
        spec.ffmpeg_location = spec
            .ffmpeg_location
            .or_else(|| found(domain::ToolKind::Ffmpeg));
        spec.js_runtime = spec
            .js_runtime
            .or_else(|| found(domain::ToolKind::Deno).map(|p| ("deno".to_string(), p)));
        let s = self.settings.read(cx).get();
        // aria2c only when switched on; none found leaves yt-dlp's own downloader
        if s.aria2c {
            spec.aria2c = spec.aria2c.or_else(|| found(domain::ToolKind::Aria2c));
        }
        if s.custom_args {
            let args = self.args.read(cx);
            spec.extra_args = args.ytdlp_usable(&s.ytdlp_args);
            if spec.aria2c.is_some() {
                spec.aria2c_args = args.aria2c_usable(&s.aria2c_args);
            }
        }
        apply_rule(
            &mut spec,
            domain::rule_for(&s.site_rules, url),
            s.default_fix,
        );
        spec
    }

    /// Pauses running jobs, saves settings and closes the database; resolves when it is safe to exit.
    pub fn shutdown(&self, cx: &mut App) -> Task<()> {
        self.settings.update(cx, |s, _| s.save_now());
        self.downloads.update(cx, |d, cx| d.shutdown(cx))
    }
}

/// What the matching site rule sets on a new download: its fix (`default_fix` when no rule
/// matches), and its folder and file name template where it sets them.
fn apply_rule(
    spec: &mut domain::JobSpec,
    rule: Option<(usize, &domain::SiteRule)>,
    default_fix: domain::FixChain,
) {
    let Some((_, rule)) = rule else {
        spec.fix = default_fix;
        return;
    };
    spec.fix = rule.fix;
    spec.smaller_quality = rule.smaller_quality;
    spec.smaller_height = rule.smaller_height;
    if let Some(dir) = &rule.output_dir {
        spec.output_dir = dir.clone();
    }
    if let Some(outtmpl) = &rule.outtmpl
        && spec.playlist_index.is_none()
    {
        spec.outtmpl = outtmpl.clone();
    }
}

#[cfg(test)]
mod rule_tests {
    use super::apply_rule;
    use domain::{Fix, SiteRule};

    #[test]
    fn a_rule_sets_folder_and_name_else_defaults_stay() {
        let base = crate::AppSettings::detached(Default::default(), Default::default()).job_spec(
            "https://a.com/v",
            domain::Preset::DEFAULT,
            "",
            None,
        );
        let rule = SiteRule {
            output_dir: Some("/x".into()),
            outtmpl: Some("%(id)s.%(ext)s".into()),
            ..Default::default()
        };

        let mut spec = base.clone();
        apply_rule(&mut spec, Some((0, &rule)), Fix::Repair.into());
        assert_eq!(
            (spec.output_dir.as_str(), spec.outtmpl.as_str(), spec.fix),
            ("/x", "%(id)s.%(ext)s", Default::default())
        );

        let tuned = SiteRule {
            smaller_quality: Some(60),
            smaller_height: Some(0),
            ..Default::default()
        };
        let mut spec = base.clone();
        apply_rule(&mut spec, Some((0, &tuned)), Default::default());
        assert_eq!(
            (spec.smaller_quality, spec.smaller_height),
            (Some(60), Some(0))
        );

        let mut entry = base.clone();
        entry.playlist_index = Some(1);
        apply_rule(&mut entry, Some((0, &rule)), Default::default());
        assert_eq!(
            (entry.output_dir.as_str(), &entry.outtmpl),
            ("/x", &base.outtmpl)
        );

        let mut plain = base.clone();
        apply_rule(
            &mut plain,
            Some((0, &SiteRule::default())),
            Default::default(),
        );
        assert_eq!(
            (&plain.output_dir, &plain.outtmpl),
            (&base.output_dir, &base.outtmpl)
        );

        let mut none = base;
        apply_rule(&mut none, None, Fix::Repair.into());
        assert_eq!(none.fix, Fix::Repair.into());
    }
}

/// How `init` wires the app.
#[derive(Default)]
pub struct InitArgs {
    /// `--data-dir`.
    pub data_dir: Option<PathBuf>,
    /// `--demo <scene>`: in-memory database, fake backend and tools, seeded rows.
    pub demo: Option<Scene>,
    /// The real engine; `None` uses `FakeBackend` (no Python build).
    pub backend: Option<Arc<dyn Backend>>,
    /// Overrides the tools implementation (tests); default is `RealTools`.
    pub tools: Option<Arc<dyn ToolsApi>>,
    /// Injected clock (tests); default is the wall clock.
    pub clock: Option<Arc<dyn Clock>>,
}

/// The updated yt-dlp site: read at start, and followed after each verified update. The pointer lives
/// in `app_state` (`ytdlp_site`, with `ytdlp_site_previous` as the way back); a pointer whose dir is
/// gone is ignored, so the bundled yt-dlp is the fallback.
fn wire_site(backend: &Arc<dyn Backend>, store: &Store, tools: &Entity<Tools>, cx: &mut App) {
    let read = store.read(|db| async move { db.state_get("ytdlp_site").await });
    let (b, t) = (backend.clone(), tools.clone());
    cx.spawn(async move |cx| {
        let site = read
            .await
            .inspect_err(|e| log::warn!("updates: cannot read the saved yt-dlp site: {e:#}"))
            .ok()
            .flatten()
            .filter(|s| {
                let ok = std::path::Path::new(s).is_dir();
                if !ok {
                    log::warn!(
                        "updates: the saved yt-dlp site folder is gone, using the bundled one"
                    );
                }
                ok
            });
        if let Some(site) = site {
            b.set_site(Some(site.clone().into()));
            t.update(cx, |t, _| t.set_site(Some(site)));
        }
    })
    .detach();
    let (backend, store) = (backend.clone(), store.clone());
    cx.subscribe(tools, move |_, event: &ToolsEvent, _| {
        if let ToolsEvent::Updated {
            result: Ok(update), ..
        } = event
        {
            backend.set_site(Some(update.site.clone().into()));
            let (site, previous) = (
                update.site.clone(),
                update.previous.clone().unwrap_or_default(),
            );
            store.write(|db| async move {
                db.state_set("ytdlp_site_previous", &previous).await?;
                db.state_set("ytdlp_site", &site).await
            });
        }
    })
    .detach();
}

/// Creates the runtime, opens storage in the background, builds every entity and registers `AppState`.
pub fn init(args: InitArgs, cx: &mut App) -> Result<AppState> {
    let io = Io::new()?;
    cx.set_global(io.clone());
    let clock: Arc<dyn Clock> = args.clock.unwrap_or_else(|| Arc::new(SystemClock));
    let version = env!("CARGO_PKG_VERSION");

    let (paths, store, backend, tools_api, scene) = match args.demo {
        Some(scene) => {
            let root = std::env::temp_dir().join(format!("ytdlpd-demo-{}", std::process::id()));
            let paths = Paths::resolve(Some(&root));
            let handle = io.handle();
            let seed = fake::seed_rows(scene, clock.now_ms());
            let store = Store::spawn(&io, async move {
                let db = Db::memory().await?;
                db.seed_downloads(&seed).await?;
                if scene != Scene::FirstRun {
                    db.state_set("first_run_done", "1").await?;
                }
                Ok(db)
            });
            let backend: Arc<dyn Backend> = Arc::new(FakeBackend::auto(handle.clone(), 1.0));
            let tools: Arc<dyn ToolsApi> =
                Arc::new(FakeTools::new(Some(handle), scene == Scene::FirstRun));
            (paths, store, backend, tools, Some(scene))
        }
        None => {
            let paths = Paths::resolve(args.data_dir.as_deref());
            if let Err(error) = paths.ensure() {
                log::error!("state: cannot create data directories: {error}");
            }
            applog::attach_file(&paths.logs());
            // no data path: it holds the user's account name, and logs get shared
            log::info!("state: ytdlpd {version} starting");
            let file = paths.db_file();
            let store = Store::spawn(&io, async move { Db::open(&file, version).await });
            let backend: Arc<dyn Backend> = args
                .backend
                .unwrap_or_else(|| Arc::new(FakeBackend::auto(io.handle(), 1.0)));
            let tools: Arc<dyn ToolsApi> = args.tools.unwrap_or_else(|| {
                Arc::new(RealTools::new(
                    io.handle(),
                    ::tools::Dirs {
                        data: paths.data.clone(),
                        cache: paths.cache.clone(),
                    },
                ))
            });
            (paths, store, backend, tools, None)
        }
    };

    let settings_path = paths.settings_file();
    let settings = cx.new(|cx| AppSettings::new(settings_path, cx));
    let cookies = CookieFiles::new(&paths.data);
    cookies.sweep_runs();
    settings.update(cx, |s, cx| {
        let mut c = s.get().cookies.clone();
        if cookies.migrate(&mut c, clock.now_ms()) {
            s.update(|st| st.cookies = c, cx);
        }
        cookies.prune(&s.get().cookies.sets);
    });
    let logs = cx.new(|_| Logs::default());
    // app log lines (every `log::` call) join the Logs screen
    if let Some(mut lines) = applog::take_screen() {
        let logs = logs.clone();
        cx.spawn(async move |cx| {
            use futures::StreamExt as _;
            while let Some((level, message)) = lines.next().await {
                logs.update(cx, |l, cx| l.push(None, &level, &message, cx));
            }
        })
        .detach();
    }
    let toasts = cx.new(|_| Toasts::default());
    let intake = cx.new(|_| Intake::default());
    let probes = cx.new(|_| Probes::new(backend.clone()).with_cookies(cookies.clone()));
    let args = cx.new(|_| CustomArgs::new(backend.clone()));
    // saved lines are checked once at start; until yt-dlp answers they count as fine
    let saved = settings.read(cx).get().clone();
    if saved.custom_args && !saved.ytdlp_args.is_empty() {
        args.update(cx, |a, cx| a.check(&saved.ytdlp_args, cx));
    }
    let tools = cx.new(|_| Tools::new(tools_api));
    wire_site(&backend, &store, &tools, cx);
    let downloads = {
        let (store, logs, clock) = (store.clone(), logs.clone(), clock.clone());
        let files = cookies.clone();
        cx.new(|cx| {
            let current = settings.read(cx).get().clone();
            Downloads::new(backend, store, clock, logs, &settings, cx).with_cookies(files, &current)
        })
    };
    downloads.update(cx, |d, cx| d.load(cx));
    tools.update(cx, |t, cx| t.refresh(cx));
    let updates = {
        let (settings, downloads, store, io) = (
            settings.clone(),
            downloads.clone(),
            store.clone(),
            io.clone(),
        );
        let folder = paths.cache.join("updates");
        let poll = scene.is_none();
        cx.new(|cx| {
            Updates::new(
                settings,
                downloads,
                store,
                io,
                folder,
                updates::api_base(),
                poll,
                cx,
            )
        })
    };

    let fixes = {
        let (io, store, tools, settings, downloads) = (
            io.clone(),
            store.clone(),
            tools.clone(),
            settings.clone(),
            downloads.clone(),
        );
        cx.new(|cx| Fixes::new(io, store, tools, settings, downloads, cx))
    };

    let state = AppState {
        io,
        paths,
        settings,
        downloads,
        probes,
        cookies,
        tools,
        logs,
        toasts,
        intake,
        updates,
        fixes,
        args,
        store,
        scene,
    };
    cx.set_global(state.clone());
    Ok(state)
}
