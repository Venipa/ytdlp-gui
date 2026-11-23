use crate::metrics::Text;
use crate::motion::HoverFade;
use crate::theme::ActiveTheme as _;
use std::cell::Cell;
use std::f32::consts::TAU;
use std::rc::Rc;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    App, Bounds, Div, DragMoveEvent, ElementId, Empty, Hsla, MouseButton, MouseDownEvent,
    MouseUpEvent, PathBuilder, Pixels, Point, Render, SharedString, Stateful, StyleRefinement,
    Window, canvas, div, point, px, relative,
};
use i18n::t;

const TRACK: f32 = 0.5;
const THUMB: f32 = 1.5;
const HIT: f32 = 3.;
const BUBBLE: f32 = 7.;
/// How far the bubble stands off the track unless `lift` says otherwise.
const LIFT: Pixels = px(12.);
/// How far one screen reader step moves the slider, as a share of its length.
const STEP: f32 = 0.05;
/// The M3 expressive slider: the track is this many thin lines thick, the bar handle this wide
/// with this much gap either side of it, the handle stands this many tracks tall, and the
/// stop dot at the end of the track is this wide. The corners where a track meets the handle
/// are `NOTCH`.
const THICK: f32 = 2.;
const HANDLE: Pixels = px(3.);
const CUT: Pixels = px(4.);
const HANDLE_TALL: f32 = 2.5;
const STOP: Pixels = px(4.);
const NOTCH: Pixels = px(2.);
/// How long an edge seek line takes to grow into the thick track under the pointer.
const GROW: std::time::Duration = std::time::Duration::from_millis(150);
/// The wavy track's crest-to-crest length, how high a crest stands as a share of the track's
/// thickness, how many radians a second the wave travels while playing, and how long it takes
/// to rise or flatten.
const WAVELENGTH: Pixels = px(24.);
const CREST: f32 = 0.5;
const DRIFT: f32 = 4.;
const SWELL: std::time::Duration = std::time::Duration::from_millis(300);
/// How many straight pieces one wavelength of the wavy track is drawn from.
const PIECES: f32 = 16.;

type Slide = Box<dyn Fn(&f32, &mut Window, &mut App) + 'static>;
type Release = Box<dyn Fn(&MouseUpEvent, &mut Window, &mut App) + 'static>;

fn track(pad: Pixels) -> Pixels {
    px((pad / px(1.) * TRACK).round())
}

fn thumb(pad: Pixels) -> Pixels {
    px((pad / px(1.) * THUMB).round())
}

fn hit(pad: Pixels) -> Pixels {
    px((pad / px(1.) * HIT).round())
}

/// How a seek bar looks: the classic thin line with a round dot under the pointer, the M3
/// expressive track with its bar handle, or that track with a wave travelling along the played
/// part while the music plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SeekStyle {
    Line,
    #[default]
    Material,
    Wavy,
}

impl SeekStyle {
    pub const ALL: [Self; 3] = [Self::Line, Self::Material, Self::Wavy];

    pub fn id(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Material => "material",
            Self::Wavy => "wavy",
        }
    }

    /// Reads a saved id, keeping Material for a missing or unknown one.
    pub fn from_id(id: &str) -> Self {
        match id {
            "line" => Self::Line,
            "wavy" => Self::Wavy,
            _ => Self::default(),
        }
    }

    /// The localized name. Resolved at render, never stored.
    pub fn label(self) -> SharedString {
        match self {
            Self::Line => t!("settings-seek-style-line"),
            Self::Material => t!("settings-seek-style-material"),
            Self::Wavy => t!("settings-seek-style-wavy"),
        }
    }
}

/// Where the wavy track's wave stands: how high it is, eased between flat and full, and when it
/// started travelling, for its phase.
struct Wave {
    swell: HoverFade,
    since: Instant,
}

#[derive(Clone)]
struct Grab(SharedString);

impl Render for Grab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub struct ScrubberState {
    id: SharedString,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl ScrubberState {
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            bounds: Rc::new(Cell::new(Bounds::default())),
        }
    }

    pub fn hovered(&self, position: Point<Pixels>, pad: Pixels) -> Option<f32> {
        self.bounds
            .get()
            .contains(&position)
            .then(|| self.fraction_at(position.x, pad))
    }

    /// How far the thumb of a horizontal scrubber moves from one end to the other, as last
    /// drawn. It is zero before the first paint.
    pub fn travel(&self, pad: Pixels) -> Pixels {
        (self.bounds.get().size.width - thumb(pad)).max(px(0.))
    }

    fn fraction_at(&self, x: Pixels, pad: Pixels) -> f32 {
        let bounds = self.bounds.get();
        let pin = thumb(pad);
        let travel = self.travel(pad);
        if travel <= px(0.) {
            return 0.;
        }
        ((x - bounds.origin.x - pin / 2.) / travel).clamp(0., 1.)
    }

    fn climb_at(&self, y: Pixels, pad: Pixels) -> f32 {
        let bounds = self.bounds.get();
        let pin = thumb(pad);
        let travel = bounds.size.height - pin;
        if travel <= px(0.) {
            return 0.;
        }
        1. - ((y - bounds.origin.y - pin / 2.) / travel).clamp(0., 1.)
    }
}

#[derive(IntoElement)]
pub struct Scrubber {
    base: Stateful<Div>,
    id: SharedString,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    fraction: f32,
    filled: Hsla,
    empty: Hsla,
    thumb: Hsla,
    enabled: bool,
    /// Whether the thumb is drawn, which a seek line on a bar's edge shows only on hover.
    knob: bool,
    /// Whether the line sits flush with the top of the hit area, for a line on a bar's edge.
    edge: bool,
    vertical: bool,
    look: SeekStyle,
    /// Whether the music plays, which sets a wavy track's wave travelling.
    playing: bool,
    /// What a screen reader calls the slider.
    name: Option<SharedString>,
    /// What a screen reader reads as the position in place of the bare fraction.
    value: Option<SharedString>,
    bubble: Option<(f32, SharedString)>,
    ranges: Vec<(f32, f32, Hsla)>,
    lift: Pixels,
    on_move: Option<Slide>,
    on_release: Option<Release>,
}

impl Scrubber {
    pub fn new(state: &ScrubberState, fraction: f32) -> Self {
        Self {
            base: div().id(ElementId::Name(state.id.clone())),
            id: state.id.clone(),
            bounds: state.bounds.clone(),
            fraction: fraction.clamp(0., 1.),
            filled: gpui::white(),
            empty: gpui::black(),
            thumb: gpui::white(),
            enabled: true,
            knob: true,
            edge: false,
            vertical: false,
            look: SeekStyle::default(),
            playing: false,
            name: None,
            value: None,
            bubble: None,
            ranges: Vec::new(),
            lift: LIFT,
            on_move: None,
            on_release: None,
        }
    }

    pub fn bubble(mut self, fraction: f32, text: impl Into<SharedString>) -> Self {
        self.bubble = Some((fraction.clamp(0., 1.), text.into()));
        self
    }

    pub fn lift(mut self, lift: Pixels) -> Self {
        self.lift = lift;
        self
    }

    /// How far a bubble standing `lift` off the track can reach past it, for an owner that
    /// composites the scrubber inside a layer of its own and has to leave the bubble room in it.
    pub fn bubble_reach(lift: Pixels, cx: &App) -> Pixels {
        lift + px((cx.theme().metrics.pad / px(1.) * BUBBLE).round())
    }

    pub fn colors(mut self, filled: Hsla, empty: Hsla, thumb: Hsla) -> Self {
        self.filled = filled;
        self.empty = empty;
        self.thumb = thumb;
        self
    }

    /// Marks fractional ranges without changing pointer or drag behavior. Passed sections use
    /// a blend toward the theme background so they stay visible over the filled track.
    pub fn ranges(mut self, ranges: impl IntoIterator<Item = (f32, f32)>, color: Hsla) -> Self {
        self.ranges = ranges
            .into_iter()
            .filter_map(|(start, end)| {
                if !start.is_finite() || !end.is_finite() {
                    return None;
                }
                let (start, end) = (start.clamp(0., 1.), end.clamp(0., 1.));
                (end > start).then_some((start, end, color))
            })
            .collect();
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draws the thumb or leaves it out, the drag and the clicks working either way.
    pub fn knob(mut self, knob: bool) -> Self {
        self.knob = knob;
        self
    }

    /// Draws the line at the top of the hit area instead of its middle, so a thin line can lie
    /// along a bar's edge with the generous hit area below it.
    pub fn edge(mut self) -> Self {
        self.edge = true;
        self
    }

    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }

    /// Draws a horizontal scrubber in `look`, for the seek bars; other sliders keep Material.
    pub fn look(mut self, look: SeekStyle) -> Self {
        self.look = look;
        self
    }

    /// Tells a wavy track whether the music plays, so its wave travels or lies flat.
    pub fn playing(mut self, playing: bool) -> Self {
        self.playing = playing;
        self
    }

    /// Names the slider for screen readers after what it sets, such as the volume.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// The position as a screen reader should read it, such as the elapsed time or a percentage,
    /// in place of the bare fraction between 0 and 1.
    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = Some(value.into());
        self
    }

    pub fn on_move(mut self, handler: impl Fn(&f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_move = Some(Box::new(handler));
        self
    }

    pub fn on_release(
        mut self,
        handler: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_release = Some(Box::new(handler));
        self
    }
}

impl Styled for Scrubber {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Scrubber {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let pad = cx.theme().metrics.pad;
        let line = track(pad);
        let pin = thumb(pad);
        let reach = hit(pad);
        let bubble_width = px((pad / px(1.) * BUBBLE).round());
        // The value bubble is a plain tooltip in the inverse colours: the ink as its fill and
        // the ground as its text.
        let popover = cx.theme().foreground;
        let popover_border = popover;
        let popover_text = cx.theme().background;
        let text_size = cx.theme().text(Text::Tiny);

        let Self {
            mut base,
            id,
            bounds,
            fraction,
            filled,
            empty,
            thumb,
            enabled,
            knob,
            edge,
            vertical,
            look,
            playing,
            name,
            value,
            bubble,
            ranges,
            lift,
            on_move,
            on_release,
        } = self;
        let look = match vertical {
            true => SeekStyle::Material,
            false => look,
        };
        let overrides = std::mem::take(base.style());

        let state = Rc::new(ScrubberState {
            id: id.clone(),
            bounds: bounds.clone(),
        });
        let on_move = on_move.map(Rc::new);
        let on_release = on_release.map(Rc::new);

        let at = move |state: &ScrubberState, position: Point<Pixels>| match vertical {
            true => state.climb_at(position.y, pad),
            false => state.fraction_at(position.x, pad),
        };

        let down = {
            let state = state.clone();
            let on_move = on_move.clone();
            move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                if let Some(handler) = on_move.as_ref() {
                    handler(&at(&state, event.position), window, cx);
                }
            }
        };

        let dragged = {
            let on_move = on_move.clone();
            let mine = id.clone();
            move |event: &DragMoveEvent<Grab>, window: &mut Window, cx: &mut App| {
                if event.drag(cx).0 != mine {
                    return;
                }
                if let Some(handler) = on_move.as_ref() {
                    handler(&at(&state, event.event.position), window, cx);
                }
            }
        };

        // A screen reader steps the slider the way a short drag would, moving it and letting go,
        // so an owner that only commits on release still hears the step.
        let steps = on_move.is_some();
        let stepped = {
            let on_move = on_move.clone();
            let on_release = on_release.clone();
            move |by: f32, window: &mut Window, cx: &mut App| {
                let Some(handler) = on_move.as_ref() else {
                    return;
                };
                handler(&(fraction + by).clamp(0., 1.), window, cx);
                if let Some(release) = on_release.as_ref() {
                    release(&MouseUpEvent::default(), window, cx);
                }
            }
        };

        let released = move |event: &MouseUpEvent, window: &mut Window, cx: &mut App| {
            if let Some(handler) = on_release.as_ref() {
                handler(event, window, cx);
            }
        };

        let inset = match enabled {
            true => pin / 2.,
            false => Pixels::ZERO,
        };

        let ranges: Vec<_> = ranges
            .into_iter()
            .flat_map(|(start, end, color)| {
                [
                    (
                        start,
                        end.min(fraction),
                        filled.blend(cx.theme().background.opacity(0.45)),
                    ),
                    (start.max(fraction), end, color),
                ]
            })
            .filter(|(start, end, _)| end > start)
            .collect();

        // A slider whose handle shows is drawn as the M3 expressive slider; a seek line on a bar's
        // edge grows into one over a short ease while hovered (at once under reduced motion) and
        // stays a thin line at rest.
        let grow = match edge && look != SeekStyle::Line {
            true => {
                let fade =
                    window.use_keyed_state((ElementId::Name(id.clone()), "grow"), cx, |_, _| {
                        HoverFade::new(GROW)
                    });
                fade.update(cx, |fade, cx| {
                    fade.set(knob);
                    fade.level(window, cx)
                })
            }
            false => f32::from(u8::from(knob)),
        };
        // A wavy track stays expressive at rest, its wave on the thin line, and a line never is.
        let expressive = enabled
            && match look {
                SeekStyle::Line => false,
                SeekStyle::Material => grow > 0.,
                SeekStyle::Wavy => true,
            };
        let thick = line + (line * THICK - line) * grow;
        let wave = (look == SeekStyle::Wavy && enabled).then(|| {
            let wave =
                window.use_keyed_state((ElementId::Name(id.clone()), "wave"), cx, |_, _| Wave {
                    swell: HoverFade::new(SWELL),
                    since: Instant::now(),
                });
            wave.update(cx, |wave, cx| {
                let moving = playing && !cx.reduce_motion();
                wave.swell.set(moving);
                let height = wave.swell.level(window, cx);
                if moving {
                    window.request_animation_frame();
                }
                (height, wave.since.elapsed().as_secs_f32() * DRIFT % TAU)
            })
        });
        let track = Track {
            fraction,
            inset,
            thick,
            grow,
            wave: wave.filter(|(height, _)| *height > 0.),
            filled,
            empty,
            thumb,
            vertical,
        };
        let bar = match vertical {
            true if expressive => {
                expressive_track(track, ranges).when_some(bubble, |this, (at, text)| {
                    this.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .top(inset)
                            .bottom(inset)
                            .child(
                                div()
                                    .absolute()
                                    .map(|this| {
                                        if lift >= Pixels::ZERO {
                                            this.right(thick + lift)
                                        } else {
                                            this.left(thick - lift)
                                        }
                                    })
                                    .bottom(relative(at))
                                    .mb(Pixels::ZERO - pin / 2.)
                                    .h(pin)
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .px_1p5()
                                            .rounded_md()
                                            .bg(popover)
                                            .border_1()
                                            .border_color(popover_border)
                                            .text_color(popover_text)
                                            .text_size(text_size)
                                            .whitespace_nowrap()
                                            .child(text),
                                    ),
                            ),
                    )
                })
            }
            false if expressive => {
                expressive_track(track, ranges)
                    // An edge line grows around its resting centre, over the layout, not down.
                    .when(edge, |this| {
                        this.absolute().top((line - thick) / 2.).left_0().right_0()
                    })
                    .when_some(bubble, |this, (at, text)| {
                        this.child(
                            div()
                                .absolute()
                                .map(|this| {
                                    if lift >= Pixels::ZERO {
                                        this.bottom(lift)
                                    } else {
                                        this.top(Pixels::ZERO - lift)
                                    }
                                })
                                .left(inset)
                                .right(inset)
                                .child(
                                    div()
                                        .absolute()
                                        .bottom_0()
                                        .left(relative(at))
                                        .ml(Pixels::ZERO - bubble_width / 2.)
                                        .w(bubble_width)
                                        .flex()
                                        .justify_center()
                                        .child(
                                            div()
                                                .px_1p5()
                                                .rounded_md()
                                                .bg(popover)
                                                .border_1()
                                                .border_color(popover_border)
                                                .text_color(popover_text)
                                                .text_size(text_size)
                                                .child(text),
                                        ),
                                ),
                        )
                    })
            }
            true => div()
                .relative()
                .h_full()
                .w(line)
                .rounded_full()
                .bg(empty)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(inset)
                        .bottom(inset)
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom(Pixels::ZERO - inset)
                                .top(relative(1. - fraction))
                                .rounded_full()
                                .bg(filled),
                        )
                        .children(ranges.into_iter().map(|(start, end, color)| {
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom(relative(start))
                                .h(relative(end - start))
                                .rounded_full()
                                .bg(color)
                        }))
                        .when(enabled, |this| {
                            this.child(
                                div()
                                    .absolute()
                                    .left((line - pin) / 2.)
                                    .bottom(relative(fraction))
                                    .mb(Pixels::ZERO - pin / 2.)
                                    .size(pin)
                                    .rounded_full()
                                    .bg(thumb),
                            )
                        })
                        .when_some(bubble, |this, (at, text)| {
                            this.child(
                                div()
                                    .absolute()
                                    .map(|this| {
                                        if lift >= Pixels::ZERO {
                                            this.right(line + lift)
                                        } else {
                                            this.left(line - lift)
                                        }
                                    })
                                    .bottom(relative(at))
                                    .mb(Pixels::ZERO - pin / 2.)
                                    .h(pin)
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .px_1p5()
                                            .rounded_md()
                                            .bg(popover)
                                            .border_1()
                                            .border_color(popover_border)
                                            .text_color(popover_text)
                                            .text_size(text_size)
                                            .whitespace_nowrap()
                                            .child(text),
                                    ),
                            )
                        }),
                ),
            false => div()
                .relative()
                .w_full()
                .h(line)
                .rounded_full()
                .bg(empty)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(inset)
                        .right(inset)
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(Pixels::ZERO - inset)
                                .right(relative(1. - fraction))
                                .rounded_full()
                                .bg(filled),
                        )
                        .children(ranges.into_iter().map(|(start, end, color)| {
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(relative(start))
                                .w(relative(end - start))
                                .rounded_full()
                                .bg(color)
                        }))
                        .when(enabled && knob, |this| {
                            this.child(
                                div()
                                    .absolute()
                                    .top((line - pin) / 2.)
                                    .left(relative(fraction))
                                    .ml(Pixels::ZERO - pin / 2.)
                                    .size(pin)
                                    .rounded_full()
                                    .bg(thumb)
                                    // A line's dot shows under the pointer alone.
                                    .when(look == SeekStyle::Line && !edge, |this| {
                                        this.invisible()
                                            .group_hover(id.clone(), |style| style.visible())
                                    }),
                            )
                        })
                        .when_some(bubble, |this, (at, text)| {
                            this.child(
                                div()
                                    .absolute()
                                    .map(|this| {
                                        if lift >= Pixels::ZERO {
                                            this.bottom(lift)
                                        } else {
                                            this.top(Pixels::ZERO - lift)
                                        }
                                    })
                                    .left(relative(at))
                                    .ml(Pixels::ZERO - bubble_width / 2.)
                                    .w(bubble_width)
                                    .flex()
                                    .justify_center()
                                    .child(
                                        div()
                                            .px_1p5()
                                            .rounded_md()
                                            .bg(popover)
                                            .border_1()
                                            .border_color(popover_border)
                                            .text_color(popover_text)
                                            .text_size(text_size)
                                            .child(text),
                                    ),
                            )
                        }),
                ),
        };

        let mut scrubber = base
            .group(id.clone())
            .role(gpui::accesskit::Role::Slider)
            .aria_numeric_value(f64::from(fraction))
            .aria_min_numeric_value(0.)
            .aria_max_numeric_value(1.)
            .aria_orientation(match vertical {
                true => gpui::accesskit::Orientation::Vertical,
                false => gpui::accesskit::Orientation::Horizontal,
            })
            .when_some(name, |this, name| this.aria_label(name))
            .when_some(value, |this, value| this.aria_value(value))
            .when(enabled && steps, |this| {
                let up = stepped.clone();
                this.aria_numeric_value_step(f64::from(STEP))
                    .on_a11y_action(gpui::accesskit::Action::Increment, move |_, window, cx| {
                        up(STEP, window, cx)
                    })
                    .on_a11y_action(gpui::accesskit::Action::Decrement, move |_, window, cx| {
                        stepped(-STEP, window, cx)
                    })
            })
            .relative()
            .flex()
            .when_else(
                vertical,
                |this| this.justify_center().h_full().w(reach),
                |this| {
                    match edge {
                        true => this.items_start(),
                        false => this.items_center(),
                    }
                    .w_full()
                    .h(reach)
                },
            )
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .when(enabled, |this| {
                this.cursor_pointer()
                    .on_mouse_down(MouseButton::Left, down)
                    .on_drag(Grab(id.clone()), |grab, _, _, cx| cx.new(|_| grab.clone()))
                    .on_drag_move(dragged)
                    .on_mouse_up(MouseButton::Left, released.clone())
                    .on_mouse_up_out(MouseButton::Left, released)
            })
            .child(bar);
        scrubber.style().refine(&overrides);
        scrubber
    }
}

/// What an expressive track draws: the position, how far the handle's centre sits from the
/// ends at either extreme, how thick the track is, its colours and which way it runs.
struct Track {
    fraction: f32,
    inset: Pixels,
    thick: Pixels,
    /// How far the handle and its gaps have grown in, 0 for a resting edge line.
    grow: f32,
    /// How high the played part's wave stands, from 0 to 1, and its phase, for a wavy track.
    wave: Option<(f32, f32)>,
    filled: Hsla,
    empty: Hsla,
    thumb: Hsla,
    vertical: bool,
}

/// The M3 expressive slider: a thick active track and a thick inactive one ending in a stop dot,
/// parted by a tall bar handle with a gap either side. The outer ends are full-round and the
/// ends at the handle small, so the track's two halves read as one shape cut by the handle. A
/// vertical track fills from the bottom up.
fn expressive_track(track: Track, ranges: Vec<(f32, f32, Hsla)>) -> Div {
    let Track {
        fraction,
        inset,
        thick,
        grow,
        wave,
        filled,
        empty,
        thumb,
        vertical,
    } = track;
    let tall = thick * HANDLE_TALL;
    let cut = (CUT + HANDLE / 2.) * grow;
    let end = thick / 2.;
    let off = (thick - STOP) / 2.;
    // `start` is where the value grows from (left, or bottom), `stop` the far end; `across`
    // pins a part to both sides of the track's thickness.
    let start = move |d: Div, at: gpui::DefiniteLength| match vertical {
        true => d.bottom(at),
        false => d.left(at),
    };
    let stop = move |d: Div, at: gpui::DefiniteLength| match vertical {
        true => d.top(at),
        false => d.right(at),
    };
    let across = move |d: Div| match vertical {
        true => d.left_0().right_0(),
        false => d.top_0().bottom_0(),
    };
    let segment = move |d: Div| match vertical {
        true => d.flex_col(),
        false => d,
    };
    let long = move |d: Div| match vertical {
        true => d.w_full(),
        false => d.h_full(),
    };
    let reach = gpui::DefiniteLength::from(Pixels::ZERO - inset);
    let inner = across(start(stop(div().absolute(), inset.into()), inset.into()));
    let active = match wave {
        Some((height, phase)) => long(div().flex_1()).child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| wavy(bounds, thick, height, phase, filled, window),
            )
            .size_full(),
        ),
        None => long(div().flex_1()).bg(filled),
    };
    let active = match vertical {
        true => active.mt(cut).rounded_b(end).rounded_t(NOTCH),
        false => active.mr(cut).rounded_l(end).rounded_r(NOTCH),
    };
    let rest = long(div().relative().flex_1()).bg(empty);
    let rest = match vertical {
        true => rest.mb(cut).rounded_b(NOTCH).rounded_t(end),
        false => rest.ml(cut).rounded_l(NOTCH).rounded_r(end),
    };
    let dot = div()
        .absolute()
        .top(off)
        .size(STOP)
        .rounded_full()
        .bg(filled);
    let dot = match vertical {
        true => dot.left(off),
        false => dot.right(off),
    };
    let handle = start(div().absolute(), relative(fraction))
        .rounded_full()
        .bg(thumb);
    let handle = match vertical {
        true => handle
            .left((thick - tall) / 2.)
            .mb(Pixels::ZERO - HANDLE / 2.)
            .h(HANDLE)
            .w(tall),
        false => handle
            .top((thick - tall) / 2.)
            .ml(Pixels::ZERO - HANDLE / 2.)
            .w(HANDLE)
            .h(tall),
    };
    let outer = match vertical {
        true => div().relative().h_full().w(thick),
        false => div().relative().w_full().h(thick),
    };
    outer.child(
        inner
            .child(
                segment(
                    across(stop(
                        start(div().absolute(), reach),
                        relative(1. - fraction),
                    ))
                    .flex(),
                )
                .child(active),
            )
            .child(
                segment(across(stop(start(div().absolute(), relative(fraction)), reach)).flex())
                    .child(rest.child(dot)),
            )
            .children(ranges.into_iter().map(move |(from, to, color)| {
                let range = across(start(div().absolute(), relative(from))).bg(color);
                match vertical {
                    true => range.h(relative(to - from)),
                    false => range.w(relative(to - from)),
                }
            }))
            .when(grow > 0., |this| this.child(handle)),
    )
}

/// Strokes the played part of a wavy track across `bounds`: a sine `thick` wide whose crests
/// stand `height` of the way to full, shifted by `phase`, with round ends. The wave is pinned to
/// the track's left end, so it travels along the track rather than stretching with it.
fn wavy(
    bounds: Bounds<Pixels>,
    thick: Pixels,
    height: f32,
    phase: f32,
    color: Hsla,
    window: &mut Window,
) {
    let half = thick / 2.;
    let (from, to) = (bounds.left() + half, bounds.right() - half);
    let middle = bounds.center().y;
    let crest = thick * CREST * height;
    let at = |x: Pixels| {
        let turn = (x - bounds.left()) / WAVELENGTH * TAU - phase;
        point(x, middle + crest * turn.sin())
    };
    let mut points = vec![at(from)];
    if to > from {
        let step = WAVELENGTH / PIECES;
        let mut x = from + step;
        while x < to {
            points.push(at(x));
            x += step;
        }
        points.push(at(to));
        let mut path = PathBuilder::stroke(thick);
        path.add_polygon(&points, false);
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    }
    // gpui's stroke has flat ends, so a dot rounds each one.
    for end in [points[0], points[points.len() - 1]] {
        window.paint_quad(
            gpui::fill(Bounds::centered_at(end, gpui::size(thick, thick)), color)
                .corner_radii(half),
        );
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::{ScrubberState, hit, thumb, track};

    const PAD: gpui::Pixels = px(8.);
    const ORIGIN: f32 = 20.;
    const WIDTH: f32 = 300.;

    fn measured() -> ScrubberState {
        let state = ScrubberState::new("test");
        state.bounds.set(Bounds {
            origin: point(px(ORIGIN), px(0.)),
            size: size(px(WIDTH), hit(PAD)),
        });
        state
    }

    #[test]
    fn the_thumb_lands_under_the_pointer() {
        let state = measured();
        let pin = thumb(PAD);
        let travel = px(WIDTH) - pin;

        for step in 0..=100 {
            let fraction = step as f32 / 100.;
            let center = px(ORIGIN) + pin / 2. + travel * fraction;
            let read = state.fraction_at(center, PAD);
            assert!((read - fraction).abs() < 1e-4, "{fraction} read as {read}");
        }
    }

    #[test]
    fn the_ends_clamp() {
        let state = measured();

        assert_eq!(state.fraction_at(px(0.), PAD), 0.);
        assert_eq!(state.fraction_at(px(ORIGIN), PAD), 0.);
        assert_eq!(state.fraction_at(px(ORIGIN + WIDTH), PAD), 1.);
        assert_eq!(state.fraction_at(px(9999.), PAD), 1.);
    }

    #[test]
    fn an_unmeasured_track_reads_zero() {
        let state = ScrubberState::new("test");

        assert_eq!(state.fraction_at(px(50.), PAD), 0.);
    }

    #[test]
    fn the_bubble_shows_exactly_where_the_grab_lands() {
        let state = measured();
        let reach = hit(PAD);
        let x = px(ORIGIN + WIDTH / 2.);

        assert!(reach > track(PAD) * 4.);
        assert!(state.hovered(point(x, px(0.)), PAD).is_some());
        assert!(state.hovered(point(x, reach - px(1.)), PAD).is_some());
        assert!(state.hovered(point(x, px(-1.)), PAD).is_none());
        assert!(state.hovered(point(x, reach), PAD).is_none());
    }
}
