//! Settings, Network, Advanced: the custom argument lists. Each tool's arguments are ledger lines,
//! one option per line with a call number, the value in ink and the check at the end; Enter adds a
//! line, a pasted run of options splits into lines, and "+" shortcuts add the common ones.
use std::time::Duration;

use domain::{ArgCheck, Settings};
use gpui::{
    AnyElement, Context, Entity, SharedString, Window, accesskit::Role, div, prelude::*, px,
};
use i18n::t;
use ui::{ActiveTheme as _, Button, FORM_CONTEXT, Input, MONO, Perch, Submit, Text, Tipped as _};

use super::SettingsView;

/// How long typing must pause before yt-dlp reads the lines.
const SETTLE: Duration = Duration::from_millis(500);

/// A tool that takes custom arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ArgTool {
    Ytdlp,
    Aria2c,
}

impl ArgTool {
    pub(super) const ALL: [ArgTool; 2] = [ArgTool::Ytdlp, ArgTool::Aria2c];

    fn lines(self, s: &Settings) -> &[String] {
        match self {
            ArgTool::Ytdlp => &s.ytdlp_args,
            ArgTool::Aria2c => &s.aria2c_args,
        }
    }

    fn lines_mut(self, s: &mut Settings) -> &mut Vec<String> {
        match self {
            ArgTool::Ytdlp => &mut s.ytdlp_args,
            ArgTool::Aria2c => &mut s.aria2c_args,
        }
    }

    fn id(self) -> &'static str {
        match self {
            ArgTool::Ytdlp => "yt-dlp",
            ArgTool::Aria2c => "aria2c",
        }
    }

    /// The "+" shortcuts: the option it stands for, the line it adds, its tooltip.
    fn shortcuts(self) -> &'static [(&'static str, &'static str, &'static str)] {
        match self {
            ArgTool::Ytdlp => &[
                (
                    "--sleep-interval",
                    "--sleep-interval 2",
                    "set-args-tip-sleep",
                ),
                (
                    "--user-agent",
                    "--user-agent \"Mozilla/5.0\"",
                    "set-args-tip-agent",
                ),
                ("--no-mtime", "--no-mtime", "set-args-tip-mtime"),
                (
                    "--concurrent-fragments",
                    "--concurrent-fragments 4",
                    "set-args-tip-fragments",
                ),
            ],
            ArgTool::Aria2c => &[
                ("--max-tries", "--max-tries=5", "set-args-tip-tries"),
                (
                    "--lowest-speed-limit",
                    "--lowest-speed-limit=50K",
                    "set-args-tip-lowest",
                ),
                (
                    "--max-connection-per-server",
                    "--max-connection-per-server=8",
                    "set-args-tip-connections",
                ),
            ],
        }
    }
}

/// One tool's lines being edited: an input per line and a last empty one that adds a line.
pub(super) struct ArgList {
    tool: ArgTool,
    inputs: Vec<Entity<Input>>,
    /// The lines last written to (or read from) the settings.
    shown: Vec<String>,
}

/// The option of a line and the rest of it (`--a 1` is `--a` and ` 1`, `--a=1` is `--a` and `=1`).
fn option_and_value(line: &str) -> (&str, &str) {
    let end = line.find([' ', '=']).unwrap_or(line.len());
    line.split_at(end)
}

impl SettingsView {
    /// The lists as the settings hold them.
    pub(super) fn arg_lists(s: &Settings, cx: &mut Context<Self>) -> Vec<ArgList> {
        ArgTool::ALL
            .into_iter()
            .map(|tool| {
                let shown = tool.lines(s).to_vec();
                let inputs = shown
                    .iter()
                    .map(String::as_str)
                    .chain([""])
                    .map(|line| Self::arg_input(tool, line, cx))
                    .collect();
                ArgList {
                    tool,
                    inputs,
                    shown,
                }
            })
            .collect()
    }

    fn arg_input(tool: ArgTool, line: &str, cx: &mut Context<Self>) -> Entity<Input> {
        let input = cx.new(|cx| Input::new("set-args-add", cx).bare().compact());
        input.update(cx, |input, cx| input.set_text(line.to_owned(), cx));
        cx.observe(&input, move |this, _, cx| this.arg_typed(tool, cx))
            .detach();
        input
    }

    fn list(&mut self, tool: ArgTool) -> &mut ArgList {
        self.arg_lists
            .iter_mut()
            .find(|l| l.tool == tool)
            .expect("a list per tool")
    }

    /// The settings changed: a list that moved without the user typing here is read again.
    pub(super) fn args_changed(&mut self, s: &Settings, cx: &mut Context<Self>) {
        for tool in ArgTool::ALL {
            if self.list(tool).shown.as_slice() != tool.lines(s) {
                let lines: Vec<String> = tool.lines(s).to_vec();
                let inputs = lines
                    .iter()
                    .map(String::as_str)
                    .chain([""])
                    .map(|line| Self::arg_input(tool, line, cx))
                    .collect();
                let list = self.list(tool);
                list.inputs = inputs;
                list.shown = lines;
            }
        }
        self.check_args_soon(cx);
    }

    /// The text changed: the lines are saved, a last line is kept empty for the next one, and
    /// yt-dlp reads the lines once typing pauses.
    fn arg_typed(&mut self, tool: ArgTool, cx: &mut Context<Self>) {
        let texts: Vec<String> = self
            .list(tool)
            .inputs
            .iter()
            .map(|i| i.read(cx).text().trim().to_owned())
            .collect();
        if texts.last().is_some_and(|t| !t.is_empty()) {
            let input = Self::arg_input(tool, "", cx);
            self.list(tool).inputs.push(input);
        }
        let lines: Vec<String> = texts.into_iter().filter(|t| !t.is_empty()).collect();
        if self.list(tool).shown != lines {
            self.list(tool).shown = lines.clone();
            self.settings().update(cx, |s, cx| {
                s.update(|st| *tool.lines_mut(st) = lines.clone(), cx)
            });
        }
        cx.notify();
    }

    /// Enter: a line holding several options becomes a line each, empty lines go, and the line
    /// after it takes the focus.
    fn arg_enter(
        &mut self,
        tool: ArgTool,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.list(tool).inputs[index].read(cx).text().to_owned();
        let split = domain::args::split(&text)
            .map(|args| domain::args::lines(&args))
            .filter(|lines| lines.len() > 1);
        let mut next = index + 1;
        if let Some(lines) = split {
            self.list(tool).inputs[index].update(cx, |i, cx| i.set_text(lines[0].clone(), cx));
            for (n, line) in lines.iter().enumerate().skip(1) {
                let input = Self::arg_input(tool, line, cx);
                self.list(tool).inputs.insert(index + n, input);
            }
            next = index + lines.len();
        }
        // empty lines above the last one go; the focus follows its line
        let list = self.list(tool);
        let last = list.inputs.len() - 1;
        let mut kept = vec![];
        for (n, input) in list.inputs.drain(..).enumerate() {
            if n == last || !input.read(cx).text().trim().is_empty() {
                kept.push(input);
            } else if n < next {
                next -= 1;
            }
        }
        list.inputs = kept;
        let target = list.inputs[next.min(list.inputs.len() - 1)].clone();
        target.update(cx, |i, cx| i.focus(window, cx));
        self.arg_typed(tool, cx);
    }

    fn arg_remove(&mut self, tool: ArgTool, index: usize, cx: &mut Context<Self>) {
        let list = self.list(tool);
        if index + 1 < list.inputs.len() {
            list.inputs.remove(index);
        }
        self.arg_typed(tool, cx);
    }

    /// A "+" shortcut: its line goes into the empty last line, which then takes the focus.
    fn arg_add(&mut self, tool: ArgTool, line: &str, window: &mut Window, cx: &mut Context<Self>) {
        let last = self.list(tool).inputs.last().cloned().expect("a last line");
        last.update(cx, |i, cx| {
            i.set_text(line.to_owned(), cx);
            i.focus(window, cx);
        });
        self.arg_typed(tool, cx);
    }

    /// Asks yt-dlp about the yt-dlp lines once typing has paused.
    fn check_args_soon(&mut self, cx: &mut Context<Self>) {
        self.arg_check = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            this.update(cx, |this, cx| {
                let lines = this.list(ArgTool::Ytdlp).shown.clone();
                this.state.args.update(cx, |a, cx| a.check(&lines, cx));
            })
            .ok();
        }));
    }

    /// The Custom arguments content: a list per tool.
    pub(super) fn args_lists(
        &self,
        s: &Settings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .children(ArgTool::ALL.map(|tool| self.args_list(tool, s, window, cx)))
            .into_any_element()
    }

    fn args_list(
        &self,
        tool: ArgTool,
        s: &Settings,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        let list = self
            .arg_lists
            .iter()
            .find(|l| l.tool == tool)
            .expect("a list per tool");
        let checks = self.state.args.read(cx);
        let count = list.shown.len();
        let side = match tool {
            ArgTool::Aria2c if !s.aria2c => t!("set-args-aria2c-off"),
            ArgTool::Ytdlp if checks.checking() => t!("set-args-checking"),
            _ => t!("set-args-count", count = count),
        };
        let last = list.inputs.len() - 1;
        // collected so the read of the checks ends before the lines are drawn with `cx`
        #[allow(clippy::needless_collect)]
        let rows: Vec<_> = list
            .inputs
            .iter()
            .map(|input| {
                let text = input.read(cx).text().trim().to_owned();
                let check = match (text.is_empty(), tool) {
                    (true, _) => None,
                    (false, ArgTool::Ytdlp) => checks.ytdlp(&text),
                    (false, ArgTool::Aria2c) => Some(checks.aria2c(&text)),
                };
                let focused = input.read(cx).is_focused(window);
                (text, check, focused, input.clone())
            })
            .collect();
        let lines: Vec<AnyElement> = rows
            .into_iter()
            .enumerate()
            .map(|(index, (text, check, focused, input))| {
                self.arg_line(
                    tool,
                    index,
                    index == last,
                    &text,
                    check,
                    focused,
                    &input,
                    cx,
                )
            })
            .collect();
        let present = |option: &str| list.shown.iter().any(|l| option_and_value(l).0 == option);
        let shortcuts: Vec<AnyElement> = tool
            .shortcuts()
            .iter()
            .filter(|(option, _, _)| !present(option))
            .map(|(option, line, tip)| {
                Button::new(SharedString::from(format!(
                    "args-add-{}-{option}",
                    tool.id()
                )))
                .small()
                .ghost()
                .border_1()
                .border_dashed()
                .border_color(theme.border)
                .icon("icons/plus.svg")
                .label(*option)
                .font_family(MONO)
                .tooltip(*tip)
                .on_click(
                    cx.listener(move |this, _, window, cx| this.arg_add(tool, line, window, cx)),
                )
                .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .when(tool == ArgTool::Aria2c && !s.aria2c, |this| {
                this.opacity(0.55)
            })
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(theme.text(Text::Small))
                    .child(div().text_color(theme.foreground).child(tool.id()))
                    .child(div().text_color(theme.muted_foreground).child(side)),
            )
            .child(
                div()
                    .key_context(FORM_CONTEXT)
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius)
                    .font_family(MONO)
                    .children(lines),
            )
            .when(!shortcuts.is_empty(), |this| {
                this.child(div().flex().flex_wrap().gap(px(6.)).children(shortcuts))
            })
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn arg_line(
        &self,
        tool: ArgTool,
        index: usize,
        is_last: bool,
        text: &str,
        check: Option<ArgCheck>,
        focused: bool,
        input: &Entity<Input>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = *cx.theme();
        let ink = crate::ink(cx);
        let invalid = matches!(check, Some(ArgCheck::Invalid { .. }));
        let group = SharedString::from(format!("args-{}-{index}", tool.id()));
        let (status, color, tip): (Option<SharedString>, _, Option<SharedString>) = match &check {
            Some(ArgCheck::Ok) => (None, theme.muted_foreground, None),
            Some(ArgCheck::Reserved) => (
                Some(t!("set-args-reserved")),
                theme.progress_bar,
                Some(t!("set-args-reserved-tip")),
            ),
            // the short line in the row, yt-dlp's whole message in its tooltip
            Some(ArgCheck::Invalid {
                message,
                suggestion,
            }) => match suggestion {
                Some(option) => (
                    Some(t!("set-args-did-you-mean", option = option.clone())),
                    theme.danger,
                    Some(t!(
                        "set-args-did-you-mean-tip",
                        message = message.clone(),
                        option = option.clone()
                    )),
                ),
                None => (
                    Some(SharedString::from(message.clone())),
                    theme.danger,
                    Some(SharedString::from(message.clone())),
                ),
            },
            None => (None, theme.muted_foreground, None),
        };
        let mark = match (&check, &status) {
            (Some(ArgCheck::Ok), _) => Some(
                gpui::svg()
                    .path(icons::path("icons/check.svg"))
                    .size(px(12.))
                    .text_color(theme.muted_foreground)
                    .into_any_element(),
            ),
            (_, Some(status)) => Some(
                div()
                    .max_w(px(260.))
                    .truncate()
                    .text_color(color)
                    .child(status.clone())
                    .into_any_element(),
            ),
            _ => None,
        };
        let content = if focused || text.is_empty() {
            div()
                .flex_1()
                .min_w_0()
                .child(input.clone())
                .into_any_element()
        } else {
            let (option, value) = option_and_value(text);
            div()
                .id(SharedString::from(format!("{group}-text")))
                .role(Role::Button)
                .aria_label(t!("set-args-edit-line", line = text.to_owned()))
                .flex_1()
                .min_w_0()
                .flex()
                // the field's own inset, so a line does not jump when it is clicked
                .px_3()
                .overflow_hidden()
                .whitespace_nowrap()
                .cursor_text()
                .child(option.to_owned())
                .child(div().text_color(ink).child(value.to_owned()))
                .on_click({
                    let input = input.clone();
                    move |_, window, cx| input.update(cx, |i, cx| i.focus(window, cx))
                })
                .into_any_element()
        };
        div()
            .id(group.clone())
            .group(group.clone())
            .flex()
            .items_center()
            .h(px(26.))
            .pr(px(6.))
            .text_size(theme.text(Text::Small))
            .when(index > 0, |this| {
                this.border_t_1().border_color(theme.border)
            })
            .when(focused, |this| this.bg(theme.secondary_hover))
            .when(invalid, |this| this.bg(theme.danger.opacity(0.08)))
            .on_action(cx.listener(move |this, _: &Submit, window, cx| {
                this.arg_enter(tool, index, window, cx)
            }))
            .child(
                div()
                    .w(px(34.))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .pr(px(10.))
                    .text_color(if invalid {
                        theme.danger
                    } else {
                        theme.muted_foreground
                    })
                    .child(format!("{:02}", index + 1)),
            )
            .child(content)
            .children(mark.map(|mark| {
                let mark = div()
                    .id(SharedString::from(format!("{group}-mark")))
                    .flex_none()
                    .pl(px(10.))
                    .child(mark);
                match tip {
                    Some(tip) => mark.tip_label(tip, Perch::Above).into_any_element(),
                    None => mark.into_any_element(),
                }
            }))
            .when(!is_last && !text.is_empty(), |this| {
                this.child(
                    div()
                        .flex_none()
                        .pl(px(4.))
                        .invisible()
                        .group_hover(group.clone(), |style| style.visible())
                        .child(
                            Button::new(SharedString::from(format!("{group}-remove")))
                                .small()
                                .ghost()
                                .icon("icons/x.svg")
                                .name(t!("set-args-remove", line = text.to_owned()))
                                .tooltip("set-args-remove-tip")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.arg_remove(tool, index, cx)
                                })),
                        ),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::option_and_value;

    #[test]
    fn a_line_splits_at_its_option() {
        assert_eq!(
            option_and_value("--sleep-interval 2"),
            ("--sleep-interval", " 2")
        );
        assert_eq!(option_and_value("--max-tries=5"), ("--max-tries", "=5"));
        assert_eq!(option_and_value("--no-mtime"), ("--no-mtime", ""));
    }
}
