//! The update card: a small window without a title bar that installs a downloaded update and opens
//! the app again (`--apply-update`, started by `state::updates` from a copy of the exe).
use std::process::ExitCode;
use std::sync::Arc;

use futures::StreamExt as _;
use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Image, ImageFormat, SharedString, Task, TitlebarOptions, Window,
    WindowBounds, WindowControlArea, WindowOptions, div, img, px, size,
};
use i18n::t;
use state::updates::apply::{Failure, Plan};
use ui::{ActiveTheme as _, Button, ProgressRule, Rule, Text, Theme};

const ICON: &[u8] =
    include_bytes!("../../../../../icons/ytdlpd/linux/hicolor/128x128/apps/ytdlpd.png");
/// The card's size at the default text size, working and failed (with its buttons).
const WIDTH: f32 = 380.;
const WORKING: f32 = 98.;
const FAILED: f32 = 126.;

/// Runs the card when the command line asks for it. `None` means start the app normally.
pub fn dispatch() -> Option<ExitCode> {
    let plan = Plan::from_args(std::env::args())?;
    gpui_platform::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| open(plan, cx));
    Some(ExitCode::SUCCESS)
}

fn open(plan: Plan, cx: &mut App) {
    if let Err(error) = crate::assets::Assets.load_fonts(cx) {
        log::error!("updates: cannot load bundled fonts: {error:#}");
    }
    i18n::set(i18n::resolve(i18n::AUTO));
    ui::motion::apply(Default::default(), Default::default(), cx);
    // shortcut: an app started with --data-dir shows the default theme here, put the dir in the plan if that matters
    let file = storage::Paths::resolve(None).settings_file();
    let settings = domain::Settings::load(&file, &Default::default()).settings;
    let mut look = views::look_of(&settings, None);
    (look.transparent, look.blur, look.blur_window) = (false, false, false);
    Theme::init(look, &views::overrides_of(&settings, None), cx);
    cx.on_window_closed(|cx, _| cx.quit()).detach();
    let scale = cx.theme().font_size / px(12.);
    let bounds = Bounds::centered(None, size(px(WIDTH * scale), px(WORKING * scale)), cx);
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("ytdlpd".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            is_resizable: false,
            is_minimizable: false,
            app_id: Some("ytdlpd".into()),
            ..Default::default()
        },
        |window, cx| {
            views::apply_window(window, cx);
            cx.new(|cx| Card::new(plan, window, cx))
        },
    );
    if let Err(error) = opened {
        log::error!("updates: cannot open the update window: {error:#}");
        cx.quit();
    }
    cx.activate(true);
}

struct Card {
    plan: Plan,
    percent: Option<u8>,
    failure: Option<Failure>,
    icon: Arc<Image>,
    task: Option<Task<()>>,
}

impl Card {
    fn new(plan: Plan, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            plan,
            percent: None,
            failure: None,
            icon: Arc::new(Image::from_bytes(ImageFormat::Png, ICON.to_vec())),
            task: None,
        };
        this.install(window, cx);
        this
    }

    fn fit(&self, height: f32, window: &mut Window, cx: &App) {
        let scale = cx.theme().font_size / px(12.);
        window.resize(size(px(WIDTH * scale), px(height * scale)));
    }

    fn install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        (self.percent, self.failure) = (None, None);
        self.fit(WORKING, window, cx);
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let plan = self.plan.clone();
        let work = cx.background_executor().spawn(async move {
            plan.run(|percent| {
                tx.unbounded_send(percent).ok();
            })
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            while let Some(percent) = rx.next().await {
                this.update(cx, |this, cx| {
                    if this.percent.replace(percent) != Some(percent) {
                        cx.notify();
                    }
                })
                .ok();
            }
            let result = work.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(()) => this.leave(this.plan.relaunch, cx),
                Err(failure) => {
                    this.failure = Some(failure);
                    this.fit(FAILED, window, cx);
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    fn leave(&self, open: bool, cx: &mut App) {
        if open && let Err(error) = self.plan.open_app() {
            log::warn!("updates: {error:#}");
        }
        cx.quit();
    }

    fn words(&self) -> (SharedString, SharedString) {
        let plan = &self.plan;
        match &self.failure {
            Some(failure) => (
                t!("update-card-failed"),
                match failure {
                    Failure::Declined => t!("update-card-declined", version = plan.from.clone()),
                    Failure::StillRunning => t!("update-card-running"),
                    Failure::Start => t!("update-card-start"),
                    Failure::Exit(code) => t!("update-card-exit", code = code.to_string()),
                },
            ),
            None => (
                t!("update-installing", version = plan.to.clone()),
                match (self.percent, plan.relaunch) {
                    (_, false) => t!("update-card-closes"),
                    (Some(_), true) => t!("update-card-copying"),
                    (None, true) => t!("update-card-reopens"),
                },
            ),
        }
    }

    fn buttons(&self, cx: &Context<Self>) -> Vec<Button> {
        let open = self.plan.relaunch;
        let log = self.plan.log();
        let show_log = log.is_file().then(|| {
            Button::new("log")
                .ghost()
                .label(t!("update-card-log"))
                .on_click(move |_, _, cx| cx.open_with_system(&log))
        });
        let leave = Button::new("leave")
            .outline()
            .label(match open {
                true => t!("update-card-open", version = self.plan.from.clone()),
                false => t!("update-card-close"),
            })
            .on_click(cx.listener(move |this, _, _, cx| this.leave(open, cx)));
        let again = Button::new("again")
            .primary()
            .label(t!("update-try-again"))
            .on_click(cx.listener(|this, _, window, cx| this.install(window, cx)));
        show_log.into_iter().chain([leave, again]).collect()
    }
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let (title, detail) = self.words();
        let failed = self.failure.is_some();
        let rule = match self.percent {
            Some(p) => Rule::Fill(f32::from(p) / 100.),
            None => Rule::Pulse,
        };
        let shown = self.percent.filter(|_| !failed);
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(14.))
            .p(px(16.))
            .border_1()
            .border_color(theme.title_bar_border)
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(theme.text(Text::Body))
            .font_family(views::UI_FONT)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .window_control_area(WindowControlArea::Drag)
                    .child(img(self.icon.clone()).size(px(48.)).flex_none())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(theme.text(Text::Heading))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .when(failed, |this| this.text_color(theme.danger))
                                    .child(title.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .gap(px(12.))
                                    .text_size(theme.text(Text::Small))
                                    .text_color(theme.muted_foreground)
                                    .child(detail)
                                    .children(shown.map(|p| {
                                        div()
                                            .text_color(theme.progress_bar)
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .font_features(ui::tabular())
                                            .child(format!("{p}%"))
                                    })),
                            ),
                    ),
            )
            .map(|this| match failed {
                true => this.child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(6.))
                        .children(self.buttons(cx)),
                ),
                false => this.child(ProgressRule::new(rule).spoken("progress", title)),
            })
    }
}
