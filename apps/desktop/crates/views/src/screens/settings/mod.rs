//! The Settings screen: a side list of pages beside a form, every control bound to
//! `state::AppSettings` (settings.json), so edits in the UI and edits of the file show in both.

mod fields;
mod layout;
mod look;
mod miniature;
mod pages;
mod themes;

pub use look::{apply_stored, apply_window};

use std::path::PathBuf;
use std::rc::Rc;

use domain::{CookiesMode, Preset, Settings, ToolKind, ToolSource};
use fields::{Field, Invalid, is_cookies_text};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, PathPromptOptions, SharedString, Task, Window,
    div, px,
};
use i18n::t;
use router::{Destination, Navigation, NavigationEvent, SettingsPage};
use state::{AppSettings, AppState, Hooks, Tools, ToolsEvent};
use ui::{
    ActiveTheme as _, Chip, Input, Look, MONO, Notice, Popovers, Scrollbar, Scroller,
    ScrubberState, SideNav, SideNavItem, Stepper, SwitchRow, Text,
};

use crate::toasts::{Kind, toast};

/// The category ids SponsorBlock knows, with the Fluent key of their label.
const SPONSOR_CATEGORIES: [(&str, &str); 7] = [
    ("sponsor", "set-sb-sponsor"),
    ("intro", "set-sb-intro"),
    ("outro", "set-sb-outro"),
    ("selfpromo", "set-sb-selfpromo"),
    ("interaction", "set-sb-interaction"),
    ("preview", "set-sb-preview"),
    ("music_offtopic", "set-sb-music"),
];

/// Browsers yt-dlp can read cookies from that we name in the UI.
const BROWSERS: [(&str, &str); 3] = [
    ("firefox", "set-browser-firefox"),
    ("chrome", "set-browser-chrome"),
    ("edge", "set-browser-edge"),
];

type Edit = Rc<dyn Fn(&mut Settings)>;

/// Below this window width the side list shows icons only.
const NARROW: f32 = 900.;

struct FieldState {
    field: Field,
    input: Entity<Input>,
    /// What `Field::show` last gave, to tell an external change from the user's own typing.
    shown: String,
    error: Option<Invalid>,
}

/// The Settings page entity.
pub struct SettingsView {
    state: AppState,
    navigation: Entity<Navigation>,
    fields: Vec<FieldState>,
    popovers: Popovers,
    /// The outcome of the last cookie action, `true` when it failed.
    cookie_note: Option<(SharedString, bool)>,
    /// The theme and font size the window shows now, so only a change repaints.
    applied: Look,
    /// The theme id `applied` was drawn from (theme files share the dark `Look`).
    applied_theme: String,
    /// The theme family whose variant menu the Appearance page holds.
    theme_variants: Option<&'static str>,
    opacity: ScrubberState,
    language: String,
    /// The look the window was last given, so a frame re-applies only after a change.
    window_look: Option<Look>,
    /// The sample file name for the current template, or why it failed.
    preview: Option<Result<String, String>>,
    preview_task: Option<Task<()>>,
    preview_inputs: (String, String),
    /// Whether the last frame showed the Downloads page, so entering it is told apart.
    on_downloads: bool,
    bar: Entity<Scrollbar>,
    /// The version of the bundled yt-dlp, once the engine told.
    ytdlp_version: Option<String>,
}

impl SettingsView {
    /// Builds the page over the registered `AppState`.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::global(cx);
        let navigation = router::trail(cx);
        cx.subscribe(&navigation, |_, _, _: &NavigationEvent, cx| cx.notify())
            .detach();
        cx.observe(&state.settings, |this, _, cx| this.settings_changed(cx))
            .detach();
        cx.subscribe(&state.tools, |this, _, event: &ToolsEvent, cx| {
            tool_toast(event, cx);
            if let ToolsEvent::Updated {
                result: Ok(update), ..
            } = event
            {
                this.ytdlp_version = Some(update.version.clone());
            }
        })
        .detach();
        cx.observe(&state.tools, |this, tools, cx| {
            this.remember_custom_tools(&tools, cx);
            cx.notify()
        })
        .detach();

        let current = state.settings.read(cx).get().clone();
        let fields = Field::ALL
            .into_iter()
            .map(|field| {
                let numeric = matches!(field, Field::Global | Field::PerDomain | Field::Retries);
                let hint = match field {
                    Field::Proxy => "set-proxy-hint",
                    Field::Rate => "set-rate-hint",
                    Field::Langs => "set-subtitles-langs-hint",
                    _ => "",
                };
                let shown = field.show(&current);
                let input = cx.new(|cx| {
                    let mut input = Input::new(hint, cx);
                    if numeric {
                        input = input.joined().align_center();
                    }
                    input
                });
                input.update(cx, |input, cx| input.set_text(shown.clone(), cx));
                cx.observe(&input, move |this, _, cx| this.typed(field, cx))
                    .detach();
                FieldState {
                    field,
                    input,
                    shown,
                    error: None,
                }
            })
            .collect();
        let bar = cx.new(|cx| Scrollbar::new(gpui::ScrollHandle::new()).watching(cx.entity_id()));
        let mut this = Self {
            state,
            navigation,
            fields,
            popovers: Popovers::default(),
            cookie_note: None,
            applied: look::look_of(&current, None),
            applied_theme: current.theme.clone(),
            theme_variants: None,
            opacity: ScrubberState::new("set-opacity"),
            language: current.language,
            window_look: None,
            preview: None,
            preview_task: None,
            preview_inputs: (current.outtmpl.clone(), current.output_dir.clone()),
            on_downloads: false,
            bar,
            ytdlp_version: None,
        };
        this.refresh_preview(cx);
        this.load_ytdlp_version(cx);
        this
    }

    /// Asks the engine, off the UI thread, which yt-dlp it runs.
    fn load_ytdlp_version(&mut self, cx: &mut Context<Self>) {
        let Some(version) = cx
            .try_global::<Hooks>()
            .and_then(|h| h.yt_dlp_version.clone())
        else {
            return;
        };
        let io = self.state.io.clone();
        cx.spawn(async move |this, cx| {
            let found = io.spawn_blocking(move || version()).await.ok().flatten();
            this.update(cx, |this, cx| {
                this.ytdlp_version = found;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn settings(&self) -> Entity<AppSettings> {
        self.state.settings.clone()
    }

    fn field(&self, field: Field) -> &FieldState {
        self.fields
            .iter()
            .find(|f| f.field == field)
            .expect("every field is built")
    }

    /// The user typed into `field`: write it back when valid, mark it when not.
    fn typed(&mut self, field: Field, cx: &mut Context<Self>) {
        let text = self.field(field).input.read(cx).text().to_owned();
        let mut outcome = Ok(());
        self.settings().update(cx, |s, cx| {
            s.update(|st| outcome = field.apply(&text, st), cx)
        });
        let state = self
            .fields
            .iter_mut()
            .find(|f| f.field == field)
            .expect("every field is built");
        state.error = outcome.err();
        state
            .input
            .update(cx, |input, cx| input.set_invalid(outcome.is_err(), cx));
        cx.notify();
    }

    /// The settings changed (own edit or the file): refresh fields that moved, theme, language.
    fn settings_changed(&mut self, cx: &mut Context<Self>) {
        let current = self.settings().read(cx).get().clone();
        if self.preview_inputs != (current.outtmpl.clone(), current.output_dir.clone()) {
            self.preview_inputs = (current.outtmpl.clone(), current.output_dir.clone());
            self.refresh_preview(cx);
        }
        for state in &mut self.fields {
            let shown = state.field.show(&current);
            if shown != state.shown {
                if state.input.read(cx).text().trim() != shown {
                    state.input.update(cx, |input, cx| {
                        input.set_text(shown.clone(), cx);
                        input.set_invalid(false, cx);
                    });
                    state.error = None;
                }
                state.shown = shown;
            }
        }
        let look = look::look_of(&current, None);
        if look != self.applied || current.theme != self.applied_theme {
            self.applied = look;
            self.applied_theme.clone_from(&current.theme);
            look::apply_look(look, &look::overrides_of(&current, None), cx);
            // the window background follows on the next frame
            self.window_look = None;
        }
        if current.language != self.language {
            self.language = current.language.clone();
            i18n::set(i18n::resolve(&current.language));
        }
        cx.notify();
    }

    /// A tool the user pointed at a file stays pointed at it after a restart.
    fn remember_custom_tools(&mut self, tools: &Entity<Tools>, cx: &mut Context<Self>) {
        let custom = |kind| {
            tools
                .read(cx)
                .get(kind)
                .filter(|s| s.source == Some(ToolSource::Custom))
                .and_then(|s| s.path.clone())
        };
        let (ffmpeg, ffprobe, deno) = (
            custom(ToolKind::Ffmpeg),
            custom(ToolKind::Ffprobe),
            custom(ToolKind::Deno),
        );
        self.settings().update(cx, |s, cx| {
            s.update(
                |st| {
                    for (slot, path) in [
                        (&mut st.tool_paths.ffmpeg, ffmpeg),
                        (&mut st.tool_paths.ffprobe, ffprobe),
                        (&mut st.tool_paths.deno, deno),
                    ] {
                        if path.is_some() && *slot != path {
                            *slot = path;
                        }
                    }
                },
                cx,
            )
        });
    }

    /// A click handler that edits the settings.
    fn edit(
        &self,
        edit: impl Fn(&mut Settings) + 'static,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let settings = self.settings();
        move |_, _, cx| settings.update(cx, |s, cx| s.update(&edit, cx))
    }

    fn switch(
        &self,
        id: &'static str,
        key: &'static str,
        on: bool,
        set: fn(&mut Settings, bool),
    ) -> AnyElement {
        // the row fills its cell edge to edge, so its hover and focus have no gap at the sides
        SwitchRow::new(id, i18n::lookup(key, None), on)
            .detail(i18n::lookup(&format!("{key}-desc"), None))
            .mx(px(-12.))
            .px(px(12.))
            .py(px(10.))
            .min_h(px(52.))
            .rounded(px(0.))
            .on_click(self.edit(move |s| set(s, !on)))
            .into_any_element()
    }

    /// A single-choice row of chips.
    fn chips(
        &self,
        row: &'static str,
        choices: impl IntoIterator<Item = (&'static str, &'static str, bool, Edit)>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .children(choices.into_iter().map(|(id, key, selected, set)| {
                Chip::new(
                    SharedString::from(format!("{row}-{id}")),
                    i18n::lookup(key, None),
                )
                .selected(selected)
                .on_click(self.edit(move |s| set(s)))
            }))
            .into_any_element()
    }

    /// The text field plus its inline error.
    fn input(&self, field: Field, width: f32, cx: &App) -> AnyElement {
        let state = self.field(field);
        let theme = *cx.theme();
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .w(px(width))
            .child(state.input.clone())
            .when_some(state.error, |this, key| {
                this.child(
                    div()
                        .text_size(theme.text(Text::Small))
                        .text_color(theme.danger)
                        .child(i18n::lookup(key, None)),
                )
            })
            .into_any_element()
    }

    /// A number field between minus and plus buttons, one bordered group, its error under it.
    fn stepper(&self, field: Field, cx: &App) -> AnyElement {
        let state = self.field(field);
        let theme = *cx.theme();
        let settings = self.settings();
        div()
            .flex()
            .flex_col()
            .items_end()
            .gap(px(4.))
            .child(
                Stepper::new(
                    SharedString::from(format!("{field:?}-step")),
                    &state.input,
                    px(48.),
                )
                .invalid(state.error.is_some())
                .on_step(move |delta, _, cx| {
                    settings.update(cx, |s, cx| s.update(|s| field.bump(delta, s), cx))
                }),
            )
            .when_some(state.error, |this, key| {
                this.child(
                    div()
                        .text_size(theme.text(Text::Small))
                        .text_color(theme.danger)
                        .child(i18n::lookup(key, None)),
                )
            })
            .into_any_element()
    }

    fn value_box(&self, text: impl Into<SharedString>, cx: &App) -> AnyElement {
        let theme = *cx.theme();
        div()
            .px(px(10.))
            .h(theme.metrics.control)
            .flex()
            .items_center()
            .min_w(px(260.))
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary)
            .font_family(MONO)
            .text_size(theme.text(Text::Small))
            .child(text.into())
            .into_any_element()
    }

    fn choose_folder(&self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        let settings = self.settings();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(path) = paths.first()
            {
                let dir = path.to_string_lossy().into_owned();
                cx.update(|cx| settings.update(cx, |s, cx| s.update(|st| st.output_dir = dir, cx)));
            }
        })
        .detach();
    }

    fn choose_file(
        &self,
        then: impl FnOnce(&mut Self, PathBuf, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(path) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| then(this, path, cx)).ok();
            }
        })
        .detach();
    }

    /// Stores `text` as the cookies file under the data directory and selects it.
    fn import_cookies(&mut self, text: &str, cx: &mut Context<Self>) {
        if !is_cookies_text(text) {
            self.cookie_note = Some((t!("set-cookies-bad"), true));
            return cx.notify();
        }
        let data = &self.state.paths.data;
        // the engine stores the file owner-only; without it (no-engine build) a plain copy
        let stored = match cx
            .try_global::<Hooks>()
            .and_then(|h| h.store_cookies.clone())
        {
            Some(store) => store(data, text),
            None => {
                let file = data.join("cookies").join("cookies.txt");
                std::fs::create_dir_all(data.join("cookies"))
                    .and_then(|_| std::fs::write(&file, text))
                    .map(|_| file)
                    .map_err(|e| e.to_string())
            }
        };
        match stored {
            Ok(file) => {
                let path = file.to_string_lossy().into_owned();
                self.settings().update(cx, |s, cx| {
                    s.update(
                        |st| {
                            st.cookies.mode = CookiesMode::File;
                            st.cookies.file = Some(path);
                        },
                        cx,
                    )
                });
                self.cookie_note = Some((t!("set-cookies-saved"), false));
            }
            Err(error) => {
                log::warn!("settings: cannot save cookies: {error}");
                self.cookie_note = Some((t!("set-cookies-save-failed"), true));
            }
        }
        cx.notify();
    }

    /// Recomputes the file name preview off the UI thread; the newest request wins.
    fn refresh_preview(&mut self, cx: &mut Context<Self>) {
        let Some(preview) = cx.try_global::<Hooks>().and_then(|h| h.preview.clone()) else {
            return;
        };
        let settings = self.settings();
        let (spec, dir) = {
            let s = settings.read(cx);
            let spec = s.job_spec(
                "https://example.com/watch?v=dQw4w9WgXcQ",
                Preset::DEFAULT,
                "",
                None,
            );
            (spec, s.get().output_dir.clone())
        };
        let io = self.state.io.clone();
        self.preview_task = Some(cx.spawn(async move |this, cx| {
            let result = io
                .spawn_blocking(move || preview(&spec))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            if let Err(error) = &result {
                log::warn!("settings: cannot preview the file name: {error}");
            }
            this.update(cx, |this, cx| {
                this.preview = Some(result.map(|path| {
                    let path = std::path::Path::new(&path);
                    let shown = path.strip_prefix(&dir).unwrap_or(path);
                    shown.to_string_lossy().into_owned()
                }));
                cx.notify();
            })
            .ok();
        }));
    }

    fn clear_cookies(&mut self, cx: &mut Context<Self>) {
        let file = self.settings().read(cx).get().cookies.file.clone();
        if let Some(file) = file {
            std::fs::remove_file(file).ok();
        }
        self.settings().update(cx, |s, cx| {
            s.update(
                |st| {
                    st.cookies.mode = CookiesMode::None;
                    st.cookies.file = None;
                },
                cx,
            )
        });
        self.cookie_note = Some((t!("set-cookies-cleared"), false));
        cx.notify();
    }
}

/// Tells the outcome of a check or a re-download.
fn tool_toast(event: &ToolsEvent, cx: &mut App) {
    let (kind, message) = match event {
        ToolsEvent::UpdateChecked { kind, latest } => {
            let name = i18n::lookup(tool_key(*kind), None);
            match latest {
                Ok(Some(version)) => {
                    let (kind, tools) = (*kind, AppState::global(cx).tools);
                    let message = t!(
                        "set-tool-update-available",
                        name = &name,
                        version = version.clone()
                    );
                    let install = std::rc::Rc::new(move |cx: &mut App| {
                        tools.update(cx, |t, cx| t.install_update(kind, cx));
                        toast(cx, Kind::Info, t!("set-tool-update-installing"), None);
                    });
                    toast(
                        cx,
                        Kind::Info,
                        message,
                        Some((t!("set-tool-update-install"), install)),
                    );
                    return;
                }
                Ok(None) => (Kind::Success, t!("set-tool-update-latest", name = &name)),
                Err(_) => (Kind::Error, t!("set-tool-update-failed", name = &name)),
            }
        }
        ToolsEvent::Updated { kind, result } => {
            let name = i18n::lookup(tool_key(*kind), None);
            match result {
                Ok(update) => (
                    Kind::Success,
                    t!(
                        "set-tool-update-done",
                        name = &name,
                        version = update.version.clone()
                    ),
                ),
                Err(error) => (
                    Kind::Error,
                    t!(
                        "set-tool-update-failed-kept",
                        name = &name,
                        reason = error.message.clone()
                    ),
                ),
            }
        }
        ToolsEvent::Reinstalled(kind) => {
            let name = i18n::lookup(tool_key(*kind), None);
            (Kind::Success, t!("set-tool-redone", name = &name))
        }
    };
    toast(cx, kind, message, None);
}

/// Whether opening the Downloads page should ask for the preview again. The page is built with
/// the window at launch, and a preview that failed or never arrived then would otherwise stay
/// until the template or folder changed (the user had to clear and retype the template).
fn retry_preview(entering: bool, preview: &Option<Result<String, String>>) -> bool {
    entering && !matches!(preview, Some(Ok(_)))
}

/// The icon of a settings page in the side list.
fn page_icon(page: SettingsPage) -> &'static str {
    match page {
        SettingsPage::General => "icons/sliders-horizontal.svg",
        SettingsPage::Downloads => "icons/download.svg",
        SettingsPage::Formats => "icons/film.svg",
        SettingsPage::Network => "icons/radio.svg",
        SettingsPage::Tools => "icons/settings.svg",
        SettingsPage::Appearance => "icons/palette.svg",
        SettingsPage::About => "icons/info.svg",
    }
}

fn tool_key(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::YtDlp => "welcome-tool-yt-dlp",
        ToolKind::YtDlpEjs => "welcome-tool-yt-dlp-ejs",
        ToolKind::Ffmpeg => "welcome-tool-ffmpeg",
        ToolKind::Ffprobe => "welcome-tool-ffprobe",
        ToolKind::Deno => "welcome-tool-deno",
        ToolKind::Python => "welcome-tool-python",
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let narrow = window.viewport_size().width < px(NARROW);
        if self.window_look != Some(self.applied) {
            self.window_look = Some(self.applied);
            look::apply_window(window, cx);
        }
        let page = match self.navigation.read(cx).current() {
            Destination::Settings(page) => page,
            _ => SettingsPage::General,
        };
        let entering = page == SettingsPage::Downloads && !self.on_downloads;
        self.on_downloads = page == SettingsPage::Downloads;
        if retry_preview(entering, &self.preview) {
            self.refresh_preview(cx);
        }
        let s = self.settings().read(cx).get().clone();
        let broken = self.settings().read(cx).error().is_some();
        let nav = SideNav::new(
            SettingsPage::ALL
                .into_iter()
                .map(|p| SideNavItem {
                    label: i18n::lookup(p.key(), None),
                })
                .collect(),
            SettingsPage::ALL
                .iter()
                .position(|p| *p == page)
                .unwrap_or(0),
            |index, _, cx| router::navigate(Destination::Settings(SettingsPage::ALL[index]), cx),
        )
        .icons(SettingsPage::ALL.map(page_icon))
        .narrow(narrow);
        let sections = match page {
            SettingsPage::General => self.page_general(&s, cx),
            SettingsPage::Downloads => self.page_downloads(&s, cx),
            SettingsPage::Formats => self.page_formats(&s, cx),
            SettingsPage::Network => self.page_network(&s, cx),
            SettingsPage::Tools => self.page_tools(cx),
            SettingsPage::Appearance => self.page_appearance(&s, cx),
            SettingsPage::About => self.page_about(cx),
        };
        let notice = broken.then(|| {
            Notice::new(t!("set-file-error-title"), t!("set-file-error"))
                .failed()
                .into_any_element()
        });
        div()
            .flex()
            .size_full()
            .child(nav)
            .child(div().flex_1().min_w_0().child(
                Scroller::new("settings-scroll", &self.bar).child(layout::page(
                    i18n::lookup(page.key(), None),
                    notice,
                    sections,
                    cx,
                )),
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::retry_preview;

    #[test]
    fn a_failed_or_missing_preview_is_asked_again_on_open() {
        assert!(retry_preview(true, &Some(Err("first call failed".into()))));
        assert!(retry_preview(true, &None));
        assert!(!retry_preview(true, &Some(Ok("a.mp4".into()))));
        // staying on the page never loops on a failure
        assert!(!retry_preview(false, &Some(Err("x".into()))));
    }
}
