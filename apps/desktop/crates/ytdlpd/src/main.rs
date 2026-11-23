#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod platform;

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

/// What the state layer builds at start-up and the views read. Lane C replaces this with the
/// real handle (fake or engine-backed) inside `build_app`.
// shortcut: empty placeholder until `state` exists, replace in Phase 0b (lane C).
struct Backend;

/// The one place state is wired up, so Lane C can plug the backend in without touching `main`.
fn build_app(_data_dir: Option<PathBuf>, _cx: &mut App) -> Backend {
    Backend
}

fn main() -> ExitCode {
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
                kind: ThemeKind::Dark,
                rounding: Rounding::Rounded,
                font: 14.,
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

            let _backend = build_app(args.data_dir.clone(), cx);
            router::init(Destination::Downloads, cx);
            let size = args.window_size.map(|size| size.0).unwrap_or(FIRST_SIZE);
            let frame_stats = args.launch.frame_stats;
            // Lane C routes `on_event` into state; for now links are only logged.
            let hooks = platform::Hooks {
                open_window: Box::new(move |cx| open_window(size, frame_stats, cx)),
                on_event: Box::new(|event, _cx| log::info!("ytdlpd: platform event {event:?}")),
            };
            // release builds with the real data dir register the ytdlpd:// scheme
            let register = !cfg!(debug_assertions) && args.data_dir.is_none();
            if platform.install(hooks, register, cx) {
                open_window(size, frame_stats, cx);
            }
            cx.activate(true);
            log::info!("ytdlpd: ready");
        });
    ExitCode::SUCCESS
}

fn open_window(first: Size<Pixels>, frame_stats: bool, cx: &mut App) {
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
            cx.new(|cx| Root::new(window, cx))
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
