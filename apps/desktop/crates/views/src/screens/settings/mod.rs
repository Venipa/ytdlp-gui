//! The Settings screen: a side list of pages beside a form, every control bound to
//! `state::AppSettings` (settings.json), so edits in the UI and edits of the file show in both.

mod fields;

use std::path::PathBuf;
use std::rc::Rc;

use domain::{
    CookiesMode, Preset, Settings, SponsorBlockMode, ToolKind, ToolSource, ToolState, ToolStatus,
};
use fields::{Field, Invalid, is_cookies_text};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, FontWeight, PathPromptOptions, SharedString,
    Window, div, px,
};
use i18n::t;
use router::{Destination, Navigation, NavigationEvent, SettingsPage};
use state::{AppSettings, AppState, Tools};
use ui::{
    ActiveTheme as _, Button, Chip, Input, Look, MONO, MenuItem, Notice, Picker, Popovers,
    ProgressRule, Rounding, Rule, SideNav, SideNavItem, SwitchRow, Text, Theme, ThemeKind,
    ThemeOverrides, tabular,
};

use crate::format;

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
    applied: (String, u32),
    language: String,
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
        cx.observe(&state.tools, |this, tools, cx| {
            this.remember_custom_tools(&tools, cx);
            cx.notify()
        })
        .detach();

        let current = state.settings.read(cx).get().clone();
        let fields = Field::ALL
            .into_iter()
            .map(|field| {
                let numeric = matches!(
                    field,
                    Field::Global | Field::PerDomain | Field::Retries | Field::FontSize
                );
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
                        input = input.align_end();
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
        Self {
            state,
            navigation,
            fields,
            popovers: Popovers::default(),
            cookie_note: None,
            applied: (current.theme.clone(), current.font_size as u32),
            language: current.language,
        }
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
        let look = (current.theme.clone(), current.font_size as u32);
        if look != self.applied {
            self.applied = look;
            let look = Look {
                kind: ThemeKind::from_id(&current.theme),
                rounding: Rounding::Subtle,
                font: current.font_size,
                transparent: false,
                transparency: 1.,
                blur: false,
                blur_window: false,
                tint: None,
                tint_secondary: None,
            };
            Theme::set(look, &ThemeOverrides::default(), cx);
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
        SwitchRow::new(id, i18n::lookup(key, None), on)
            .on_click(self.edit(move |s| set(s, !on)))
            .into_any_element()
    }

    /// A single-choice row of chips.
    fn chips<const N: usize>(
        &self,
        row: &'static str,
        choices: [(&'static str, &'static str, bool, Edit); N],
    ) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap_2()
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

    fn row(
        &self,
        label: impl Into<SharedString>,
        control: impl IntoElement,
        cx: &App,
    ) -> AnyElement {
        let theme = *cx.theme();
        div()
            .flex()
            .items_center()
            .gap_3()
            .min_h(px(32.))
            .child(
                div()
                    .w(px(200.))
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(label.into()),
            )
            .child(control)
            .into_any_element()
    }

    /// The text field plus its inline error.
    fn input(&self, field: Field, width: f32, cx: &App) -> AnyElement {
        let state = self.field(field);
        let theme = *cx.theme();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(width))
            .child(state.input.clone())
            .when_some(state.error, |this, key| {
                this.child(
                    div()
                        .text_size(theme.text(Text::Label))
                        .text_color(theme.danger)
                        .child(i18n::lookup(key, None)),
                )
            })
            .into_any_element()
    }

    /// A number field with minus and plus buttons around it.
    fn stepper(&self, field: Field, cx: &App) -> AnyElement {
        let step = |id: String, label: &'static str, delta: i32| {
            Button::new(SharedString::from(id))
                .outline()
                .small()
                .label(label)
                .on_click(self.edit(move |s| field.bump(delta, s)))
        };
        div()
            .flex()
            .items_start()
            .gap_1()
            .child(step(format!("{field:?}-minus"), "\u{2212}", -1))
            .child(self.input(field, 64., cx))
            .child(step(format!("{field:?}-plus"), "+", 1))
            .into_any_element()
    }

    fn value_box(&self, text: impl Into<SharedString>, cx: &App) -> AnyElement {
        let theme = *cx.theme();
        div()
            .px_2()
            .h(theme.metrics.field)
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
        let dir = self.state.paths.data.join("cookies");
        let file = dir.join("cookies.txt");
        let written = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, text));
        match written {
            Ok(()) => {
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

    fn page_general(&self, s: &Settings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let language = Picker::plain(
            "set-language",
            &self.popovers,
            match s.language.as_str() {
                "en-US" => t!("set-language-en"),
                _ => t!("set-language-auto"),
            },
        )
        .left()
        .items(
            [
                ("auto", "set-language-auto", s.language == "auto"),
                ("en-US", "set-language-en", s.language == "en-US"),
            ]
            .map(|(id, key, on)| {
                MenuItem::new(
                    SharedString::from(format!("lang-{id}")),
                    i18n::lookup(key, None),
                )
                .checked(on)
                .on_click(self.edit(move |st| st.language = id.to_owned()))
            }),
        );
        vec![
            self.row(t!("set-language"), language, cx),
            self.switch("s-min", "set-start-minimized", s.start_minimized, |s, v| {
                s.start_minimized = v
            }),
            self.switch("s-tray", "set-close-to-tray", s.close_to_tray, |s, v| {
                s.close_to_tray = v
            }),
            self.switch("s-notify", "set-notifications", s.notifications, |s, v| {
                s.notifications = v
            }),
            self.switch(
                "s-clip",
                "set-clipboard-watch",
                s.clipboard_watch,
                |s, v| s.clipboard_watch = v,
            ),
            self.switch(
                "s-clip-auto",
                "set-clipboard-auto",
                s.clipboard_auto_add,
                |s, v| s.clipboard_auto_add = v,
            ),
            self.switch(
                "s-resume",
                "set-resume-on-start",
                s.resume_on_start,
                |s, v| s.resume_on_start = v,
            ),
        ]
    }

    fn page_downloads(&self, s: &Settings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = *cx.theme();
        let folder = div()
            .flex()
            .gap_2()
            .child(self.value_box(s.output_dir.clone(), cx))
            .child(
                Button::new("choose-dir")
                    .outline()
                    .label(t!("set-choose"))
                    .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
            );
        vec![
            self.row(t!("set-save-to"), folder, cx),
            self.row(
                t!("set-file-name"),
                self.input(Field::Template, 420., cx),
                cx,
            ),
            // shortcut: no live preview until engine::preview_filename lands, see plan.md Phase 4
            self.row(
                t!("set-preview"),
                div()
                    .text_size(theme.text(Text::Small))
                    .text_color(theme.muted_foreground)
                    .child(t!("set-preview-pending")),
                cx,
            ),
            self.row(
                t!("set-at-once"),
                div()
                    .flex()
                    .gap_3()
                    .items_start()
                    .child(self.stepper(Field::Global, cx))
                    .child(
                        div()
                            .h(px(32.))
                            .flex()
                            .items_center()
                            .text_color(theme.muted_foreground)
                            .child(t!("set-per-site")),
                    )
                    .child(self.stepper(Field::PerDomain, cx)),
                cx,
            ),
            self.row(t!("set-retries"), self.stepper(Field::Retries, cx), cx),
            self.switch(
                "d-thumbs",
                "set-show-thumbnails",
                s.show_thumbnails,
                |s, v| {
                    s.show_thumbnails = v;
                    s.compact_mode = !v;
                },
            ),
        ]
    }

    fn page_formats(&self, s: &Settings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let preset = Picker::plain("set-preset", &self.popovers, s.last_preset.clone())
            .left()
            .scroll_after(px(280.))
            .items(Preset::all().into_iter().map(|p| {
                let id = p.to_string();
                MenuItem::new(SharedString::from(format!("preset-{id}")), id.clone())
                    .checked(id == s.last_preset)
                    .on_click(self.edit(move |st| st.last_preset = id.clone()))
            }));
        let sb = s.sponsorblock.mode;
        let mode = |id, key, m: SponsorBlockMode| {
            (
                id,
                key,
                sb == m,
                Rc::new(move |st: &mut Settings| st.sponsorblock.mode = m) as Edit,
            )
        };
        let mut rows = vec![
            self.row(t!("set-default-preset"), preset, cx),
            self.switch(
                "f-subs",
                "set-subtitles-enable",
                s.subtitles.enabled,
                |s, v| s.subtitles.enabled = v,
            ),
            self.row(
                t!("set-subtitles-langs"),
                self.input(Field::Langs, 260., cx),
                cx,
            ),
            self.switch(
                "f-subs-embed",
                "set-subtitles-embed",
                s.subtitles.embed,
                |s, v| s.subtitles.embed = v,
            ),
            self.switch(
                "f-thumb",
                "set-embed-thumbnail",
                s.embed_thumbnail,
                |s, v| s.embed_thumbnail = v,
            ),
            self.switch("f-meta", "set-embed-metadata", s.embed_metadata, |s, v| {
                s.embed_metadata = v
            }),
            self.switch("f-chap", "set-embed-chapters", s.embed_chapters, |s, v| {
                s.embed_chapters = v
            }),
            self.row(
                t!("set-sponsorblock"),
                self.chips(
                    "sb",
                    [
                        mode("off", "set-sponsorblock-off", SponsorBlockMode::Off),
                        mode("mark", "set-sponsorblock-mark", SponsorBlockMode::Mark),
                        mode(
                            "remove",
                            "set-sponsorblock-remove",
                            SponsorBlockMode::Remove,
                        ),
                    ],
                ),
                cx,
            ),
        ];
        if sb != SponsorBlockMode::Off {
            rows.push(
                self.row(
                    t!("set-sb-categories"),
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .children(SPONSOR_CATEGORIES.map(|(id, key)| {
                            let on = s.sponsorblock.categories.iter().any(|c| c == id);
                            Chip::new(
                                SharedString::from(format!("sbc-{id}")),
                                i18n::lookup(key, None),
                            )
                            .selected(on)
                            .on_click(self.edit(move |st| {
                                let list = &mut st.sponsorblock.categories;
                                match list.iter().position(|c| c == id) {
                                    Some(at) => {
                                        list.remove(at);
                                    }
                                    None => list.push(id.to_owned()),
                                }
                            }))
                        })),
                    cx,
                ),
            );
        }
        rows
    }

    fn page_network(&self, s: &Settings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = *cx.theme();
        let mode = s.cookies.mode;
        let has_file = s.cookies.file.is_some();
        let source = self.chips(
            "ck",
            [
                (
                    "none",
                    "set-cookies-none",
                    mode == CookiesMode::None,
                    Rc::new(|st: &mut Settings| st.cookies.mode = CookiesMode::None) as Edit,
                ),
                (
                    "file",
                    "set-cookies-file",
                    mode == CookiesMode::File,
                    Rc::new(move |st: &mut Settings| {
                        if has_file {
                            st.cookies.mode = CookiesMode::File
                        }
                    }) as Edit,
                ),
                (
                    "browser",
                    "set-cookies-browser",
                    mode == CookiesMode::Browser,
                    Rc::new(|st: &mut Settings| {
                        st.cookies.mode = CookiesMode::Browser;
                        st.cookies.browser.get_or_insert_with(|| "firefox".into());
                    }) as Edit,
                ),
            ],
        );
        let mut rows = vec![
            self.row(t!("set-proxy"), self.input(Field::Proxy, 360., cx), cx),
            self.row(t!("set-rate-limit"), self.input(Field::Rate, 160., cx), cx),
            self.row(t!("set-retries"), self.stepper(Field::Retries, cx), cx),
            self.row(t!("set-cookies"), source, cx),
        ];
        if mode == CookiesMode::Browser {
            let browser = s.cookies.browser.clone().unwrap_or_default();
            rows.push(self.row(
                t!("set-cookies-browser-pick"),
                self.chips(
                    "br",
                    BROWSERS.map(|(id, key)| {
                        (
                            id,
                            key,
                            browser == id,
                            Rc::new(move |st: &mut Settings| st.cookies.browser = Some(id.into()))
                                as Edit,
                        )
                    }),
                ),
                cx,
            ));
            rows.push(
                self.row(
                    SharedString::default(),
                    div()
                        .text_color(theme.muted_foreground)
                        .child(t!("set-cookies-firefox")),
                    cx,
                ),
            );
            rows.push(Notice::new(t!("set-cookies"), t!("set-cookies-chrome")).into_any_element());
        } else {
            let file = div()
                .flex()
                .gap_2()
                .child(
                    Button::new("import-cookies")
                        .outline()
                        .label(t!("set-cookies-import"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.choose_file(
                                |this, path, cx| match std::fs::read_to_string(&path) {
                                    Ok(text) => this.import_cookies(&text, cx),
                                    Err(_) => {
                                        this.cookie_note = Some((t!("set-cookies-bad"), true));
                                        cx.notify();
                                    }
                                },
                                cx,
                            )
                        })),
                )
                .child(
                    Button::new("paste-cookies")
                        .outline()
                        .label(t!("set-cookies-clipboard"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            let text = cx.read_from_clipboard().and_then(|c| c.text());
                            this.import_cookies(&text.unwrap_or_default(), cx);
                        })),
                )
                .child(
                    Button::new("clear-cookies")
                        .ghost()
                        .disabled(!has_file)
                        .label(t!("set-cookies-clear"))
                        .on_click(cx.listener(|this, _, _, cx| this.clear_cookies(cx))),
                );
            rows.push(self.row(SharedString::default(), file, cx));
            if let Some((note, failed)) = self.cookie_note.clone() {
                rows.push(
                    self.row(
                        SharedString::default(),
                        div()
                            .text_color(if failed {
                                theme.danger
                            } else {
                                theme.muted_foreground
                            })
                            .child(note),
                        cx,
                    ),
                );
            }
        }
        rows
    }

    fn page_tools(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let statuses = self.state.tools.read(cx).all().to_vec();
        let mut rows: Vec<AnyElement> = ToolKind::ALL
            .iter()
            .filter_map(|kind| statuses.iter().find(|s| s.kind == *kind))
            .map(|tool| self.tool_row(tool, cx))
            .collect();
        rows.push(Notice::new(t!("set-page-tools"), t!("set-long-paths")).into_any_element());
        rows
    }

    fn tool_row(&self, tool: &ToolStatus, cx: &mut Context<Self>) -> AnyElement {
        let theme = *cx.theme();
        let kind = tool.kind;
        let path = tool.path.clone().unwrap_or_default();
        let note: SharedString = match (&tool.state, tool.source) {
            (ToolState::Missing, _) => t!("welcome-missing"),
            (ToolState::Checking, _) => t!("welcome-waiting"),
            (ToolState::Failed(error), _) => SharedString::from(error.message.clone()),
            (_, Some(ToolSource::Path)) => t!("set-tool-found", path = path),
            (_, Some(ToolSource::Custom)) => t!("set-tool-custom", path = path),
            _ if kind == ToolKind::Python => t!("set-tool-bundled"),
            _ => t!("set-tool-managed"),
        };
        let failed = matches!(tool.state, ToolState::Failed(_));
        let busy = matches!(tool.state, ToolState::Downloading { .. });
        let tools = self.state.tools.clone();
        let button = |id: &'static str, label: SharedString| {
            Button::new(SharedString::from(format!("{id}-{kind}")))
                .outline()
                .small()
                .disabled(busy)
                .label(label)
        };
        let mut actions = Vec::new();
        match kind {
            ToolKind::YtDlp => actions.push(button("tool-check", t!("set-tool-check")).on_click({
                let tools = tools;
                // yt-dlp and its challenge solver update as a pair
                move |_, _, cx| {
                    tools.update(cx, |t, cx| {
                        t.ensure(ToolKind::YtDlp, cx);
                        t.ensure(ToolKind::YtDlpEjs, cx);
                    })
                }
            })),
            ToolKind::Ffmpeg | ToolKind::Ffprobe => {
                actions.push(button("tool-redo", t!("set-tool-redownload")).on_click({
                    let tools = tools;
                    move |_, _, cx| tools.update(cx, |t, cx| t.ensure(kind, cx))
                }))
            }
            _ => {}
        }
        if matches!(kind, ToolKind::Ffmpeg | ToolKind::Ffprobe | ToolKind::Deno) {
            actions.push(
                button("tool-choose", t!("set-tool-choose")).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.choose_file(
                            move |this, path, cx| {
                                let path = path.to_string_lossy().into_owned();
                                this.state
                                    .tools
                                    .update(cx, |t, cx| t.use_custom(kind, path, cx));
                            },
                            cx,
                        )
                    },
                )),
            );
        }
        let progress = match &tool.state {
            ToolState::Downloading { done, total } => Some((
                Rule::Fill(total.map_or(0., |t| *done as f32 / t.max(1) as f32)),
                total.map(|t| {
                    t!(
                        "welcome-progress",
                        done = format::bytes(*done as i64),
                        total = format::bytes(t as i64)
                    )
                }),
            )),
            _ => None,
        };
        let status = match &progress {
            Some((_, Some(text))) => text.clone(),
            Some(_) => t!("welcome-downloading"),
            None => note,
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .min_h(px(32.))
                    .child(div().w(px(110.)).child(i18n::lookup(tool_key(kind), None)))
                    .child(
                        div()
                            .w(px(100.))
                            .font_features(tabular())
                            .text_color(theme.muted_foreground)
                            .child(tool.version.clone().unwrap_or_default()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(if failed {
                                theme.danger
                            } else {
                                theme.muted_foreground
                            })
                            .child(status),
                    )
                    .children(actions),
            )
            .when_some(progress, |this, (rule, _)| {
                this.child(ProgressRule::new(rule))
            })
            .into_any_element()
    }

    fn page_appearance(&self, s: &Settings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let choice = |id: &'static str, key| {
            (
                id,
                key,
                s.theme == id,
                Rc::new(move |st: &mut Settings| st.theme = id.to_owned()) as Edit,
            )
        };
        vec![
            self.row(
                t!("set-theme"),
                self.chips(
                    "theme",
                    [
                        choice("system", "set-theme-system"),
                        choice("dark", "set-theme-dark"),
                        choice("light", "set-theme-light"),
                    ],
                ),
                cx,
            ),
            self.switch("a-compact", "set-compact", s.compact_mode, |s, v| {
                s.compact_mode = v;
                s.show_thumbnails = !v;
            }),
            self.row(t!("set-font-size"), self.stepper(Field::FontSize, cx), cx),
        ]
    }

    fn page_about(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = *cx.theme();
        let (data, logs) = (self.state.paths.data.clone(), self.state.paths.logs());
        let open = |id: &'static str, label: SharedString, dir: PathBuf| {
            Button::new(id)
                .outline()
                .label(label)
                .on_click(move |_, _, cx| {
                    std::fs::create_dir_all(&dir).ok();
                    cx.open_with_system(&dir)
                })
        };
        let muted = |text: SharedString| div().text_color(theme.muted_foreground).child(text);
        vec![
            self.row(
                t!("set-about-version"),
                div()
                    .font_features(tabular())
                    .child(env!("CARGO_PKG_VERSION")),
                cx,
            ),
            self.row(SharedString::default(), muted(t!("set-about-license")), cx),
            self.row(
                SharedString::default(),
                muted(t!("set-about-third-party")),
                cx,
            ),
            self.row(
                SharedString::default(),
                div()
                    .flex()
                    .gap_2()
                    .child(open("open-data", t!("set-about-open-data"), data))
                    .child(open("open-logs", t!("set-about-open-logs"), logs)),
                cx,
            ),
        ]
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let page = match self.navigation.read(cx).current() {
            Destination::Settings(page) => page,
            _ => SettingsPage::General,
        };
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
        );
        let body = match page {
            SettingsPage::General => self.page_general(&s, cx),
            SettingsPage::Downloads => self.page_downloads(&s, cx),
            SettingsPage::Formats => self.page_formats(&s, cx),
            SettingsPage::Network => self.page_network(&s, cx),
            SettingsPage::Tools => self.page_tools(cx),
            SettingsPage::Appearance => self.page_appearance(&s, cx),
            SettingsPage::About => self.page_about(cx),
        };
        div().flex().size_full().child(nav).child(
            div()
                .id("settings-body")
                .flex()
                .flex_col()
                .flex_1()
                .gap_1()
                .p(theme.metrics.pad)
                .overflow_y_scroll()
                .child(
                    div()
                        .mb_2()
                        .text_size(theme.text(Text::Title))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(i18n::lookup(page.key(), None)),
                )
                .when(broken, |this| {
                    this.child(
                        Notice::new(t!("set-file-error-title"), t!("set-file-error")).failed(),
                    )
                })
                .children(body),
        )
    }
}
