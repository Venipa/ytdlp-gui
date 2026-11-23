#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod http;
mod platform;
mod selftest;

use std::path::PathBuf;
use std::process::ExitCode;
use std::str::FromStr;

use clap::Parser;
use gpui::{
    App, AppContext as _, Bounds, Pixels, Size, TitlebarOptions, WindowBounds, WindowOptions, px,
    size,
};
use router::Destination;
use ui::{Look, Rounding, Theme, ThemeKind, ThemeOverrides};
use views::Root;

const LEAST_SIZE: Size<Pixels> = size(px(480.), px(400.));
const FIRST_SIZE: Size<Pixels> = size(px(1100.), px(720.));

/// A native yt-dlp downloader.
#[derive(Parser)]
#[command(version)]
struct Args {
    /// Where the database, settings and logs live instead of the platform default.
    #[arg(long, value_name = "DIR")]
    data_dir: Option<PathBuf>,
    /// The first window size, as WIDTHxHEIGHT.
    #[arg(long, value_name = "WxH")]
    window_size: Option<WindowSize>,
    #[command(flatten)]
    launch: cli::args::Launch,
    #[command(subcommand)]
    command: Option<cli::args::Command>,
    /// Debug: open on a named screen state, such as `card`, `logs` or `settings-network`.
    #[arg(long, value_name = "NAME")]
    screen: Option<String>,
    /// Theme override: `dark`, `light` or `system` (default: the saved setting).
    #[arg(long)]
    theme: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WindowSize(Size<Pixels>);

impl FromStr for WindowSize {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (width, height) = text
            .split_once(['x', 'X'])
            .ok_or("expected WIDTHxHEIGHT, such as 1280x800")?;
        let number = |part: &str| {
            part.trim()
                .parse::<f32>()
                .ok()
                .filter(|value| *value >= 1.)
                .ok_or_else(|| format!("{part:?} is not a size"))
        };
        Ok(Self(size(px(number(width)?), px(number(height)?))))
    }
}

/// The one place state is wired up: runtime, storage (opened in the background, honours
/// `--data-dir`), the real engine (feature `real-engine`) or the fake one, and every entity.
fn build_app(data_dir: Option<PathBuf>, demo: Option<&str>, cx: &mut App) -> state::AppState {
    let scene = demo.and_then(|name| {
        let scene = state::Scene::parse(name);
        if scene.is_none() {
            log::warn!(
                "ytdlpd: unknown demo scene {name:?}, expected one of {:?}",
                state::Scene::NAMES
            );
        }
        scene
    });
    #[cfg(feature = "real-engine")]
    let backend: Option<std::sync::Arc<dyn domain::Backend>> = scene.is_none().then(|| {
        let paths = storage::Paths::resolve(data_dir.as_deref());
        let config = engine::EngineConfig {
            worker_exe: std::env::current_exe().unwrap_or_default(),
            python_home: None,
            site_updates: None,
            env: engine::spec::Env {
                cache_dir: Some(paths.cache.join("yt-dlp").to_string_lossy().into_owned()),
                // the JavaScript runtime travels in each job spec (settings, then the detected deno)
                js_runtime: None,
            },
        };
        std::sync::Arc::new(engine::EngineBackend::new(config)) as _
    });
    #[cfg(feature = "real-engine")]
    cx.set_global(engine_hooks());
    #[cfg(not(feature = "real-engine"))]
    let backend = None;
    let args = state::InitArgs {
        data_dir,
        demo: scene,
        backend,
        ..Default::default()
    };
    state::init(args, cx).expect("cannot start the application state")
}

fn main() -> ExitCode {
    if let Some(code) = selftest::dispatch() {
        return code;
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("ytdlpd=info"))
        .init();
    let args = Args::parse();
    // single instance: a second launch forwards its links to the running app and exits
    let Some(platform) = platform::start(
        &args.launch,
        args.command.as_ref(),
        args.data_dir.as_deref(),
    ) else {
        return ExitCode::SUCCESS;
    };

    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            if let Err(error) = assets::Assets.load_fonts(cx) {
                log::error!("ytdlpd: cannot load bundled fonts: {error:#}");
            }
            i18n::set(i18n::resolve(i18n::AUTO));
            ui::motion::apply(Default::default(), Default::default(), cx);
            let look = Look {
                kind: if args.theme.as_deref() == Some("light") {
                    ThemeKind::Light
                } else {
                    ThemeKind::Dark
                },
                rounding: Rounding::Subtle,
                font: 13.,
                transparent: false,
                transparency: 1.,
                blur: false,
                blur_window: false,
                tint: None,
                tint_secondary: None,
            };
            Theme::init(look, &ThemeOverrides::default(), cx);
            cx.bind_keys(input::bindings());
            cx.on_action(|_: &input::Quit, cx| cx.quit());
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() && !platform::keeps_running(cx) {
                    cx.quit();
                }
            })
            .detach();

            let state = build_app(args.data_dir.clone(), args.launch.demo.as_deref(), cx);
            cx.set_http_client(std::sync::Arc::new(http::Client::new(
                state.io.handle(),
                Some(state.paths.cache.join("thumbnails")),
            )));
            views::apply_stored(args.theme.as_deref(), cx);
            wire_platform(&state, cx);
            router::init(Destination::Downloads, cx);
            let size = args.window_size.map(|size| size.0).unwrap_or(FIRST_SIZE);
            let frame_stats = args.launch.frame_stats;
            let scene = args.screen.clone();
            let hooks = platform::Hooks {
                open_window: Box::new(move |cx| open_window(scene.clone(), size, frame_stats, cx)),
                on_event: Box::new(move |event, cx| route_event(&state, event.clone(), cx)),
            };
            // release builds with the real data dir register the ytdlpd:// scheme
            let register = !cfg!(debug_assertions) && args.data_dir.is_none();
            if platform.install(hooks, register, cx) {
                open_window(args.screen, size, frame_stats, cx);
            }
            cx.activate(true);
            log::info!("ytdlpd: ready");
        });
    ExitCode::SUCCESS
}

/// Settings drive the tray behaviour; finished and failed downloads raise notifications.
fn wire_platform(state: &state::AppState, cx: &mut App) {
    cx.observe(&state.settings, |settings, cx| {
        platform::set_close_to_tray(settings.read(cx).get().close_to_tray, cx);
    })
    .detach();
    let settings = state.settings.clone();
    cx.subscribe(&state.downloads, move |downloads, event, cx| {
        if !settings.read(cx).get().notifications {
            return;
        }
        let title = |id| {
            downloads
                .read(cx)
                .get(id)
                .and_then(|r| r.title.clone())
                .unwrap_or_else(|| "ytdlpd".into())
        };
        match event {
            state::DownloadsEvent::Completed(id) => {
                platform::notifier(cx).notify(&title(*id), &i18n::lookup("notify-finished", None))
            }
            state::DownloadsEvent::Failed(id, error) => {
                // the localised cause when `err-*` has one, else the engine's own words
                let key = error.code.message_key();
                let text = i18n::translate(&key);
                let reason = match text.as_ref() == key {
                    true => error.message.clone(),
                    false => text.to_string(),
                };
                platform::notifier(cx).notify(&title(*id), &reason)
            }
            state::DownloadsEvent::Loaded => {}
        }
    })
    .detach();
}

/// Platform events become state: links go to `Intake`, quit pauses jobs first.
fn route_event(state: &state::AppState, event: platform::Event, cx: &mut App) {
    match event {
        platform::Event::AddUrls(urls) => state.intake.update(cx, |i, cx| i.add(&urls, cx)),
        platform::Event::ClipboardLinks(urls) => {
            state.intake.update(cx, |i, cx| i.offer(&urls, cx))
        }
        platform::Event::Show => {}
        platform::Event::Quit => {
            let done = state.shutdown(cx);
            cx.spawn(async move |cx| {
                done.await;
                cx.update(|cx| cx.quit());
            })
            .detach();
        }
    }
}

fn open_window(scene: Option<String>, first: Size<Pixels>, frame_stats: bool, cx: &mut App) {
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, first, cx))),
            titlebar: Some(TitlebarOptions {
                title: Some("ytdlpd".into()),
                appears_transparent: true,
                // vertically centred in the 36px title bar (ui metrics.title_bar)
                traffic_light_position: Some(gpui::point(px(12.), px(11.))),
            }),
            is_movable: true,
            is_resizable: true,
            app_owns_titlebar_drag: true,
            app_id: Some("ytdlpd".into()),
            window_min_size: Some(LEAST_SIZE),
            ..Default::default()
        },
        |window, cx| {
            views::apply_window(window, cx);
            platform::attach_window(window, cx);
            if frame_stats {
                platform::stats::start(window);
            }
            cx.new(|cx| Root::new(scene.as_deref(), window, cx))
        },
    )
    .expect("cannot open the window");
}

/// The engine services views call directly: filename preview and private cookie storage.
#[cfg(feature = "real-engine")]
fn engine_hooks() -> state::Hooks {
    use std::sync::Arc;
    let preview: state::PreviewFn = Arc::new(|spec| {
        engine::runtime::init(engine::runtime::InitOptions {
            home: None,
            site_updates: None,
            sink: engine::runtime::log_sink,
        })
        .map_err(|e| e.to_string())?;
        let info = serde_json::json!({
            "id": "dQw4w9WgXcQ", "title": "Never Gonna Give You Up", "ext": "mp4",
            "uploader": "Rick Astley", "upload_date": "20091025",
        });
        engine::preview_filename(spec, &engine::spec::Env::default(), &info).map_err(|e| e.message)
    });
    let store_cookies: state::StoreCookiesFn =
        Arc::new(|dir, text| engine::cookies::store_text(dir, text).map_err(|e| format!("{e:#}")));
    let yt_dlp_version: state::VersionFn = Arc::new(|| {
        engine::runtime::init(engine::runtime::InitOptions {
            home: None,
            site_updates: None,
            sink: engine::runtime::log_sink,
        })
        .ok()?;
        engine::runtime::yt_dlp_version()
    });
    state::Hooks {
        preview: Some(preview),
        store_cookies: Some(store_cookies),
        yt_dlp_version: Some(yt_dlp_version),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_size_parses_and_refuses_junk() {
        assert_eq!(
            "1280x800".parse::<WindowSize>().unwrap(),
            WindowSize(size(px(1280.), px(800.)))
        );
        assert!("1280".parse::<WindowSize>().is_err());
        assert!("0x5".parse::<WindowSize>().is_err());
        assert!("axb".parse::<WindowSize>().is_err());
    }
}
