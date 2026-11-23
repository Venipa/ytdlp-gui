//! The Site rules page: rules matched by domain (with subdomains), URL prefix or regex decide what
//! the clipboard watch does, which preset a download uses and which fix runs after it. The first
//! switched-on match wins; "Everything else" holds the defaults. An editor with a link tester
//! edits one rule at a time.

use domain::{ClipAction, Fix, MatchKind, Preset, Settings, SiteRule, check_pattern, rule_for};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, SharedString, Window, div, px};
use i18n::t;
use ui::{ActiveTheme as _, Button, Chip, Input, MONO, Switch, Text};

use super::SettingsView;
use super::layout::{row, section, stacked};
use crate::format;

/// The rule being edited; `at` is its place in the list, `None` for a new rule, and `fallback`
/// marks the "Everything else" row.
pub(super) struct RuleEditor {
    at: Option<usize>,
    fallback: bool,
    draft: SiteRule,
    pattern: Entity<Input>,
    test: Entity<Input>,
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

fn clip_name(action: Option<ClipAction>) -> SharedString {
    match action {
        None => t!("rule-default"),
        Some(ClipAction::Add) => t!("rule-clip-add"),
        Some(ClipAction::Ask) => t!("rule-clip-ask"),
        Some(ClipAction::Ignore) => t!("rule-clip-ignore"),
    }
}

/// "Clipboard: add · Video 1080p · Fix: Compatible" for a rule row.
fn summary(rule: &SiteRule) -> SharedString {
    let preset = rule
        .preset
        .map(format::preset_label)
        .unwrap_or_else(|| t!("rule-default").to_string());
    let fix = rule
        .fix
        .map(format::fix_name)
        .unwrap_or_else(|| t!("rule-fix-none").to_string());
    t!(
        "rule-summary",
        clipboard = clip_name(rule.clipboard).to_string(),
        preset = preset,
        fix = fix
    )
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
                clipboard: Some(if s.clipboard_auto_add {
                    ClipAction::Add
                } else {
                    ClipAction::Ask
                }),
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
        cx.observe(&pattern, |this, input, cx| {
            let text = input.read(cx).text().to_owned();
            if let Some(editor) = &mut this.rule_editor {
                editor.draft.pattern = text;
            }
            cx.notify();
        })
        .detach();
        cx.observe(&test, |_, _, cx| cx.notify()).detach();
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
        });
        cx.notify();
    }

    fn draft(&mut self, edit: impl FnOnce(&mut SiteRule), cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.rule_editor {
            edit(&mut editor.draft);
        }
        cx.notify();
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

    pub(super) fn page_rules(
        &mut self,
        s: &Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        if self.rule_editor.is_some() {
            return vec![self.rule_editor_section(s, window, cx)];
        }
        let theme = *cx.theme();
        let rules = &s.site_rules;
        let last = rules.len().saturating_sub(1);
        let mut rows: Vec<AnyElement> = vec![];
        for (at, rule) in rules.iter().enumerate() {
            let id = rule.id.clone();
            let bad = check_pattern(rule.kind, &rule.pattern).is_err();
            let title = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .px(px(6.))
                        .rounded(theme.radius)
                        .bg(theme.table_active)
                        .text_size(theme.text(Text::Small))
                        .text_color(crate::ink(cx))
                        .child(kind_name(rule.kind)),
                )
                .child(
                    div()
                        .font_family(MONO)
                        .truncate()
                        .when(bad, |this| this.text_color(theme.danger))
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
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(title)
                            .child(
                                div()
                                    .text_size(theme.text(Text::Small))
                                    .text_color(theme.muted_foreground)
                                    .child(summary(rule)),
                            ),
                    )
                    .child(controls)
                    .into_any_element(),
            );
        }
        let fallback = SiteRule {
            clipboard: Some(if s.clipboard_auto_add {
                ClipAction::Add
            } else {
                ClipAction::Ask
            }),
            fix: s.default_fix,
            ..Default::default()
        };
        rows.push(row(
            t!("rule-fallback"),
            Some(summary(&fallback)),
            Button::new("rule-edit-fallback")
                .ghost()
                .small()
                .label(t!("set-ck-edit"))
                .on_click(
                    cx.listener(|this, _, window, cx| this.open_rule(None, true, window, cx)),
                ),
            cx,
        ));
        let encoder = s.encoder.clone();
        let encoders = self.chips(
            "enc",
            [
                (
                    "auto",
                    "rule-enc-auto",
                    encoder == "auto",
                    std::rc::Rc::new(|st: &mut Settings| st.encoder = "auto".into()) as super::Edit,
                ),
                (
                    "hardware",
                    "rule-enc-hardware",
                    encoder == "hardware",
                    std::rc::Rc::new(|st: &mut Settings| st.encoder = "hardware".into())
                        as super::Edit,
                ),
                (
                    "software",
                    "rule-enc-software",
                    encoder == "software",
                    std::rc::Rc::new(|st: &mut Settings| st.encoder = "software".into())
                        as super::Edit,
                ),
            ],
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
            section(
                Some(t!("rule-converting")),
                vec![stacked(
                    t!("rule-encoder"),
                    Some(t!("rule-encoder-desc")),
                    encoders,
                    cx,
                )],
                cx,
            ),
        ]
    }

    fn rule_editor_section(
        &self,
        s: &Settings,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        let editor = self.rule_editor.as_ref().expect("an open editor");
        let draft = editor.draft.clone();
        let fallback = editor.fallback;
        let check = check_pattern(draft.kind, &draft.pattern);
        let chip = |id: String, label: SharedString, selected: bool| {
            Chip::new(SharedString::from(id), label).selected(selected)
        };
        let mut rows: Vec<AnyElement> = vec![];
        if !fallback {
            let kinds = div().flex().flex_wrap().gap(px(8.)).children(
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
            rows.push(stacked(
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
            ));
        }
        let clips: Vec<Option<ClipAction>> = match fallback {
            true => vec![Some(ClipAction::Add), Some(ClipAction::Ask)],
            false => vec![
                None,
                Some(ClipAction::Add),
                Some(ClipAction::Ask),
                Some(ClipAction::Ignore),
            ],
        };
        rows.push(stacked(
            t!("rule-clipboard"),
            Some(t!("rule-clipboard-desc")),
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .children(clips.into_iter().map(|action| {
                    chip(
                        format!("rule-clip-{action:?}"),
                        clip_name(action),
                        draft.clipboard == action,
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.draft(|d| d.clipboard = action, cx)),
                    )
                })),
            cx,
        ));
        if !fallback {
            let presets = std::iter::once(None).chain(Preset::all().into_iter().map(Some));
            rows.push(stacked(
                t!("rule-format"),
                Some(t!("rule-format-desc")),
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .children(presets.map(|preset| {
                        let label = preset
                            .map(|p| SharedString::from(format::preset_label(p)))
                            .unwrap_or_else(|| t!("rule-default"));
                        chip(
                            format!("rule-preset-{preset:?}"),
                            label,
                            draft.preset == preset,
                        )
                        .on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.draft(|d| d.preset = preset, cx)
                            }),
                        )
                    })),
                cx,
            ));
        }
        let fixes = std::iter::once(None).chain(Fix::ALL.into_iter().map(Some));
        let fix_desc = draft.fix.map(|fix| {
            i18n::lookup(
                &format!(
                    "{}-desc",
                    match fix {
                        Fix::Compatible => "fix-compatible",
                        Fix::EditFriendly => "fix-edit-friendly",
                        Fix::Repair => "fix-repair",
                        Fix::Normalize => "fix-normalize",
                        Fix::Smaller => "fix-smaller",
                    }
                ),
                None,
            )
        });
        rows.push(stacked(
            t!("rule-fix"),
            Some(fix_desc.unwrap_or_else(|| t!("rule-fix-desc"))),
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .children(fixes.map(|fix| {
                    let label = fix
                        .map(|f| SharedString::from(format::fix_name(f)))
                        .unwrap_or_else(|| t!("rule-fix-none"));
                    chip(format!("rule-fix-{fix:?}"), label, draft.fix == fix).on_click(
                        cx.listener(move |this, _, _, cx| this.draft(|d| d.fix = fix, cx)),
                    )
                })),
            cx,
        ));
        if !fallback {
            // the draft stands in for its own place in the list, so the test shows what saving does
            let mut rules = s.site_rules.clone();
            match editor.at {
                Some(at) if at < rules.len() => rules[at] = draft,
                _ => rules.push(draft),
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
            rows.push(stacked(
                t!("rule-test"),
                Some(t!("rule-test-desc")),
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().font_family(MONO).child(editor.test.clone()))
                    .children(result.map(|line| {
                        div()
                            .text_size(theme.text(Text::Small))
                            .text_color(theme.muted_foreground)
                            .child(line)
                    })),
                cx,
            ));
        }
        let can_save = fallback || check.is_ok();
        rows.push(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .py(px(10.))
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
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.rule_editor = None;
                            cx.notify();
                        })),
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
        section(
            Some(match fallback {
                true => t!("rule-fallback"),
                false => t!("rule-edit-title"),
            }),
            rows,
            cx,
        )
    }
}
