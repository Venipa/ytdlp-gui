//! The Site rules page: rules matched by domain (with subdomains), URL prefix or regex decide what
//! the clipboard watch does, and the preset, folder, file name and fix of a download. The first
//! switched-on match wins; "Everything else" holds the defaults. A rule opens on its own page,
//! grouped by when its settings act: which links, when a link is copied, when it downloads.

use std::str::FromStr as _;
use std::time::Duration;

use domain::{ClipAction, Fix, MatchKind, Preset, Settings, SiteRule, check_pattern, rule_for};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, SharedString, Task, Window, div, px};
use i18n::t;
use ui::{ActiveTheme as _, Button, Chip, Input, MONO, Stepper, Switch, Text};

use super::SettingsView;
use super::fields::percent;
use super::layout::{section, stacked};
use super::pages::HEIGHTS;
use crate::format;

/// The rule being edited; `at` is its place in the list, `None` for a new rule, and `fallback`
/// marks the "Everything else" row. `example` is the file name an example link would get.
pub(super) struct RuleEditor {
    at: Option<usize>,
    fallback: bool,
    draft: SiteRule,
    pattern: Entity<Input>,
    test: Entity<Input>,
    outtmpl: Entity<Input>,
    /// The Smaller file quality field, and the text it last showed from here rather than typed.
    quality: Entity<Input>,
    quality_shown: String,
    quality_bad: bool,
    example: Option<Result<String, String>>,
    example_wait: Option<Task<()>>,
    example_task: Option<Task<()>>,
}

fn kind_name(kind: MatchKind) -> SharedString {
    match kind {
        MatchKind::Domain => t!("rule-kind-domain"),
        MatchKind::Prefix => t!("rule-kind-prefix"),
        MatchKind::Regex => t!("rule-kind-regex"),
    }
}

/// The Fluent key of the pattern field's example for `kind`.
fn pattern_hint(kind: MatchKind) -> &'static str {
    match kind {
        MatchKind::Domain => "rule-pattern-hint-domain",
        MatchKind::Prefix => "rule-pattern-hint-prefix",
        MatchKind::Regex => "rule-pattern-hint-regex",
    }
}

fn clip_name(action: ClipAction) -> SharedString {
    match action {
        ClipAction::Add => t!("rule-clip-add"),
        ClipAction::Ask => t!("rule-clip-ask"),
        ClipAction::Ignore => t!("rule-clip-ignore"),
    }
}

/// What the clipboard watch does with links no rule claims.
fn fallback_clip(s: &Settings) -> ClipAction {
    match s.clipboard_auto_add {
        true => ClipAction::Add,
        false => ClipAction::Ask,
    }
}

/// The preset downloads get when no rule picks one: the remembered one.
fn default_preset(s: &Settings) -> Preset {
    Preset::from_str(&s.last_preset).unwrap_or(Preset::DEFAULT)
}

/// "Video 1080p" / "Audio mp3": a preset named so video and audio "best" differ.
fn preset_summary(preset: Preset) -> String {
    let label = format::preset_label(preset);
    match preset {
        Preset::Video(_) => t!("rule-sum-video", q = label).to_string(),
        Preset::Audio(_) => t!("rule-sum-audio", a = label).to_string(),
        Preset::Custom => label,
    }
}

/// Only what `rule` changes, in words: "Saves to D:/Streams · Own file name · Fix: Edit-friendly".
fn summary(rule: &SiteRule) -> SharedString {
    let mut parts: Vec<String> = vec![];
    if let Some(action) = rule.clipboard {
        parts.push(clip_summary(action));
    }
    if let Some(preset) = rule.preset {
        parts.push(preset_summary(preset));
    }
    if let Some(dir) = &rule.output_dir {
        parts.push(t!("rule-sum-folder", dir = dir.clone()).to_string());
    }
    if rule.outtmpl.is_some() {
        parts.push(t!("rule-sum-name").to_string());
    }
    if !rule.fix.is_empty() {
        parts.push(t!("rule-sum-fix", fix = format::chain_name(rule.fix)).to_string());
    }
    match parts.is_empty() {
        true => t!("rule-sum-nothing"),
        false => parts.join(" · ").into(),
    }
}

/// The real defaults, spelled out for the "Everything else" row.
fn fallback_summary(s: &Settings) -> SharedString {
    let fix = match s.default_fix.is_empty() {
        false => t!("rule-sum-fix", fix = format::chain_name(s.default_fix)).to_string(),
        true => t!("rule-sum-no-fix").to_string(),
    };
    [
        clip_summary(fallback_clip(s)),
        preset_summary(default_preset(s)),
        s.output_dir.clone(),
        fix,
    ]
    .join(" · ")
    .into()
}

fn clip_summary(action: ClipAction) -> String {
    match action {
        ClipAction::Add => t!("rule-sum-add"),
        ClipAction::Ask => t!("rule-sum-ask"),
        ClipAction::Ignore => t!("rule-sum-ignore"),
    }
    .to_string()
}

impl SettingsView {
    fn edit_rules(&self, edit: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        self.settings().update(cx, |s, cx| s.update(edit, cx));
    }

    fn open_rule(
        &mut self,
        at: Option<usize>,
        fallback: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let s = self.settings().read(cx).get().clone();
        let draft = match (at, fallback) {
            (_, true) => SiteRule {
                on: true,
                clipboard: Some(fallback_clip(&s)),
                fix: s.default_fix,
                ..Default::default()
            },
            (Some(at), _) => s.site_rules.get(at).cloned().unwrap_or_default(),
            (None, _) => SiteRule {
                on: true,
                ..Default::default()
            },
        };
        let pattern = cx.new(|cx| {
            let mut input = Input::new(pattern_hint(draft.kind), cx);
            input.set_text(draft.pattern.clone(), cx);
            input
        });
        let test = cx.new(|cx| Input::new("rule-test-hint", cx).clearable());
        let outtmpl = cx.new(|cx| {
            let mut input = Input::new("rule-file-name-hint", cx)
                .name("rule-file-name")
                .clearable();
            input.set_text(draft.outtmpl.clone().unwrap_or_default(), cx);
            input
        });
        let quality_shown = draft
            .smaller_quality
            .unwrap_or(s.smaller_quality)
            .to_string();
        let quality = cx.new(|cx| {
            let mut input = Input::new("", cx)
                .joined()
                .align_center()
                .suffix("set-unit-percent");
            input.set_text(quality_shown.clone(), cx);
            input
        });
        cx.observe(&quality, |this, input, cx| {
            let text = input.read(cx).text().trim().to_owned();
            let Some(editor) = &mut this.rule_editor else {
                return;
            };
            // text put there by a chip or a step is not the user's pick
            if text == editor.quality_shown {
                return;
            }
            editor.quality_shown.clone_from(&text);
            let value = percent(&text);
            editor.quality_bad = value.is_err();
            if let Ok(value) = value {
                editor.draft.smaller_quality = Some(value);
            }
            input.update(cx, |input, cx| input.set_invalid(value.is_err(), cx));
            cx.notify();
        })
        .detach();
        cx.observe(&pattern, |this, input, cx| {
            let text = input.read(cx).text().to_owned();
            if let Some(editor) = &mut this.rule_editor {
                editor.draft.pattern = text;
            }
            cx.notify();
        })
        .detach();
        cx.observe(&test, |_, _, cx| cx.notify()).detach();
        cx.observe(&outtmpl, |this, input, cx| {
            let text = input.read(cx).text().trim().to_owned();
            this.draft(|d| d.outtmpl = (!text.is_empty()).then_some(text), cx);
            this.refresh_example(cx);
        })
        .detach();
        if !fallback {
            let focus = pattern.clone();
            focus.update(cx, |input, cx| input.focus(window, cx));
        }
        self.rule_editor = Some(RuleEditor {
            at,
            fallback,
            draft,
            pattern,
            test,
            outtmpl,
            quality,
            quality_shown,
            quality_bad: false,
            example: None,
            example_wait: None,
            example_task: None,
        });
        self.refresh_example(cx);
        cx.notify();
    }

    fn draft(&mut self, edit: impl FnOnce(&mut SiteRule), cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.rule_editor {
            edit(&mut editor.draft);
        }
        cx.notify();
    }

    /// Puts `value` in the quality field without making it the draft's own pick.
    fn show_rule_quality(&mut self, value: u8, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.rule_editor else {
            return;
        };
        editor.quality_shown = value.to_string();
        editor.quality_bad = false;
        let text = editor.quality_shown.clone();
        editor.quality.update(cx, |input, cx| {
            input.set_text(text, cx);
            input.set_invalid(false, cx);
        });
    }

    /// The Default chip of Smaller file quality: the setting decides again.
    fn reset_rule_quality(&mut self, cx: &mut Context<Self>) {
        let value = self.settings().read(cx).get().smaller_quality;
        self.draft(|d| d.smaller_quality = None, cx);
        self.show_rule_quality(value, cx);
    }

    /// The minus and plus of Smaller file quality: 5% a click, kept inside 50 to 100.
    fn step_rule_quality(&mut self, delta: i32, cx: &mut Context<Self>) {
        let default = self.settings().read(cx).get().smaller_quality;
        let now = self
            .rule_editor
            .as_ref()
            .map_or(default, |e| e.draft.smaller_quality.unwrap_or(default));
        let next =
            (i32::from(now) + delta * 5).clamp(i32::from(domain::fixes::MIN_QUALITY), 100) as u8;
        self.draft(|d| d.smaller_quality = Some(next), cx);
        self.show_rule_quality(next, cx);
    }

    /// Previews the drafted file name once typing pauses; the newest request wins.
    fn refresh_example(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.rule_editor else {
            return;
        };
        editor.example_wait = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            this.update(cx, |this, cx| {
                let Some(draft) = this.rule_editor.as_ref().map(|e| e.draft.clone()) else {
                    return;
                };
                let task = this.file_name_preview(
                    draft.outtmpl,
                    draft.output_dir,
                    cx,
                    |this, result, cx| {
                        if let Some(editor) = &mut this.rule_editor {
                            editor.example = Some(result);
                        }
                        cx.notify();
                    },
                );
                if let Some(editor) = &mut this.rule_editor {
                    editor.example_task = task;
                }
            })
            .ok();
        }));
    }

    /// Asks for the folder the drafted rule saves to.
    fn choose_rule_folder(&self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await
                && let Some(path) = paths.first()
            {
                let dir = path.to_string_lossy().into_owned();
                this.update(cx, |this, cx| {
                    this.draft(|d| d.output_dir = Some(dir), cx);
                    this.refresh_example(cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn save_rule(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.rule_editor.take() else {
            return;
        };
        let RuleEditor {
            at,
            fallback,
            mut draft,
            ..
        } = editor;
        if fallback {
            self.edit_rules(
                |s| {
                    s.clipboard_auto_add = draft.clipboard == Some(ClipAction::Add);
                    s.default_fix = draft.fix;
                },
                cx,
            );
        } else {
            draft.pattern = draft.pattern.trim().to_owned();
            if draft.id.is_empty() {
                draft.id = format!("{:x}", jiff::Timestamp::now().as_millisecond());
            }
            self.edit_rules(
                |s| match at {
                    Some(at) if at < s.site_rules.len() => s.site_rules[at] = draft,
                    _ => s.site_rules.push(draft),
                },
                cx,
            );
        }
        cx.notify();
    }

    fn delete_rule(&mut self, cx: &mut Context<Self>) {
        if let Some(RuleEditor { at: Some(at), .. }) = self.rule_editor.take() {
            self.edit_rules(
                |s| {
                    if at < s.site_rules.len() {
                        s.site_rules.remove(at);
                    }
                },
                cx,
            );
        }
        cx.notify();
    }

    fn close_rule(&mut self, cx: &mut Context<Self>) {
        self.rule_editor = None;
        cx.notify();
    }

    pub(super) fn page_rules(
        &mut self,
        s: &Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if self.rule_editor.is_some() {
            return self.rule_editor_page(s, window, cx);
        }
        let theme = *cx.theme();
        let rules = &s.site_rules;
        let last = rules.len().saturating_sub(1);
        let number = |n: Option<usize>| {
            div()
                .w(px(18.))
                .flex_none()
                .font_family(MONO)
                .text_size(theme.text(Text::Small))
                .text_color(theme.muted_foreground)
                .children(n.map(|n| SharedString::from((n + 1).to_string())))
        };
        let line = |text: SharedString| {
            div()
                .truncate()
                .text_size(theme.text(Text::Small))
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let mut rows: Vec<AnyElement> = vec![];
        for (at, rule) in rules.iter().enumerate() {
            let id = rule.id.clone();
            let bad = check_pattern(rule.kind, &rule.pattern).is_err();
            let title = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_w_0()
                .child(kind_tag(rule.kind, cx))
                .child(
                    div()
                        .font_family(MONO)
                        .truncate()
                        .when(bad, |this| this.text_color(theme.danger))
                        .when(!rule.on && !bad, |this| {
                            this.text_color(theme.muted_foreground)
                        })
                        .child(rule.pattern.clone()),
                );
            let controls = div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    Button::new(SharedString::from(format!("rule-up-{id}")))
                        .ghost()
                        .small()
                        .icon("icons/chevron-up.svg")
                        .name(t!("set-ck-up"))
                        .disabled(at == 0)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_rules(|s| s.site_rules.swap(at, at - 1), cx)
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("rule-down-{id}")))
                        .ghost()
                        .small()
                        .icon("icons/chevron-down.svg")
                        .name(t!("set-ck-down"))
                        .disabled(at == last)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_rules(|s| s.site_rules.swap(at, at + 1), cx)
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("rule-edit-{id}")))
                        .ghost()
                        .small()
                        .label(t!("set-ck-edit"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_rule(Some(at), false, window, cx)
                        })),
                )
                .child(
                    Switch::new(SharedString::from(format!("rule-on-{id}")), rule.on)
                        .name(t!("rule-on"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_rules(
                                |s| {
                                    if let Some(rule) = s.site_rules.get_mut(at) {
                                        rule.on = !rule.on;
                                    }
                                },
                                cx,
                            )
                        })),
                );
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .min_h(px(52.))
                    .py(px(10.))
                    .child(number(Some(at)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(title)
                            .child(line(summary(rule))),
                    )
                    .child(controls)
                    .into_any_element(),
            );
        }
        rows.push(
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .min_h(px(52.))
                .py(px(10.))
                .child(number(None))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(t!("rule-fallback"))
                        .child(line(fallback_summary(s))),
                )
                .child(
                    Button::new("rule-edit-fallback")
                        .ghost()
                        .small()
                        .label(t!("set-ck-edit"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_rule(None, true, window, cx)
                        })),
                )
                .into_any_element(),
        );
        vec![
            section(Some(t!("rule-list")), rows, cx),
            div()
                .flex()
                .justify_end()
                .child(
                    Button::new("rule-add")
                        .outline()
                        .icon("icons/plus.svg")
                        .label(t!("rule-add"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_rule(None, false, window, cx)
                        })),
                )
                .into_any_element(),
        ]
    }

    /// The open rule: a way back, the rule's header, then its settings in three groups.
    fn rule_editor_page(
        &self,
        s: &Settings,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = *cx.theme();
        let editor = self.rule_editor.as_ref().expect("an open editor");
        let draft = editor.draft.clone();
        let fallback = editor.fallback;
        let check = check_pattern(draft.kind, &draft.pattern);
        let chip = |id: String, label: SharedString, selected: bool| {
            Chip::new(SharedString::from(id), label).selected(selected)
        };
        let small = |text: SharedString| {
            div()
                .text_size(theme.text(Text::Small))
                .text_color(theme.muted_foreground)
                .child(text)
        };
        let chips = || div().flex().flex_wrap().gap(px(8.));
        let mut page: Vec<AnyElement> = vec![];

        // 1A: "‹ Site rules / Rule 2 of 4"
        let where_ = match (fallback, editor.at) {
            (true, _) => t!("rule-fallback"),
            (false, Some(at)) => t!("rule-n-of", n = at + 1, total = s.site_rules.len()),
            (false, None) => t!("rule-new-title"),
        };
        page.push(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_color(theme.muted_foreground)
                .child(
                    Button::new("rule-back")
                        .ghost()
                        .small()
                        .icon("icons/chevron-left.svg")
                        .label(t!("set-page-rules"))
                        .on_click(cx.listener(|this, _, _, cx| this.close_rule(cx))),
                )
                .child("/")
                .child(div().text_color(theme.foreground).child(where_))
                .into_any_element(),
        );
        if !fallback {
            let shown = match draft.pattern.trim().is_empty() {
                true => t!("rule-new-title"),
                false => SharedString::from(draft.pattern.clone()),
            };
            page.push(section(
                None,
                vec![
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .min_w_0()
                        .min_h(px(52.))
                        .py(px(10.))
                        .child(kind_tag(draft.kind, cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_family(MONO)
                                .truncate()
                                .child(shown),
                        )
                        .child(small(t!("rule-on")))
                        .child(
                            Switch::new("rule-editor-on", draft.on)
                                .name(t!("rule-on"))
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.draft(|d| d.on = !d.on, cx)),
                                ),
                        )
                        .into_any_element(),
                ],
                cx,
            ));

            // Links: Match with the link test right under it
            let kinds = chips().children(
                [MatchKind::Domain, MatchKind::Prefix, MatchKind::Regex].map(|kind| {
                    chip(
                        format!("rule-kind-{kind:?}"),
                        kind_name(kind),
                        draft.kind == kind,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(editor) = &this.rule_editor {
                            let pattern = editor.pattern.clone();
                            pattern.update(cx, |input, cx| input.set_hint(pattern_hint(kind), cx));
                        }
                        this.draft(|d| d.kind = kind, cx)
                    }))
                }),
            );
            let mut rules = s.site_rules.clone();
            match editor.at {
                // the draft stands in for its own place in the list, so the test shows what saving does
                Some(at) if at < rules.len() => rules[at] = draft.clone(),
                _ => rules.push(draft.clone()),
            }
            let link = editor.test.read(cx).text().trim().to_owned();
            let result = (!link.is_empty()).then(|| match rule_for(&rules, &link) {
                Some((at, rule)) => t!(
                    "rule-test-match",
                    n = at + 1,
                    pattern = rule.pattern.clone(),
                    what = summary(rule).to_string()
                ),
                None => t!("rule-test-none"),
            });
            page.push(section(
                Some(t!("rule-grp-links")),
                vec![
                    stacked(
                        t!("rule-match"),
                        Some(t!("rule-match-desc")),
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(kinds)
                            .child(div().font_family(MONO).child(editor.pattern.clone()))
                            .children(
                                check
                                    .clone()
                                    .err()
                                    .filter(|_| !draft.pattern.trim().is_empty())
                                    .map(|why| {
                                        div()
                                            .text_size(theme.text(Text::Small))
                                            .text_color(theme.danger)
                                            .child(t!("rule-pattern-bad", why = why))
                                    }),
                            ),
                        cx,
                    ),
                    stacked(
                        t!("rule-test"),
                        Some(t!("rule-test-desc")),
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(div().font_family(MONO).child(editor.test.clone()))
                            .children(result.map(small)),
                        cx,
                    ),
                ],
                cx,
            ));
        }

        // When a link is copied
        let clips: Vec<Option<ClipAction>> = match fallback {
            true => vec![Some(ClipAction::Add), Some(ClipAction::Ask)],
            false => vec![
                None,
                Some(ClipAction::Add),
                Some(ClipAction::Ask),
                Some(ClipAction::Ignore),
            ],
        };
        let default_clip = clip_name(fallback_clip(s));
        page.push(section(
            Some(t!("rule-grp-copied")),
            vec![stacked(
                t!("rule-clipboard"),
                Some(t!("rule-clipboard-desc")),
                chips().children(clips.into_iter().map(|action| {
                    let label = match action {
                        Some(action) => clip_name(action),
                        None => t!("rule-default-is", value = default_clip.to_string()),
                    };
                    chip(
                        format!("rule-clip-{action:?}"),
                        label,
                        draft.clipboard == action,
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.draft(|d| d.clipboard = action, cx)),
                    )
                })),
                cx,
            )],
            cx,
        ));

        // When it downloads
        let mut downloads: Vec<AnyElement> = vec![];
        if !fallback {
            let preset_chip = |preset: Option<Preset>, label: SharedString| {
                chip(
                    format!("rule-preset-{preset:?}"),
                    label,
                    draft.preset == preset,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.draft(|d| d.preset = preset, cx)))
            };
            let caption = |key: &str| {
                div()
                    .w(px(44.))
                    .flex_none()
                    .h(theme.metrics.control)
                    .flex()
                    .items_center()
                    .text_size(theme.text(Text::Small))
                    .text_color(theme.muted_foreground)
                    .child(i18n::lookup(key, None))
            };
            let (video, audio): (Vec<Preset>, Vec<Preset>) = Preset::all()
                .into_iter()
                .partition(|p| matches!(p, Preset::Video(_)));
            let default_label = t!(
                "rule-default-is",
                value = format::preset_label(default_preset(s))
            );
            downloads.push(stacked(
                t!("rule-format"),
                Some(t!("rule-format-desc")),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .child(caption("rule-fmt-video"))
                            .child(
                                chips()
                                    .flex_1()
                                    .child(preset_chip(None, default_label))
                                    .children(video.into_iter().map(|p| {
                                        preset_chip(Some(p), format::preset_label(p).into())
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .child(caption("rule-fmt-audio"))
                            .child(
                                chips().flex_1().children(
                                    audio.into_iter().map(|p| {
                                        preset_chip(Some(p), format::preset_label(p).into())
                                    }),
                                ),
                            ),
                    ),
                cx,
            ));
            let folder = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(self.value_box(
                    match &draft.output_dir {
                        Some(dir) => SharedString::from(dir.clone()),
                        None => t!("rule-folder-default", dir = s.output_dir.clone()),
                    },
                    cx,
                ))
                .child(
                    Button::new("rule-folder-choose")
                        .outline()
                        .label(t!("set-choose"))
                        .on_click(cx.listener(|this, _, _, cx| this.choose_rule_folder(cx))),
                )
                .when(draft.output_dir.is_some(), |this| {
                    this.child(
                        Button::new("rule-folder-reset")
                            .ghost()
                            .label(t!("rule-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.draft(|d| d.output_dir = None, cx);
                                this.refresh_example(cx);
                            })),
                    )
                });
            downloads.push(stacked(
                t!("rule-folder"),
                Some(t!("rule-folder-desc")),
                folder,
                cx,
            ));
            let example = match &editor.example {
                Some(Ok(name)) => Some(t!("rule-file-name-example", name = name.clone())),
                Some(Err(_)) => Some(t!("set-preview-failed")),
                None => None,
            };
            downloads.push(stacked(
                t!("rule-file-name"),
                Some(t!("rule-file-name-desc", template = s.outtmpl.clone())),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().font_family(MONO).child(editor.outtmpl.clone()))
                    .children(example.map(|line| small(line).font_family(MONO))),
                cx,
            ));
        }
        let fix_desc = match draft.fix.is_empty() {
            true => t!("rule-fix-desc"),
            false => t!("rule-fix-runs", fixes = format::chain_name(draft.fix)),
        };
        // one chip row per kind: Keep, then that kind's fixes (the video ones replace each other)
        let fix_row = |kind: &'static str, label: SharedString, picks: Vec<(Option<Fix>, bool)>| {
            let keep = picks.iter().all(|(_, on)| !on);
            let all = std::iter::once((None, keep)).chain(picks);
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(div().w(px(96.)).child(small(label)))
                .child(chips().children(all.map(|(fix, on)| {
                    let label =
                        fix.map_or_else(|| t!("rule-fix-keep"), |f| format::fix_name(f).into());
                    let id = format!(
                        "rule-fix-{kind}-{}",
                        fix.map_or_else(|| "keep".into(), |f| format!("{f:?}"))
                    );
                    chip(id, label, on).on_click(cx.listener(move |this, _, _, cx| {
                        this.draft(
                            |d| match (kind, fix) {
                                ("video", _) => d.fix.set_video(fix),
                                ("audio", f) => d.fix.normalize = f.is_some(),
                                (_, f) => d.fix.repair = f.is_some(),
                            },
                            cx,
                        )
                    }))
                })))
        };
        let video = [Fix::Compatible, Fix::EditFriendly, Fix::Smaller]
            .map(|f| (Some(f), draft.fix.video == Some(f)));
        downloads.push(stacked(
            t!("rule-fix"),
            Some(fix_desc),
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(fix_row("video", t!("rule-fix-video"), video.into()))
                .child(fix_row(
                    "audio",
                    t!("rule-fix-audio"),
                    vec![(Some(Fix::Normalize), draft.fix.normalize)],
                ))
                .child(fix_row(
                    "timestamps",
                    t!("rule-fix-timestamps"),
                    vec![(Some(Fix::Repair), draft.fix.repair)],
                )),
            cx,
        ));
        if !fallback && draft.fix.video == Some(Fix::Smaller) {
            let this = cx.entity();
            let keep = t!("rule-smaller-keep-lower");
            let default_size = match HEIGHTS.iter().find(|(h, _)| *h == s.smaller_height) {
                Some((h, key)) if *h > 0 => i18n::lookup(key, None),
                _ => keep,
            };
            downloads.push(stacked(
                t!("rule-smaller-quality"),
                Some(t!("rule-smaller-site-desc")),
                chips()
                    .items_center()
                    .child(
                        chip(
                            "rule-smq-default".into(),
                            t!("rule-default-is", value = format!("{}%", s.smaller_quality)),
                            draft.smaller_quality.is_none(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.reset_rule_quality(cx))),
                    )
                    .child(
                        Stepper::new("rule-smq-step", &editor.quality, px(64.))
                            .invalid(editor.quality_bad)
                            .on_step(move |delta, _, cx| {
                                this.update(cx, |this, cx| this.step_rule_quality(delta, cx))
                            }),
                    ),
                cx,
            ));
            downloads.push(stacked(
                t!("rule-smaller-height"),
                Some(t!("rule-smaller-site-desc")),
                chips()
                    .child(
                        chip(
                            "rule-smh-default".into(),
                            t!("rule-default-is", value = default_size.to_string()),
                            draft.smaller_height.is_none(),
                        )
                        .on_click(
                            cx.listener(|this, _, _, cx| {
                                this.draft(|d| d.smaller_height = None, cx)
                            }),
                        ),
                    )
                    .children(HEIGHTS.map(|(height, key)| {
                        chip(
                            format!("rule-smh-{height}"),
                            i18n::lookup(key, None),
                            draft.smaller_height == Some(height),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.draft(|d| d.smaller_height = Some(height), cx)
                        }))
                    })),
                cx,
            ));
        }
        page.push(section(Some(t!("rule-grp-download")), downloads, cx));

        let can_save = fallback || check.is_ok();
        page.push(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1())
                .when(editor.at.is_some() && !fallback, |this| {
                    this.child(
                        Button::new("rule-delete")
                            .ghost()
                            .destructive()
                            .label(t!("rule-delete"))
                            .on_click(cx.listener(|this, _, _, cx| this.delete_rule(cx))),
                    )
                })
                .child(
                    Button::new("rule-cancel")
                        .ghost()
                        .label(t!("set-ck-close"))
                        .on_click(cx.listener(|this, _, _, cx| this.close_rule(cx))),
                )
                .child(
                    Button::new("rule-save")
                        .primary()
                        .label(t!("set-ck-save"))
                        .disabled(!can_save)
                        .on_click(cx.listener(|this, _, _, cx| this.save_rule(cx))),
                )
                .into_any_element(),
        );
        page
    }
}

/// The ink-blue tag naming how a rule matches.
fn kind_tag(kind: MatchKind, cx: &gpui::App) -> gpui::Div {
    let theme = cx.theme();
    div()
        .flex_none()
        .px(px(6.))
        .rounded(theme.radius)
        .bg(theme.table_active)
        .text_size(theme.text(Text::Small))
        .text_color(crate::ink(cx))
        .child(kind_name(kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_names_only_what_a_rule_changes() {
        assert_eq!(summary(&SiteRule::default()), t!("rule-sum-nothing"));
        let rule = SiteRule {
            output_dir: Some("D:/Streams".into()),
            outtmpl: Some("%(id)s.%(ext)s".into()),
            fix: Fix::EditFriendly.into(),
            ..Default::default()
        };
        let text = summary(&rule).to_string();
        assert!(
            text.contains("D:/Streams") && !text.contains("copied"),
            "{text}"
        );
        assert_eq!(text.matches(" · ").count(), 2, "{text}");
    }
}
