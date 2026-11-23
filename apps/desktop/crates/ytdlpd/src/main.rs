#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
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
    /// Run against fake data: an in-memory database, a fake engine and fake tools.
    #[arg(long, value_name = "SCENE", value_parser = clap::builder::PossibleValuesParser::new(state::Scene::NAMES))]
    demo: Option<String>,
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
    /// Theme: `dark` (default) or `light`.
    #[arg(long, default_value = "dark")]
    theme: String,
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
/// `--data-dir`), fake or real engine, and every entity.
fn build_app(data_dir: Option<PathBuf>, demo: Option<&str>, cx: &mut App) -> state::AppState {
    let args = state::InitArgs {
        data_dir,
        demo: demo.and_then(state::Scene::parse),
        // TODO(real-engine): construct the engine `Backend` here once the engine crate lands;
        // until then both builds run on `FakeBackend`.
        backend: None,
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
                kind: if args.theme == "light" {
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

            let _state = build_app(args.data_dir.clone(), args.demo.as_deref(), cx);
            router::init(Destination::Downloads, cx);
            let size = args.window_size.map(|size| size.0).unwrap_or(FIRST_SIZE);
            let frame_stats = args.launch.frame_stats;
            let scene = args.screen.clone();
            // Lane C routes `on_event` into state; for now links are only logged.
            let hooks = platform::Hooks {
                open_window: Box::new(move |cx| open_window(scene.clone(), size, frame_stats, cx)),
                on_event: Box::new(|event, _cx| log::info!("ytdlpd: platform event {event:?}")),
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

fn open_window(scene: Option<String>, first: Size<Pixels>, frame_stats: bool, cx: &mut App) {
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, first, cx))),
            titlebar: Some(TitlebarOptions {
                title: Some("ytdlpd".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            is_movable: true,
            is_resizable: true,
            app_owns_titlebar_drag: true,
            app_id: Some("ytdlpd".into()),
            window_min_size: Some(LEAST_SIZE),
            ..Default::default()
        },
        |window, cx| {
            use ui::ActiveTheme as _;
            window.set_rem_size(cx.theme().font_size);
            platform::attach_window(window, cx);
            if frame_stats {
                platform::stats::start(window);
            }
            cx.new(|cx| Root::new(scene.as_deref(), window, cx))
        },
    )
    .expect("cannot open the window");
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
