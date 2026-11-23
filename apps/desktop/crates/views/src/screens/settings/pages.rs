//! The seven settings pages as sections of rows (see `layout`).

use std::path::PathBuf;
use std::rc::Rc;

use domain::{
    CookiesMode, Preset, Settings, SponsorBlockMode, ToolKind, ToolSource, ToolState, ToolStatus,
};
use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, div, px};
use i18n::t;
use ui::{
    ActiveTheme as _, Button, Chip, MONO, MenuItem, Notice, Picker, ProgressRule, Rule, Text,
    WindowBlur, tabular,
};

use super::fields::Field;
use super::layout::{row, section, stacked};
use super::{BROWSERS, Edit, SPONSOR_CATEGORIES, SettingsView, look, tool_key};
use crate::format;

/// The licence notices, bundled so About can open them without the repository around.
const THIRD_PARTY: &str = include_str!("../../../../../../../THIRD-PARTY.md");

impl SettingsView {
    pub(super) fn page_general(&self, s: &Settings, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
            section(
                Some(t!("set-sec-interface")),
                vec![row(
                    t!("set-language"),
                    Some(t!("set-language-desc")),
                    language,
                    cx,
                )],
                cx,
            ),
            section(
                Some(t!("set-sec-window")),
                vec![
                    self.switch("s-min", "set-start-minimized", s.start_minimized, |s, v| {
                        s.start_minimized = v
                    }),
                    self.switch("s-tray", "set-close-to-tray", s.close_to_tray, |s, v| {
                        s.close_to_tray = v
                    }),
                    self.switch("s-notify", "set-notifications", s.notifications, |s, v| {
                        s.notifications = v
                    }),
                ],
                cx,
            ),
            section(
                Some(t!("set-sec-links")),
                vec![
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
                ],
                cx,
            ),
        ]
    }

    pub(super) fn page_downloads(&self, s: &Settings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = *cx.theme();
        let folder = div()
            .flex()
            .items_center()
            .gap_2()
            .child(self.value_box(s.output_dir.clone(), cx))
            .child(
                Button::new("choose-dir")
                    .outline()
                    .label(t!("set-choose"))
                    .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
            );
        let preview = div()
            .font_family(MONO)
            .text_size(theme.text(Text::Small))
            .text_color(match &self.preview {
                Some(Err(_)) => theme.danger,
                _ => theme.muted_foreground,
            })
            .child(match &self.preview {
                Some(Ok(name)) => SharedString::from(name.clone()),
                Some(Err(_)) => t!("set-preview-failed"),
                None => t!("set-preview-pending"),
            });
        vec![
            section(
                Some(t!("set-sec-location")),
                vec![
                    stacked(t!("set-save-to"), Some(t!("set-save-to-desc")), folder, cx),
                    row(
                        t!("set-file-name"),
                        Some(t!("set-file-name-desc")),
                        self.input(Field::Template, 300., cx),
                        cx,
                    ),
                    stacked(t!("set-preview"), None, preview, cx),
                ],
                cx,
            ),
            section(
                Some(t!("set-sec-queue")),
                vec![
                    row(
                        t!("set-at-once"),
                        Some(t!("set-at-once-desc")),
                        self.stepper(Field::Global, cx),
                        cx,
                    ),
                    row(
                        t!("set-per-site"),
                        Some(t!("set-per-site-desc")),
                        self.stepper(Field::PerDomain, cx),
                        cx,
                    ),
                    row(
                        t!("set-retries"),
                        Some(t!("set-retries-desc")),
                        self.stepper(Field::Retries, cx),
                        cx,
                    ),
                    self.switch(
                        "d-thumbs",
                        "set-show-thumbnails",
                        s.show_thumbnails,
                        |s, v| {
                            s.show_thumbnails = v;
                            s.compact_mode = !v;
                        },
                    ),
                ],
                cx,
            ),
        ]
    }

    pub(super) fn page_formats(&self, s: &Settings, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
        let mut sponsor = vec![row(
            t!("set-sponsorblock"),
            Some(t!("set-sponsorblock-desc")),
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
        )];
        if sb != SponsorBlockMode::Off {
            sponsor.push(stacked(
                t!("set-sb-categories"),
                Some(t!("set-sb-categories-desc")),
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
            ));
        }
        vec![
            section(
                Some(t!("set-sec-defaults")),
                vec![row(
                    t!("set-default-preset"),
                    Some(t!("set-default-preset-desc")),
                    preset,
                    cx,
                )],
                cx,
            ),
            section(
                Some(t!("set-subtitles")),
                vec![
                    self.switch(
                        "f-subs",
                        "set-subtitles-enable",
                        s.subtitles.enabled,
                        |s, v| s.subtitles.enabled = v,
                    ),
                    row(
                        t!("set-subtitles-langs"),
                        Some(t!("set-subtitles-langs-desc")),
                        self.input(Field::Langs, 200., cx),
                        cx,
                    ),
                    self.switch(
                        "f-subs-embed",
                        "set-subtitles-embed",
                        s.subtitles.embed,
                        |s, v| s.subtitles.embed = v,
                    ),
                ],
                cx,
            ),
            section(
                Some(t!("set-sec-embed")),
                vec![
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
                ],
                cx,
            ),
            section(Some(t!("set-sponsorblock")), sponsor, cx),
        ]
    }

    pub(super) fn page_network(&self, s: &Settings, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
        let mut cookies = vec![row(
            t!("set-cookies"),
            Some(t!("set-cookies-desc")),
            source,
            cx,
        )];
        if mode == CookiesMode::Browser {
            let browser = s.cookies.browser.clone().unwrap_or_default();
            cookies.push(row(
                t!("set-cookies-browser-pick"),
                Some(t!("set-cookies-firefox")),
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
            cookies
                .push(Notice::new(t!("set-cookies"), t!("set-cookies-chrome")).into_any_element());
        } else {
            let buttons = div()
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
            cookies.push(stacked(
                t!("set-cookies-file-title"),
                Some(t!("set-cookies-file-desc")),
                div().flex().flex_col().gap_2().child(buttons).children(
                    self.cookie_note.clone().map(|(note, failed)| {
                        div()
                            .text_size(theme.text(Text::Small))
                            .text_color(if failed {
                                theme.danger
                            } else {
                                theme.muted_foreground
                            })
                            .child(note)
                    }),
                ),
                cx,
            ));
        }
        vec![
            section(
                Some(t!("set-sec-connection")),
                vec![
                    row(
                        t!("set-proxy"),
                        Some(t!("set-proxy-desc")),
                        self.input(Field::Proxy, 260., cx),
                        cx,
                    ),
                    row(
                        t!("set-rate-limit"),
                        Some(t!("set-rate-limit-desc")),
                        self.input(Field::Rate, 120., cx),
                        cx,
                    ),
                ],
                cx,
            ),
            section(Some(t!("set-cookies")), cookies, cx),
        ]
    }

    pub(super) fn page_tools(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let statuses = self.state.tools.read(cx).all().to_vec();
        // the four tools people act on always show, even before the first check reports
        let always = [
            ToolKind::YtDlp,
            ToolKind::Ffmpeg,
            ToolKind::Ffprobe,
            ToolKind::Deno,
        ];
        let ytdlp = self.ytdlp_version.clone();
        let rows: Vec<AnyElement> = ToolKind::ALL
            .iter()
            .filter_map(|kind| match statuses.iter().find(|s| s.kind == *kind) {
                Some(s) => Some(s.clone()),
                None => always.contains(kind).then_some(ToolStatus {
                    kind: *kind,
                    state: ToolState::Missing,
                    source: None,
                    path: None,
                    version: None,
                }),
            })
            .map(|mut tool| {
                // the bundled yt-dlp is not a tool we detect: the engine reports its version
                if tool.kind == ToolKind::YtDlp
                    && tool.state == ToolState::Missing
                    && let Some(version) = &ytdlp
                {
                    tool.state = ToolState::Ready;
                    tool.version = Some(version.clone());
                }
                tool
            })
            .map(|tool| self.tool_row(&tool, cx))
            .collect();
        vec![
            section(None, rows, cx),
            Notice::new(t!("set-page-tools"), t!("set-long-paths")).into_any_element(),
        ]
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
            _ if matches!(kind, ToolKind::Python | ToolKind::YtDlp) => t!("set-tool-bundled"),
            _ => t!("set-tool-managed"),
        };
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
                let current = self.ytdlp_version.clone();
                move |_, _, cx| tools.update(cx, |t, cx| t.check_update(kind, current.clone(), cx))
            })),
            ToolKind::Ffmpeg | ToolKind::Ffprobe | ToolKind::Deno => {
                actions.push(button("tool-redo", t!("set-tool-redownload")).on_click({
                    let tools = tools;
                    move |_, _, cx| tools.update(cx, |t, cx| t.reinstall(kind, cx))
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
        let (rule, progress_text) = match &tool.state {
            ToolState::Downloading { done, total } => (
                Some(Rule::Fill(
                    total.map_or(0., |t| *done as f32 / t.max(1) as f32),
                )),
                Some(match total {
                    Some(t) => t!(
                        "welcome-progress",
                        done = format::bytes(*done as i64),
                        total = format::bytes(*t as i64)
                    ),
                    None => t!("welcome-downloading"),
                }),
            ),
            _ => (None, None),
        };
        let version = tool.version.clone().unwrap_or_default();
        let detail = progress_text.unwrap_or_else(|| match version.is_empty() {
            true => note,
            false => SharedString::from(format!("{version}  \u{b7}  {note}")),
        });
        let failed = matches!(tool.state, ToolState::Failed(_));
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .min_h(px(48.))
                    .py_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(i18n::lookup(tool_key(kind), None))
                            .child(
                                div()
                                    .truncate()
                                    .font_features(tabular())
                                    .text_size(theme.text(Text::Small))
                                    .text_color(if failed {
                                        theme.danger
                                    } else {
                                        theme.muted_foreground
                                    })
                                    .child(detail),
                            ),
                    )
                    .child(div().flex().flex_none().gap_2().children(actions)),
            )
            .children(rule.map(|rule| div().pb_2().child(ProgressRule::new(rule))))
            .into_any_element()
    }

    pub(super) fn page_appearance(&self, s: &Settings, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let bg = |id: &'static str, key| {
            (
                id,
                key,
                s.background == id,
                Rc::new(move |st: &mut Settings| st.background = id.to_owned()) as Edit,
            )
        };
        let mut backgrounds = vec![
            bg("solid", "set-bg-solid"),
            bg("translucent", "set-bg-translucent"),
        ];
        if WindowBlur::supported(cx) {
            backgrounds.push(bg("blur", "set-bg-blur"));
        }
        let choice = |id: &'static str, key| {
            (
                id,
                key,
                s.theme == id,
                Rc::new(move |st: &mut Settings| st.theme = id.to_owned()) as Edit,
            )
        };
        vec![
            section(
                Some(t!("set-sec-colours")),
                vec![
                    row(
                        t!("set-theme"),
                        Some(t!("set-theme-desc")),
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
                    row(
                        t!("set-background"),
                        Some(t!("set-background-desc")),
                        self.chips("bg", backgrounds),
                        cx,
                    ),
                ],
                cx,
            ),
            section(
                Some(t!("set-sec-text")),
                vec![row(
                    t!("set-font-size"),
                    Some(t!("set-font-size-desc")),
                    div().flex().gap_2().children(look::FONT_SIZES.map(|px| {
                        Chip::new(
                            SharedString::from(format!("font-{px}")),
                            t!("set-px", value = px as i64),
                        )
                        .selected(s.font_size == px)
                        .on_click(self.edit(move |st| st.font_size = px))
                    })),
                    cx,
                )],
                cx,
            ),
        ]
    }

    pub(super) fn page_about(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
        let notices = {
            let file = self.state.paths.data.join("THIRD-PARTY.md");
            Button::new("open-third-party")
                .outline()
                .label(t!("set-about-open-third-party"))
                .on_click(move |_, _, cx| {
                    std::fs::create_dir_all(file.parent().unwrap_or(&file)).ok();
                    if std::fs::write(&file, THIRD_PARTY).is_ok() {
                        cx.open_with_system(&file)
                    }
                })
        };
        vec![
            section(
                Some(t!("set-sec-app")),
                vec![
                    row(
                        t!("set-about-version"),
                        Some(t!("set-about-license")),
                        div()
                            .font_features(tabular())
                            .child(env!("CARGO_PKG_VERSION")),
                        cx,
                    ),
                    row(
                        t!("set-about-third-party-title"),
                        Some(t!("set-about-third-party")),
                        notices,
                        cx,
                    ),
                ],
                cx,
            ),
            section(
                Some(t!("set-sec-folders")),
                vec![
                    row(
                        t!("set-about-open-data"),
                        Some(t!("set-about-data-desc")),
                        open("open-data", t!("set-open"), data),
                        cx,
                    ),
                    row(
                        t!("set-about-open-logs"),
                        Some(t!("set-about-logs-desc")),
                        open("open-logs", t!("set-open"), logs),
                        cx,
                    ),
                ],
                cx,
            ),
        ]
    }
}
