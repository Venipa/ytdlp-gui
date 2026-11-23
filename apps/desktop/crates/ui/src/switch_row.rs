use gpui::accesskit::{Role, Toggled};
use gpui::prelude::*;
use gpui::{
    App, ClickEvent, Div, ElementId, Interactivity, SharedString, Stateful, StyleRefinement,
    Window, div,
};

use crate::metrics::Text;
use crate::switch::Switch;
use crate::theme::ActiveTheme as _;

type Click = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// A settings entry whose only control is a switch, drawn as one button from edge to edge: the
/// title and an optional explanation at the start, the switch at the end. A press anywhere on
/// the row toggles it, and the row is the one switch screen readers hear, named after its
/// title. The caller sets the inset and the corners, concentric with the card it sits in.
#[derive(IntoElement)]
pub struct SwitchRow {
    id: ElementId,
    base: Stateful<Div>,
    title: SharedString,
    detail: Option<SharedString>,
    checked: bool,
    disabled: bool,
    on_click: Option<Click>,
}

impl SwitchRow {
    #[track_caller]
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>, checked: bool) -> Self {
        let id = id.into();

        Self {
            base: div().id(id.clone()),
            id,
            title: title.into(),
            detail: None,
            checked,
            disabled: false,
            on_click: None,
        }
    }

    /// Adds a muted explanation below the title, which screen readers hear as the row's
    /// description. It wraps well short of the switch.
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Mutes the row and takes no presses, for a setting that is unavailable.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl Styled for SwitchRow {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for SwitchRow {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl RenderOnce for SwitchRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            id,
            mut base,
            title,
            detail,
            checked,
            disabled,
            on_click,
        } = self;
        let theme = *cx.theme();
        let overrides = std::mem::take(base.style());
        let hover = theme.secondary_hover;
        let press = theme.secondary_active;

        let mut row = base
            .role(Role::Switch)
            .aria_label(title.clone())
            .aria_toggled(Toggled::from(checked))
            .when_some(detail.clone(), |this, detail| this.aria_description(detail))
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .min_h(theme.metrics.list_row)
            .rounded(theme.radius)
            .when(disabled, |this| this.text_color(theme.muted_foreground))
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .active(move |style| style.bg(press))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().truncate().child(title))
                    .children(detail.map(|detail| {
                        div()
                            .pr_8()
                            .text_size(theme.text(Text::Small))
                            .text_color(theme.muted_foreground)
                            .child(detail)
                    })),
            )
            .child(
                Switch::new((id, "switch"), checked)
                    .silent()
                    .disabled(disabled),
            );

        row.style().refine(&overrides);
        if !disabled && let Some(handler) = on_click {
            row = row.on_click(handler);
        }
        row
    }
}
