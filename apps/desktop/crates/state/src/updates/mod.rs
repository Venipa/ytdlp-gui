//! The app updater (ported from maple `state/src/updates`): polls GitHub releases on the chosen
//! channel, downloads the package for this platform while hashing it, keeps it only when it
//! matches `SHA256SUMS.txt`, and installs it on Relaunch or when the app quits.
//! States: idle → checking → available → downloading → ready → installing, or failed (with Try again).
#[cfg(windows)]
pub mod apply;
mod install;
mod release;
#[doc(hidden)]
pub mod testkit;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use domain::{UpdateChannel, UpdateMode};
use futures::StreamExt as _;
use gpui::{App, Context, Entity, Subscription, Task};
use sha2::{Digest as _, Sha256};

use crate::settings::SettingsChanged;
use crate::{AppSettings, Downloads, Io, Store};
use install::Install;
pub use release::{Asset, Published, SUMS, Version, asset_name, expected_sum, pick};

/// The GitHub `owner/repo` releases come from.
pub const REPO: &str = "Venipa/ytdlp-gui";
/// Overrides the API base (default `https://api.github.com`) for the fixture server; a dev build
/// with it set also walks the real install path.
pub const API_ENV: &str = "YTDLPD_UPDATES_API";
const API: &str = "https://api.github.com";
/// How often a running app asks again.
const POLL: Duration = Duration::from_secs(6 * 60 * 60);
/// `app_state` key of the last finished check (unix ms).
const LAST_CHECKED: &str = "updates_last_checked";
/// `app_state` key of the version that ran last, to tell when an update went in.
const LAST_VERSION: &str = "updates_last_version";
const AGENT: &str = concat!("ytdlpd/", env!("CARGO_PKG_VERSION"));

/// The running version.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The API base: `YTDLPD_UPDATES_API` or GitHub.
pub fn api_base() -> String {
    std::env::var(API_ENV)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| API.to_string())
}

/// A release newer than the running version on the chosen channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    /// The GitHub release page (release notes are a link only).
    pub page: String,
    /// The package this platform installs from, when the release has one.
    pub asset: Option<Asset>,
    /// The `SHA256SUMS.txt` download url.
    pub sums: Option<String>,
}

impl Release {
    /// The package size in bytes, when known.
    pub fn size(&self) -> Option<u64> {
        self.asset.as_ref().map(|a| a.size).filter(|s| *s > 0)
    }
}

/// Why the last step failed; each has Try again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateError {
    /// The check could not reach GitHub.
    Offline,
    /// GitHub answered, but not with a release list.
    Check,
    /// The download broke off after `at` bytes.
    Download { at: u64 },
    /// The download did not match its checksum, or the release lists none; nothing was installed.
    Checksum,
    /// The installer could not be started.
    Install,
}

/// Where the updater is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    /// Up to date, or not checked yet.
    Idle,
    Checking,
    Available(Release),
    Downloading {
        release: Release,
        done: u64,
        total: Option<u64>,
    },
    /// Downloaded and verified; goes in on Relaunch or, when asked, on quit.
    Ready(Release),
    Installing(Release),
    Failed {
        error: UpdateError,
        /// The release the failure concerns (`None` for a failed check).
        release: Option<Release>,
    },
}

/// Starts the prepared update after exit: `(path, relaunch)`. A seam so tests never spawn installers.
pub type LaunchFn = Arc<dyn Fn(&Path, bool) -> anyhow::Result<()> + Send + Sync>;

/// The updater entity.
pub struct Updates {
    state: UpdateState,
    settings: Entity<AppSettings>,
    downloads: Entity<Downloads>,
    store: Store,
    io: Io,
    http: Option<reqwest::Client>,
    api: String,
    folder: PathBuf,
    /// The verified (and prepared) package while `Ready`.
    staged: Option<PathBuf>,
    install_on_quit: bool,
    last_checked: Option<i64>,
    updated: bool,
    updates_itself: bool,
    launch: LaunchFn,
    wishes: (UpdateChannel, UpdateMode),
    task: Option<Task<()>>,
    _poll: Option<Task<()>>,
    _settings: Subscription,
    _quit: Subscription,
}

fn wishes(settings: &Entity<AppSettings>, cx: &App) -> (UpdateChannel, UpdateMode) {
    let s = settings.read(cx).get();
    (s.update_channel, s.update_mode)
}

impl Updates {
    /// Builds the updater. `folder` keeps downloads (the cache's `updates/`); `api` is the API
    /// base; `poll` checks now and every 6 hours (off in demo scenes and tests).
    #[allow(clippy::too_many_arguments)] // the entity's wiring, called once from `init` and tests
    pub fn new(
        settings: Entity<AppSettings>,
        downloads: Entity<Downloads>,
        store: Store,
        io: Io,
        folder: PathBuf,
        api: String,
        poll: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let http = client();
        let watch = cx.subscribe(&settings, |this, settings, _: &SettingsChanged, cx| {
            let now = wishes(&settings, cx);
            let before = std::mem::replace(&mut this.wishes, now);
            if now.0 != before.0 && !matches!(this.state, UpdateState::Installing(_)) {
                // another channel offers other builds: forget what this one found
                this.task = None;
                this.staged = None;
                this.set(UpdateState::Idle, cx);
                this.check(cx);
            } else if now.1 == UpdateMode::Automatic
                && let UpdateState::Available(_) = this.state
            {
                this.download(cx);
            }
        });
        let quit = cx.on_app_quit(|this, _| {
            if this.install_on_quit
                && let (UpdateState::Ready(_), Some(path)) = (&this.state, &this.staged)
                && let Err(error) = (this.launch)(path, false)
            {
                log::warn!("updates: cannot install on quit: {error:#}");
            }
            async {}
        });
        let poller = poll.then(|| {
            cx.spawn(async move |this, cx| {
                loop {
                    if this.update(cx, |this, cx| this.check(cx)).is_err() {
                        break;
                    }
                    cx.background_executor().timer(POLL).await;
                }
            })
        });
        #[cfg(windows)]
        if poll {
            // the copy the update card ran from; a card still open keeps its files locked
            std::fs::remove_dir_all(folder.join(apply::RUN_DIR)).ok();
        }
        let read = store.read(|db| async move {
            let checked = db.state_get(LAST_CHECKED).await?;
            let version = db.state_get(LAST_VERSION).await?;
            if version.as_deref() != Some(current_version()) {
                db.state_set(LAST_VERSION, current_version()).await?;
            }
            Ok((checked, version))
        });
        cx.spawn(async move |this, cx| {
            if let Ok((checked, version)) = read.await {
                this.update(cx, |this, cx| {
                    this.last_checked = this
                        .last_checked
                        .or_else(|| checked.and_then(|at| at.parse().ok()));
                    this.updated = version.is_some_and(|v| newer(current_version(), &v));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        let install = Install::current();
        Self {
            state: UpdateState::Idle,
            wishes: wishes(&settings, cx),
            settings,
            downloads,
            store,
            io,
            http,
            api,
            folder,
            staged: None,
            install_on_quit: false,
            last_checked: None,
            updated: false,
            updates_itself: install.updates_itself(),
            launch: Arc::new(|path, relaunch| Install::current().launch(path, relaunch)),
            task: None,
            _poll: poller,
            _settings: watch,
            _quit: quit,
        }
    }

    /// Tests: replace the installer launch; the copy then counts as one that updates itself.
    #[doc(hidden)]
    pub fn set_launcher(&mut self, launch: LaunchFn) {
        self.launch = launch;
        self.updates_itself = true;
    }

    /// Demo scenes and screenshots: show a state without network.
    #[doc(hidden)]
    pub fn show(&mut self, state: UpdateState, cx: &mut Context<Self>) {
        self.task = None;
        self.set(state, cx);
    }

    /// The current state.
    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    /// When the last check finished (unix ms).
    pub fn last_checked(&self) -> Option<i64> {
        self.last_checked
    }

    /// Whether this is the first run of a newer version, until dismissed.
    pub fn updated(&self) -> bool {
        self.updated
    }

    /// Hides "Updated to".
    pub fn dismiss_updated(&mut self, cx: &mut Context<Self>) {
        self.updated = false;
        cx.notify();
    }

    /// Whether the ready update goes in when the app quits.
    pub fn install_on_quit(&self) -> bool {
        self.install_on_quit
    }

    /// Whether this copy installs updates itself; otherwise Download opens the release page.
    pub fn updates_itself(&self) -> bool {
        self.updates_itself
    }

    fn set(&mut self, state: UpdateState, cx: &mut Context<Self>) {
        self.state = state;
        cx.notify();
    }

    /// Asks GitHub for a newer release on the chosen channel. Does nothing while a check or a
    /// download runs, or once an update is ready.
    pub fn check(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.state,
            UpdateState::Checking
                | UpdateState::Downloading { .. }
                | UpdateState::Ready(_)
                | UpdateState::Installing(_)
        ) {
            return;
        }
        let Some(http) = self.http.clone() else {
            return self.set(failed(UpdateError::Check, None), cx);
        };
        let (channel, url) = (
            self.wishes.0,
            format!("{}/repos/{REPO}/releases?per_page=30", self.api),
        );
        let folder = self.folder.clone();
        self.set(UpdateState::Checking, cx);
        let work = self.io.spawn(async move {
            let found = find(&http, &url, channel).await;
            // a package an earlier run verified (only verified files get their final name) is ready at once
            let earlier = match &found {
                Ok(Some(r)) => r
                    .asset
                    .as_ref()
                    .map(|a| folder.join(&a.name))
                    .filter(|p| p.is_file()),
                _ => None,
            };
            let staged = match earlier {
                Some(path) => prepared(path).await.ok(),
                None => None,
            };
            (found, staged)
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let (found, staged) = work.await.unwrap_or_else(|e| {
                log::warn!("updates: check task failed: {e}");
                (Err(UpdateError::Check), None)
            });
            this.update(cx, |this, cx| this.checked(found, staged, cx))
                .ok();
        }));
    }

    fn checked(
        &mut self,
        found: Result<Option<Release>, UpdateError>,
        staged: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.task = None;
        let now = now_ms();
        self.last_checked = Some(now);
        self.store
            .write(move |db| async move { db.state_set(LAST_CHECKED, &now.to_string()).await });
        match found {
            Err(error) => {
                log::warn!("updates: check failed: {error:?}");
                self.set(failed(error, None), cx)
            }
            Ok(None) => {
                log::info!("updates: up to date");
                self.set(UpdateState::Idle, cx)
            }
            Ok(Some(release)) => {
                log::info!("updates: {} is available", release.version);
                match staged {
                    Some(path) if self.updates_itself => self.stage(release, path, cx),
                    _ => {
                        self.set(UpdateState::Available(release), cx);
                        if self.wishes.1 == UpdateMode::Automatic && self.updates_itself {
                            self.download(cx);
                        }
                    }
                }
            }
        }
    }

    /// Downloads the available release (or the failed one again). A copy that does not update
    /// itself, or a release without a package, opens the release page instead.
    pub fn download(&mut self, cx: &mut Context<Self>) {
        let release = match &self.state {
            UpdateState::Available(r) => r.clone(),
            UpdateState::Failed {
                release: Some(r), ..
            } => r.clone(),
            _ => return,
        };
        let (Some(asset), Some(http)) = (release.asset.clone(), self.http.clone()) else {
            return cx.open_url(&release.page);
        };
        if !self.updates_itself {
            return cx.open_url(&release.page);
        }
        let Some(sums) = release.sums.clone() else {
            // fail closed: no checksums, no install
            return self.set(failed(UpdateError::Checksum, Some(release)), cx);
        };
        log::info!("updates: downloading {} ({})", release.version, asset.name);
        self.set(
            UpdateState::Downloading {
                release: release.clone(),
                done: 0,
                total: (asset.size > 0).then_some(asset.size),
            },
            cx,
        );
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<(u64, Option<u64>)>();
        let folder = self.folder.clone();
        let work = self.io.spawn(async move {
            let path = fetch(&http, &asset, &sums, &folder, &tx).await?;
            drop(tx);
            prepared(path).await
        });
        self.task = Some(cx.spawn(async move |this, cx| {
            let progress = this.clone();
            let ticks = async {
                while let Some(mut tick) = rx.next().await {
                    // drain bursts so one notify covers many chunks
                    while let Ok(next) = rx.try_recv() {
                        tick = next;
                    }
                    let alive = progress.update(cx, |this, cx| {
                        if let UpdateState::Downloading { done, total, .. } = &mut this.state {
                            (*done, *total) = (tick.0, tick.1.or(*total));
                            cx.notify();
                        }
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            };
            ticks.await;
            let result = work.await.unwrap_or_else(|e| {
                log::warn!("updates: download task failed: {e}");
                Err(UpdateError::Download { at: 0 })
            });
            this.update(cx, |this, cx| {
                this.task = None;
                match result {
                    Ok(path) => this.stage(release, path, cx),
                    Err(error) => {
                        log::warn!("updates: download of {} failed: {error:?}", release.version);
                        this.set(failed(error, Some(release)), cx)
                    }
                }
            })
            .ok();
        }));
    }

    fn stage(&mut self, release: Release, path: PathBuf, cx: &mut Context<Self>) {
        log::info!(
            "updates: {} is ready ({})",
            release.version,
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        self.staged = Some(path);
        self.set(UpdateState::Ready(release), cx);
    }

    /// Try again: the failed download again, else a fresh check.
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        match &self.state {
            UpdateState::Failed {
                error: UpdateError::Install,
                release: Some(release),
            } if self.staged.is_some() => {
                let release = release.clone();
                self.set(UpdateState::Ready(release), cx);
            }
            UpdateState::Failed {
                release: Some(_), ..
            } => self.download(cx),
            UpdateState::Failed { .. } => {
                self.state = UpdateState::Idle;
                self.check(cx);
            }
            _ => {}
        }
    }

    /// Turns "Install when I quit" on or off.
    pub fn toggle_install_on_quit(&mut self, cx: &mut Context<Self>) {
        self.install_on_quit = !self.install_on_quit;
        cx.notify();
    }

    /// Relaunch to update: schedules the installer for after exit, pauses running downloads
    /// (they resume on the next start), saves settings and quits.
    pub fn relaunch(&mut self, cx: &mut Context<Self>) {
        let (UpdateState::Ready(release), Some(path)) = (&self.state, self.staged.clone()) else {
            return;
        };
        let release = release.clone();
        if let Err(error) = (self.launch)(&path, true) {
            log::warn!("updates: cannot start the installer: {error:#}");
            return self.set(failed(UpdateError::Install, Some(release)), cx);
        }
        log::info!("updates: installing {} after relaunch", release.version);
        self.set(UpdateState::Installing(release), cx);
        self.settings.update(cx, |s, _| s.save_now());
        let paused = self.downloads.update(cx, |d, cx| d.pause_for_update(cx));
        self.task = Some(cx.spawn(async move |_, cx| {
            paused.await;
            cx.update(|cx| cx.quit());
        }));
    }
}

/// Readies a verified package beside what it replaces (identity on Windows and for a managed copy).
async fn prepared(path: PathBuf) -> Result<PathBuf, UpdateError> {
    tokio::task::spawn_blocking(move || Install::current().prepare(&path))
        .await
        .map_err(|_| UpdateError::Install)?
        .map_err(|error| {
            log::warn!("updates: cannot prepare the update: {error:#}");
            UpdateError::Install
        })
}

fn newer(a: &str, b: &str) -> bool {
    matches!((Version::parse(a), Version::parse(b)), (Some(a), Some(b)) if a > b)
}

fn failed(error: UpdateError, release: Option<Release>) -> UpdateState {
    UpdateState::Failed { error, release }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn client() -> Option<reqwest::Client> {
    // reqwest has no built-in crypto provider; a second install only means someone else did it
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    reqwest::Client::builder()
        .user_agent(AGENT)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .build()
        .inspect_err(|e| log::error!("updates: cannot build the http client: {e}"))
        .ok()
}

fn offline_or(error: &reqwest::Error, otherwise: UpdateError) -> UpdateError {
    match error.is_connect() || error.is_timeout() {
        true => UpdateError::Offline,
        false => otherwise,
    }
}

/// The newest release `channel` takes, with this platform's package and the checksums file.
async fn find(
    http: &reqwest::Client,
    url: &str,
    channel: UpdateChannel,
) -> Result<Option<Release>, UpdateError> {
    let response = http
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| {
            log::warn!("updates: cannot ask for releases: {e}");
            offline_or(&e, UpdateError::Check)
        })?;
    let list: Vec<Published> = response.json().await.map_err(|e| {
        log::warn!("updates: cannot read the release list: {e}");
        UpdateError::Check
    })?;
    let current = Version::parse(current_version()).ok_or(UpdateError::Check)?;
    let Some((published, _)) = pick(&list, channel, &current) else {
        return Ok(None);
    };
    let version = published.tag_name.trim_start_matches('v').to_string();
    let name = asset_name(&version, std::env::consts::OS, std::env::consts::ARCH);
    let named = |n: &str| published.assets.iter().find(|a| a.name == n).cloned();
    Ok(Some(Release {
        asset: name.as_deref().and_then(named),
        sums: named(SUMS).map(|a| a.browser_download_url),
        page: published.html_url.clone(),
        version,
    }))
}

/// Streams `asset` into `folder` while hashing it; keeps it only when the hash is the one
/// `SHA256SUMS.txt` lists (a missing or malformed line fails closed).
async fn fetch(
    http: &reqwest::Client,
    asset: &Asset,
    sums: &str,
    folder: &Path,
    progress: &futures::channel::mpsc::UnboundedSender<(u64, Option<u64>)>,
) -> Result<PathBuf, UpdateError> {
    let broke = |at: u64| {
        move |e: reqwest::Error| {
            log::warn!("updates: download broke off at {at} bytes: {e}");
            UpdateError::Download { at }
        }
    };
    let listed = http
        .get(sums)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(broke(0))?
        .text()
        .await
        .map_err(broke(0))?;
    let expected = expected_sum(&listed, &asset.name).ok_or_else(|| {
        log::warn!("updates: {SUMS} lists no checksum for {}", asset.name);
        UpdateError::Checksum
    })?;
    let mut response = http
        .get(&asset.browser_download_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(broke(0))?;
    let total = response
        .content_length()
        .or_else(|| (asset.size > 0).then_some(asset.size));
    let disk = |e: std::io::Error| {
        log::warn!("updates: cannot keep the download: {e}");
        UpdateError::Download { at: 0 }
    };
    std::fs::create_dir_all(folder).map_err(disk)?;
    let part = folder.join(format!("{}.part", asset.name));
    let mut file = std::fs::File::create(&part).map_err(disk)?;
    let (mut hasher, mut done) = (Sha256::new(), 0u64);
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(e) => {
                drop(file);
                std::fs::remove_file(&part).ok();
                return Err(broke(done)(e));
            }
        };
        hasher.update(&chunk);
        file.write_all(&chunk).map_err(disk)?;
        done += chunk.len() as u64;
        progress.unbounded_send((done, total)).ok();
    }
    drop(file);
    let got: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != expected {
        log::warn!("updates: {} does not match its checksum", asset.name);
        std::fs::remove_file(&part).ok();
        return Err(UpdateError::Checksum);
    }
    let path = folder.join(&asset.name);
    std::fs::rename(&part, &path).map_err(disk)?;
    Ok(path)
}
