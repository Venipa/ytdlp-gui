//! The browser-extension bridge in the app: implements [`bridge::Host`], starts and stops the
//! server with `Settings.browser_extension`, and keeps it in [`state::BridgeHandle`] for the
//! Settings > Browser page. Network and the extractor check run off the gpui thread; everything
//! that touches state is marshalled to it through a channel, like the platform event pump.
use std::rc::Rc;
use std::sync::Arc;

use bridge::{AddReq, AddState, Added, Checked, Host, Kind, PresetInfo, WireError};
use domain::{AudioFormat, Preset, VideoQuality};
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use futures::channel::oneshot;
use futures::future::BoxFuture;
use gpui::{App, Global, Task};
use i18n::t;
use state::{AddDirectError, AppState, BridgeHandle};

use crate::platform::{self, Event};

/// Chrome extension ids allowed to connect (the id is fixed by the `key` in the extension manifest).
pub const CHROME_IDS: &[&str] = &["bgkgbgdkidinpmgcignpdgoeeekdfhlk"];

/// Release builds listen here; debug builds use [`DEV_PORTS`] so both can run side by side.
const PORTS: std::ops::RangeInclusive<u16> = 47821..=47830;
/// Ports of the `ytdlpd.dev` build.
const DEV_PORTS: std::ops::RangeInclusive<u16> = 48821..=48830;
/// What a client is told to wait when the app is not ready yet.
const RETRY_MS: u64 = 2000;

/// Work the bridge needs from the gpui thread.
enum Req {
    /// The presets to offer and the default one for `url`.
    Presets {
        url: String,
        reply: oneshot::Sender<(Vec<PresetInfo>, String)>,
    },
    /// Queue `url` now.
    Add {
        url: String,
        preset: Option<Preset>,
        reply: oneshot::Sender<Result<Added, WireError>>,
    },
    /// Open `url` in the intake box and raise the window.
    Intake(String),
    /// A browser paired.
    Paired(String),
}

/// What the bridge sees of the app.
struct Link {
    tx: UnboundedSender<Req>,
    io: tokio::runtime::Handle,
}

/// The extractor that claims a URL.
struct Found {
    key: String,
    name: String,
    kind: Kind,
}

/// The saved yt-dlp site (`ytdlp_site`), set once `install` has read it; the same value `wire_site`
/// hands the engine, so the interpreter starts on the site the app's own probes use.
static SITE: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();

/// Imports the extractors on a worker thread so the first real check is fast; waits for [`SITE`].
fn warm(io: &tokio::runtime::Handle) {
    if SITE.get().is_some() {
        io.spawn_blocking(|| drop(find("https://www.youtube.com/")));
    }
}

#[cfg(feature = "real-engine")]
fn find(url: &str) -> Result<Option<Found>, WireError> {
    use engine::UrlKind;
    let down = |e: String| WireError::new("engine_unavailable", e);
    // the interpreter keeps the yt-dlp site it starts with for the whole process, so it must not
    // start before the saved site is known (`install` reads it)
    let Some(site) = SITE.get() else {
        return Err(down("starting".into()));
    };
    engine::runtime::init(engine::runtime::InitOptions {
        home: None,
        site_updates: site.clone(),
        sink: engine::runtime::log_sink,
    })
    .map_err(|e| down(e.to_string()))?;
    let found = engine::suitable(url).map_err(|e| down(format!("{e:#}")))?;
    Ok(found.map(|s| Found {
        key: s.key,
        name: s.name,
        kind: match s.kind {
            UrlKind::Video => Kind::Video,
            UrlKind::Playlist => Kind::Playlist,
            UrlKind::Channel => Kind::Channel,
            UrlKind::Unknown => Kind::Unknown,
        },
    }))
}

#[cfg(not(feature = "real-engine"))]
fn find(_: &str) -> Result<Option<Found>, WireError> {
    Err(WireError::new("engine_unavailable", "no yt-dlp engine"))
}

impl Link {
    /// Runs the extractor check on the blocking pool.
    fn find(&self, url: String) -> BoxFuture<'static, Result<Option<Found>, WireError>> {
        let work = self.io.spawn_blocking(move || find(&url));
        Box::pin(async move {
            work.await
                .unwrap_or_else(|e| Err(WireError::new("internal", e.to_string())))
        })
    }

    /// Asks the gpui thread; a gone app is an internal error.
    fn ask<T: Send + 'static>(
        &self,
        make: impl FnOnce(oneshot::Sender<T>) -> Req,
    ) -> BoxFuture<'static, Result<T, WireError>> {
        let (tx, rx) = oneshot::channel();
        let sent = self.tx.unbounded_send(make(tx)).is_ok();
        Box::pin(async move {
            match sent {
                true => rx.await.ok(),
                false => None,
            }
            .ok_or_else(|| WireError::new("internal", "the app is shutting down"))
        })
    }
}

impl Host for Link {
    fn check(&self, url: String) -> BoxFuture<'static, Result<Checked, WireError>> {
        let found = self.find(url.clone());
        let presets = self.ask(|reply| Req::Presets {
            url: url.clone(),
            reply,
        });
        Box::pin(async move {
            let Some(f) = found.await? else {
                return Ok(Checked {
                    url,
                    supported: false,
                    extractor: None,
                    name: None,
                    kind: Kind::Unknown,
                    presets: vec![],
                    preset: None,
                });
            };
            let (presets, default) = presets.await?;
            Ok(Checked {
                url,
                supported: true,
                extractor: Some(f.key),
                name: Some(f.name),
                kind: f.kind,
                presets,
                preset: Some(default),
            })
        })
    }

    fn add(&self, req: AddReq) -> BoxFuture<'static, Result<Added, WireError>> {
        let preset = match req.preset.as_deref().map(str::parse::<Preset>) {
            Some(Ok(p)) if p != Preset::Custom => Some(p),
            Some(_) => {
                let e = WireError::new("bad_request", "unknown preset");
                return Box::pin(std::future::ready(Err(e)));
            }
            None => None,
        };
        let found = self.find(req.url.clone());
        let tx = self.tx.clone();
        let (reply, done) = oneshot::channel();
        Box::pin(async move {
            let kind = found
                .await?
                .ok_or_else(|| WireError::new("unsupported", "no yt-dlp extractor for this page"))?
                .kind;
            let gone = || WireError::new("internal", "the app is shutting down");
            // a playlist is never queued by one click; the user picks what to take in the app
            if !req.start || kind != Kind::Video {
                tx.unbounded_send(Req::Intake(req.url))
                    .map_err(|_| gone())?;
                return Ok(Added {
                    download: None,
                    state: AddState::Intake,
                    duplicate: false,
                });
            }
            tx.unbounded_send(Req::Add {
                url: req.url,
                preset,
                reply,
            })
            .map_err(|_| gone())?;
            done.await.map_err(|_| gone())?
        })
    }

    fn paired(&self, device: String) {
        self.tx.unbounded_send(Req::Paired(device)).ok();
    }
}

/// The presets offered to the extension, with labels in the app's language.
fn presets() -> Vec<PresetInfo> {
    let video = t!("dl-card-video");
    let audio = t!("dl-card-audio");
    Preset::all()
        .into_iter()
        .map(|p| {
            let label = match p {
                Preset::Video(VideoQuality::Best) => format!("{video} {}", t!("dl-preset-best")),
                Preset::Video(q) => format!("{video} {}p", q.as_str()),
                Preset::Audio(AudioFormat::Best) => format!("{audio} {}", t!("dl-preset-best")),
                Preset::Audio(a) => format!("{audio} {}", a.as_str()),
                Preset::Custom => String::new(),
            };
            PresetInfo {
                id: p.to_string(),
                label,
            }
        })
        .collect()
}

fn wire_error(e: AddDirectError) -> WireError {
    match e {
        AddDirectError::Busy => WireError {
            retry_after_ms: Some(RETRY_MS),
            ..WireError::new("busy", e.to_string())
        },
        AddDirectError::Failed(_) => WireError::new("internal", e.to_string()),
    }
}

fn handle(req: Req, app: &AppState, cx: &mut App) {
    match req {
        Req::Presets { url, reply } => {
            let default = app.default_preset(&url, cx).to_string();
            reply.send((presets(), default)).ok();
        }
        Req::Intake(url) => platform::emit(Event::AddUrls(vec![url]), cx),
        Req::Add { url, preset, reply } => {
            let task = app.add_direct(url, preset, cx);
            cx.spawn(async move |cx| {
                let result = task.await;
                if matches!(result, Ok((_, false))) {
                    cx.update(|cx| {
                        views::toast(
                            cx,
                            views::ToastKind::Success,
                            t!("toast-ext-added"),
                            Some((
                                t!("toast-ext-show"),
                                Rc::new(|cx: &mut App| platform::emit(Event::Show, cx)),
                            )),
                        )
                    });
                }
                reply
                    .send(result.map_err(wire_error).map(|(id, duplicate)| Added {
                        download: id.map(|id| id.0),
                        state: AddState::Queued,
                        duplicate,
                    }))
                    .ok();
            })
            .detach();
        }
        Req::Paired(device) => {
            let text = t!("toast-ext-paired", device = device.as_str());
            views::toast(cx, views::ToastKind::Success, text.clone(), None);
            platform::notifier(cx).notify(&t!("toast-ext-paired-title"), &text);
        }
    }
}

/// The request pump; lives as long as the app.
struct Pump {
    _task: Task<()>,
    _site: Task<()>,
}

impl Global for Pump {}

/// Starts the bridge now when the setting asks for it, and follows the setting from then on.
/// Call only on the first instance (a second one would fight for the ports).
pub fn install(app: &AppState, cx: &mut App) {
    let (tx, mut rx) = unbounded();
    let state = app.clone();
    let task = cx.spawn(async move |cx| {
        while let Some(req) = rx.next().await {
            cx.update(|cx| handle(req, &state, cx));
        }
    });
    let read = app
        .store
        .read(|db| async move { db.state_get("ytdlp_site").await });
    let io = app.io.handle();
    let site = cx.spawn(async move |_| {
        let site = read
            .await
            .ok()
            .flatten()
            .filter(|s| std::path::Path::new(s).is_dir());
        SITE.set(site.map(Into::into)).ok();
        warm(&io);
    });
    cx.set_global(Pump {
        _task: task,
        _site: site,
    });
    cx.set_global(BridgeHandle(None, None));
    let app = app.clone();
    // a failed bind is tried again only when the setting flips, not on every settings change
    let wanted = std::cell::Cell::new(false);
    let sync = move |cx: &mut App| {
        let on = app.settings.read(cx).get().browser_extension;
        if on == wanted.replace(on) {
            return;
        }
        if on {
            let link = Link {
                tx: tx.clone(),
                io: app.io.handle(),
            };
            match bridge::start(config(&app), Arc::new(link)) {
                Ok(b) => {
                    // the first extractor check imports every extractor, so pay for it up front
                    warm(&app.io.handle());
                    cx.set_global(BridgeHandle(Some(b), None));
                }
                Err(e) => {
                    log::error!("bridge: {e:#}");
                    views::toast(
                        cx,
                        views::ToastKind::Error,
                        t!("toast-ext-bind-failed"),
                        None,
                    );
                    cx.set_global(BridgeHandle(None, Some(format!("{e:#}"))));
                }
            }
        } else {
            let handle = cx.global_mut::<BridgeHandle>();
            handle.1 = None;
            if let Some(b) = handle.0.take() {
                // `stop` waits for the close frames to flush
                std::thread::spawn(|| b.stop());
            }
        }
    };
    sync(cx);
    let settings = cx.global::<AppState>().settings.clone();
    cx.observe(&settings, move |_, cx| sync(cx)).detach();
}

fn config(app: &AppState) -> bridge::Config {
    let dev = storage::app_dir_name().ends_with(".dev");
    bridge::Config {
        data_dir: app.paths.data.clone(),
        ports: if dev { DEV_PORTS } else { PORTS },
        allowed_chrome_ids: CHROME_IDS,
        app_version: env!("CARGO_PKG_VERSION").into(),
        label: storage::app_dir_name().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_cover_all_but_custom() {
        let all = presets();
        let ids: Vec<_> = all.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids.len(), 10);
        assert!(ids.contains(&"video:1080") && !ids.contains(&"custom"));
        assert!(all.iter().all(|p| !p.label.is_empty()));
    }

    #[test]
    fn busy_asks_for_a_retry() {
        let e = wire_error(AddDirectError::Busy);
        assert_eq!(
            (e.code.as_str(), e.retry_after_ms),
            ("busy", Some(RETRY_MS))
        );
    }
}
