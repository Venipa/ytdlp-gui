use crate::{
    AnyElement, AnyEntity, AnyWeakEntity, App, AvailableSpace, Bounds, ContentMask, Context,
    Element, ElementId, Entity, EntityId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, PaintIndex, Pixels, PrepaintStateIndex, Render, RenderOnce, Size, Style,
    StyleRefinement, TextStyle, WeakEntity,
};
use crate::{Empty, Window};
use anyhow::Result;
use collections::FxHashSet;
use refineable::Refineable;
use std::{
    any::{TypeId, type_name},
    fmt,
    ops::Range,
};

/// A dynamically-typed view handle that can be downcast to a specific `Entity<V>`.
///
/// This is the type-erased counterpart to [`ViewElement`]: it holds an entity plus
/// a function pointer to its render, and is itself a [`View`], so embedding it as an
/// element goes through the same [`ViewElement`] machinery as any other view.
#[derive(Clone, Debug)]
pub struct AnyView {
    entity: AnyEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
    view_type_name: &'static str,
}

impl<V: Render> From<Entity<V>> for AnyView {
    fn from(value: Entity<V>) -> Self {
        AnyView {
            entity: value.into_any(),
            render: any_view::render::<V>,
            view_type_name: type_name::<V>(),
        }
    }
}

impl AnyView {
    /// Embed this view as a cached [`ViewElement`] laid out at `style`.
    ///
    /// The rendered subtree is recycled from the previous frame unless
    /// [Context::notify] was called on the backing entity since it was rendered
    /// (or [Window::refresh] is called, which ignores caching).
    pub fn cached(self, style: StyleRefinement) -> ViewElement<AnyView> {
        self.into_element().cached(style)
    }

    /// Convert this to a weak handle.
    pub fn downgrade(&self) -> AnyWeakView {
        AnyWeakView {
            entity: self.entity.downgrade(),
            render: self.render,
            view_type_name: self.view_type_name,
        }
    }

    /// Convert this to a [Entity] of a specific type.
    /// If this handle does not contain a view of the specified type, returns itself in an `Err` variant.
    pub fn downcast<T: 'static>(self) -> Result<Entity<T>, Self> {
        match self.entity.downcast() {
            Ok(entity) => Ok(entity),
            Err(entity) => Err(Self {
                entity,
                render: self.render,
                view_type_name: self.view_type_name,
            }),
        }
    }

    /// Gets the [TypeId] of the underlying view.
    pub fn entity_type(&self) -> TypeId {
        self.entity.entity_type
    }

    /// The [`EntityId`] of this view.
    pub fn entity_id(&self) -> EntityId {
        self.entity.entity_id()
    }
}

impl PartialEq for AnyView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl Eq for AnyView {}

/// `AnyView` is the type-erased [`View`]: its `render` is a function pointer rather
/// than a concrete type, but it participates in the reactive graph exactly like any
/// other view via [`ViewElement`].
impl View for AnyView {
    fn entity_id(&self) -> Option<EntityId> {
        Some(self.entity.entity_id())
    }

    #[cfg(not(gpui_hot_reload))]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        (self.render)(&self, window, cx)
    }

    // The stored function pointer was captured from the original binary, so it must be looked up
    // in the jump table to reach the patched render.
    #[cfg(gpui_hot_reload)]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        subsecond::HotFn::current(self.render).call((&self, window, cx))
    }
}

impl<V: 'static + Render> IntoElement for Entity<V> {
    type Element = ViewElement<Entity<V>>;

    fn into_element(self) -> Self::Element {
        ViewElement::with_type_name(self, type_name::<V>())
    }

    #[inline(never)]
    fn into_any_element(self) -> AnyElement {
        self.into_element().into_any()
    }
}

impl IntoElement for AnyView {
    type Element = ViewElement<AnyView>;

    fn into_element(self) -> Self::Element {
        let view_type_name = self.view_type_name;
        ViewElement::with_type_name(self, view_type_name)
    }
}

/// A weak, dynamically-typed view handle.
pub struct AnyWeakView {
    entity: AnyWeakEntity,
    render: fn(&AnyView, &mut Window, &mut App) -> AnyElement,
    view_type_name: &'static str,
}

impl AnyWeakView {
    /// Upgrade to a strong `AnyView` handle, if the view is still alive.
    pub fn upgrade(&self) -> Option<AnyView> {
        let entity = self.entity.upgrade()?;
        Some(AnyView {
            entity,
            render: self.render,
            view_type_name: self.view_type_name,
        })
    }
}

impl<V: 'static + Render> From<WeakEntity<V>> for AnyWeakView {
    fn from(view: WeakEntity<V>) -> Self {
        AnyWeakView {
            entity: view.into(),
            render: any_view::render::<V>,
            view_type_name: type_name::<V>(),
        }
    }
}

impl PartialEq for AnyWeakView {
    fn eq(&self, other: &Self) -> bool {
        self.entity == other.entity
    }
}

impl std::fmt::Debug for AnyWeakView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnyWeakView")
            .field("entity_id", &self.entity.entity_id)
            .finish_non_exhaustive()
    }
}

mod any_view {
    use crate::{AnyElement, AnyView, App, IntoElement, Render, Window};

    pub(crate) fn render<V: 'static + Render>(
        view: &AnyView,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let view = view.clone().downcast::<V>().unwrap();
        // Record the view's Render type name so the accessibility debug dump can
        // attribute nodes to the view that produced them.
        #[cfg(debug_assertions)]
        window
            .a11y
            .view_type_names
            .insert(view.entity_id(), std::any::type_name::<V>());
        view.update(cx, |view, cx| view.render(window, cx).into_any_element())
    }
}

/// A renderable that participates in GPUI's reactive graph — the unifying model
/// behind [`Render`] and [`RenderOnce`].
///
/// When `entity_id()` returns `Some`, that id becomes the view's identity: it gets
/// a unique element-id space (so internal `use_state` / `.id(..)` never collide
/// across siblings) and `cx.notify()` on that entity re-renders only this view's
/// subtree. `None` behaves like a stateless component.
///
/// You rarely implement `View` directly. `Entity<T: Render>` and any `T: RenderOnce`
/// get a blanket impl below; implement it by hand only when a component needs both
/// parent-supplied props *and* a backing entity for identity.
pub trait View: 'static + Sized {
    /// This view's identity, if it has one. A view typically holds the backing
    /// entity as a field and returns its [`EntityId`] here.
    ///
    /// The id becomes this view's [`ElementId`], so two views keyed on the same
    /// entity must not be rendered at the same position in the element tree
    /// (e.g. as siblings under the same parent): their internal element state
    /// (`use_state`, scroll offsets, etc.) would silently collide. Nesting is
    /// fine — the id is scoped by the parent path.
    fn entity_id(&self) -> Option<EntityId>;

    /// Render this view into an element tree, consuming `self`.
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement;
}

/// A stateless component (`RenderOnce`) is a `View` with no identity.
impl<T: RenderOnce> View for T {
    fn entity_id(&self) -> Option<EntityId> {
        None
    }

    #[inline]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        RenderOnce::render(self, window, cx)
    }
}

/// An entity that renders itself (`Render`) is a `View` keyed on its own id.
impl<T: Render> View for Entity<T> {
    fn entity_id(&self) -> Option<EntityId> {
        Some(Entity::entity_id(self))
    }

    #[inline]
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.update(cx, |this, cx| {
            Render::render(this, window, cx).into_any_element()
        })
    }
}

impl<T: Render> Entity<T> {
    /// Embed this entity as a cached [`ViewElement`] laid out at `style`.
    ///
    /// The rendered subtree is reused until the entity is notified (or the
    /// cached bounds / text style change). Caching requires a definite size:
    /// a cached view is laid out from `style` and is *not* measured from its
    /// contents. Use [`ViewElement::new`] (or `.child(entity)`) for the
    /// uncached case.
    #[track_caller]
    pub fn cached(self, style: StyleRefinement) -> ViewElement<Entity<T>> {
        ViewElement::with_type_name(self, type_name::<T>()).cached(style)
    }
}

/// The element type for [`View`] implementations. Wraps a `View` and hooks it
/// into layout, prepaint, and paint. Constructed via [`ViewElement::new`].
#[doc(hidden)]
pub struct ViewElement<V: View> {
    view: Option<V>,
    entity_id: Option<EntityId>,
    cached_style: Option<StyleRefinement>,
    #[cfg(feature = "profiler")]
    view_type_name: &'static str,
    #[cfg(debug_assertions)]
    source: &'static core::panic::Location<'static>,
}

impl<V: View> ViewElement<V> {
    /// Wrap a [`View`] as an element.
    #[track_caller]
    pub fn new(view: V) -> Self {
        Self::with_type_name(view, type_name::<V>())
    }

    /// Wraps a view whose draw time profiling attributes to `view_type_name`,
    /// such as an entity's type rather than [`Entity`]'s.
    #[track_caller]
    #[cfg_attr(not(feature = "profiler"), expect(unused_variables))]
    pub(crate) fn with_type_name(view: V, view_type_name: &'static str) -> Self {
        let entity_id = view.entity_id();
        ViewElement {
            entity_id,
            cached_style: None,
            #[cfg(feature = "profiler")]
            view_type_name,
            view: Some(view),
            #[cfg(debug_assertions)]
            source: core::panic::Location::caller(),
        }
    }

    /// Enable caching of this view's rendered subtree, laid out at `style`.
    /// The composer supplies the layout style because caching skips rendering
    /// the contents to measure them.
    ///
    /// Crate-private on purpose: caching is only sound for entity-backed views,
    /// where [`Context::notify`] is the contract that busts the cache. A stateless
    /// view has no such contract, so a frozen subtree could never be invalidated.
    /// Reach this through [`Entity::cached`] or [`AnyView::cached`], which are
    /// entity-backed by construction.
    pub(crate) fn cached(mut self, style: StyleRefinement) -> Self {
        self.cached_style = Some(style);
        self
    }
}

impl<V: View> IntoElement for ViewElement<V> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct ViewElementState {
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
    /// The frame the ranges were recorded on, from [`Window::drawn_frames`]. They index the
    /// frame drawn last only when that frame recorded them. A view whose cached ancestor was
    /// replayed whole last frame was not visited then, so its ranges are older and it renders.
    frame: u64,
    /// The frame `paint_range` was recorded on, `None` before the view first paints. A view can
    /// prepaint on a frame that does not paint it, which leaves its paint range older than its
    /// prepaint range, so a replay needs both from the frame drawn last.
    painted: Option<u64>,
    /// The inherited opacity the view last painted under. Painting multiplies colours by it,
    /// so a replay under another one shows the old colours, and the view renders next frame.
    /// It is only known at paint, after prepaint chose to replay.
    opacity: f32,
    cache_key: ViewElementCacheKey,
    accessed_entities: FxHashSet<EntityId>,
}

struct ViewElementCacheKey {
    bounds: Bounds<Pixels>,
    content_mask: ContentMask<Pixels>,
    text_style: TextStyle,
    /// What a rem is inside the view, which a parent can change without moving its bounds.
    rem_size: Pixels,
}

#[cfg(not(gpui_hot_reload))]
#[inline]
fn render_view<V: View>(view: V, window: &mut Window, cx: &mut App) -> AnyElement {
    view.render(window, cx).into_any_element()
}

/// Every view and component render passes through this hot-patchable boundary, so the
/// patched code for that render and everything it calls runs on the next frame.
#[cfg(gpui_hot_reload)]
fn render_view<V: View>(view: V, window: &mut Window, cx: &mut App) -> AnyElement {
    #[inline(never)]
    fn render_view_impl<V: View>(view: V, window: &mut Window, cx: &mut App) -> AnyElement {
        view.render(window, cx).into_any_element()
    }

    subsecond::HotFn::current(render_view_impl::<V>).call((view, window, cx))
}

impl<V: View> Element for ViewElement<V> {
    type RequestLayoutState = Option<AnyElement>;
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        self.entity_id.map(ElementId::View)
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        #[cfg(debug_assertions)]
        return Some(self.source);

        #[cfg(not(debug_assertions))]
        return None;
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        if let Some(entity_id) = self.entity_id {
            // Stateful path: create a reactive boundary.
            #[cfg(feature = "profiler")]
            let start = window.window_profiler.begin_view();
            let view = &mut self.view;
            let layout = request_layout_view(
                entity_id,
                self.cached_style.as_ref(),
                window,
                cx,
                &mut |window, cx| render_view(view.take().unwrap(), window, cx),
            );
            #[cfg(feature = "profiler")]
            window.window_profiler.end_view(
                start,
                self.view_type_name,
                crate::profiler::ViewPhase::RequestLayout,
            );
            layout
        } else {
            // Stateless path: isolate subtree via type name (no entity identity).
            request_layout_component(type_name::<V>(), window, cx, &mut |window, cx| {
                render_view(self.view.take().unwrap(), window, cx)
            })
        }
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        if let Some(entity_id) = self.entity_id {
            // Stateful path.
            #[cfg(feature = "profiler")]
            let start = window.window_profiler.begin_view();
            let prepainted = prepaint_view(
                entity_id,
                global_id,
                bounds,
                element,
                window,
                cx,
                &mut |window, cx| render_view(self.view.take().unwrap(), window, cx),
            );
            #[cfg(feature = "profiler")]
            window.window_profiler.end_view(
                start,
                self.view_type_name,
                crate::profiler::ViewPhase::Prepaint,
            );
            prepainted
        } else {
            // Stateless path: just prepaint the element.
            prepaint_component(type_name::<V>(), element, window, cx)
        }
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        element: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(entity_id) = self.entity_id {
            // Stateful path.
            #[cfg(feature = "profiler")]
            let start = window.window_profiler.begin_view();
            paint_view(
                entity_id,
                self.cached_style.is_some(),
                global_id,
                element,
                window,
                cx,
            );
            #[cfg(feature = "profiler")]
            window.window_profiler.end_view(
                start,
                self.view_type_name,
                crate::profiler::ViewPhase::Paint,
            );
        } else {
            // Stateless path: just paint the element.
            paint_component(std::any::type_name::<V>(), element, window, cx);
        }
    }
}

/// A view that renders nothing
pub struct EmptyView;

impl Render for EmptyView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[inline(never)]
fn request_layout_view(
    entity_id: EntityId,
    cached_style: Option<&StyleRefinement>,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> (LayoutId, Option<AnyElement>) {
    window.with_rendered_view(entity_id, |window| {
        let caching_disabled = window.is_inspector_picking(cx);
        match cached_style {
            Some(style) if !caching_disabled => {
                let mut root_style = Style::default();
                root_style.refine(style);
                let layout_id = window.request_layout(root_style, None, cx);
                (layout_id, None)
            }
            _ => {
                let mut element = render(window, cx);
                let layout_id = element.request_layout(window, cx);
                (layout_id, Some(element))
            }
        }
    })
}

#[inline(never)]
fn request_layout_component(
    name: &'static str,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> (LayoutId, Option<AnyElement>) {
    window.with_id(ElementId::from(name), |window| {
        let mut element = render(window, cx);
        let layout_id = element.request_layout(window, cx);
        (layout_id, Some(element))
    })
}

#[inline(never)]
fn prepaint_view(
    entity_id: EntityId,
    global_id: Option<&GlobalElementId>,
    bounds: Bounds<Pixels>,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
    render: &mut dyn FnMut(&mut Window, &mut App) -> AnyElement,
) -> Option<AnyElement> {
    window.set_view_id(entity_id);
    window.with_rendered_view(entity_id, |window| {
        if let Some(mut element) = element.take() {
            window.note_notified(entity_id, bounds);
            element.prepaint(window, cx);
            return Some(element);
        }

        window.with_element_state::<ViewElementState, _>(
            global_id.unwrap(),
            |element_state, window| {
                let content_mask = window.content_mask();
                let text_style = window.text_style();
                let rem_size = window.rem_size();
                let last_frame = |frame: u64| frame + 1 == window.drawn_frames;

                // A replay does not restore the accessibility nodes the view's elements push
                // while they prepaint, and the tree is built anew each frame, so a screen reader
                // would lose the view.
                if let Some(mut element_state) = element_state
                    && element_state.cache_key.bounds == bounds
                    && element_state.cache_key.content_mask == content_mask
                    && element_state.cache_key.text_style == text_style
                    && element_state.cache_key.rem_size == rem_size
                    && !window.dirty_views.contains(&entity_id)
                    && !window.refreshing
                    && !window.a11y.is_active()
                    && last_frame(element_state.frame)
                    && element_state.painted.is_some_and(last_frame)
                {
                    let prepaint_start = window.prepaint_index();
                    window.note_view_cache(true);
                    window.reuse_prepaint(element_state.prepaint_range.clone());
                    cx.entities
                        .extend_accessed(&element_state.accessed_entities);
                    let prepaint_end = window.prepaint_index();
                    element_state.prepaint_range = prepaint_start..prepaint_end;
                    element_state.frame = window.drawn_frames;

                    return (None, element_state);
                }

                // A cached view nested in this one replays what it recorded last frame, which
                // its frame stamp vouches for, so rendering this view leaves the others alone.
                window.note_view_cache(false);
                window.note_notified(entity_id, bounds);
                window.note_repaint(entity_id, bounds);
                let prepaint_start = window.prepaint_index();
                let (element, accessed_entities) = cx.detect_accessed_entities(|cx| {
                    let mut element = render(window, cx);
                    element.layout_as_root(Size::<AvailableSpace>::from(bounds.size), window, cx);
                    element.prepaint_at(bounds.origin, window, cx);
                    element
                });

                let prepaint_end = window.prepaint_index();

                (
                    Some(element),
                    ViewElementState {
                        accessed_entities,
                        prepaint_range: prepaint_start..prepaint_end,
                        paint_range: PaintIndex::default()..PaintIndex::default(),
                        frame: window.drawn_frames,
                        painted: None,
                        opacity: window.element_opacity(),
                        cache_key: ViewElementCacheKey {
                            bounds,
                            content_mask,
                            text_style,
                            rem_size,
                        },
                    },
                )
            },
        )
    })
}

#[inline(never)]
fn prepaint_component(
    name: &'static str,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    window.with_id(ElementId::from(name), |window| {
        element.as_mut().unwrap().prepaint(window, cx);
    });
    Some(element.take().unwrap())
}

#[inline(never)]
fn paint_view(
    entity_id: EntityId,
    cached: bool,
    global_id: Option<&GlobalElementId>,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) {
    window.with_rendered_view(entity_id, |window| {
        let caching_disabled = window.is_inspector_picking(cx);
        if cached && !caching_disabled {
            window.with_element_state::<ViewElementState, _>(
                global_id.unwrap(),
                |element_state, window| {
                    let mut element_state = element_state.unwrap();

                    let paint_start = window.paint_index();
                    let opacity = window.element_opacity();

                    if let Some(element) = element {
                        element.paint(window, cx);
                        element_state.opacity = opacity;
                    } else {
                        window.reuse_paint(element_state.paint_range.clone());
                        if element_state.opacity != opacity {
                            window.faded_views.push(entity_id);
                        }
                    }

                    let paint_end = window.paint_index();
                    element_state.paint_range = paint_start..paint_end;
                    element_state.painted = Some(window.drawn_frames);

                    ((), element_state)
                },
            )
        } else {
            element.as_mut().unwrap().paint(window, cx);
        }
    });
}

#[inline(never)]
fn paint_component(
    name: &'static str,
    element: &mut Option<AnyElement>,
    window: &mut Window,
    cx: &mut App,
) {
    window.with_id(ElementId::Name(name.into()), |window| {
        element.as_mut().unwrap().paint(window, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppContext as _, Hsla, InspectorElementId, ParentElement as _, Styled as _, TestAppContext,
        WindowHandle, blue, div, green, px, red, rems, size, util::FluentBuilder as _,
    };
    use std::{cell::Cell, rc::Rc};

    /// A box of one colour that counts how often it renders.
    struct Leaf {
        color: Hsla,
        /// Draws a second box, which moves everything recorded after this view.
        extra: bool,
        renders: Rc<Cell<usize>>,
    }

    impl Render for Leaf {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div()
                .size_full()
                .bg(self.color)
                .child(div().w(rems(1.)).h(px(2.)).bg(red()))
                .when(self.extra, |this| this.child(div().size(px(5.)).bg(red())))
        }
    }

    /// Lays its child out and prepaints it under `rem_size`, and paints it only while not
    /// `hidden`, as an element that measures what it may not show does.
    struct Veil {
        child: AnyElement,
        hidden: bool,
        rem_size: Option<Pixels>,
    }

    impl Element for Veil {
        type RequestLayoutState = ();
        type PrepaintState = ();

        fn id(&self) -> Option<ElementId> {
            None
        }

        fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
            None
        }

        fn request_layout(
            &mut self,
            _: Option<&GlobalElementId>,
            _: Option<&InspectorElementId>,
            window: &mut Window,
            cx: &mut App,
        ) -> (LayoutId, ()) {
            let child = &mut self.child;
            let layout_id =
                window.with_rem_size(self.rem_size, |window| child.request_layout(window, cx));
            (layout_id, ())
        }

        fn prepaint(
            &mut self,
            _: Option<&GlobalElementId>,
            _: Option<&InspectorElementId>,
            _: Bounds<Pixels>,
            _: &mut (),
            window: &mut Window,
            cx: &mut App,
        ) {
            let child = &mut self.child;
            window.with_rem_size(self.rem_size, |window| child.prepaint(window, cx));
        }

        fn paint(
            &mut self,
            _: Option<&GlobalElementId>,
            _: Option<&InspectorElementId>,
            _: Bounds<Pixels>,
            _: &mut (),
            _: &mut (),
            window: &mut Window,
            cx: &mut App,
        ) {
            if self.hidden {
                return;
            }
            let child = &mut self.child;
            window.with_rem_size(self.rem_size, |window| child.paint(window, cx));
        }
    }

    impl IntoElement for Veil {
        type Element = Self;

        fn into_element(self) -> Self {
            self
        }
    }

    fn leaf(color: Hsla, cx: &mut App) -> Entity<Leaf> {
        cx.new(|_| Leaf {
            color,
            extra: false,
            renders: Rc::default(),
        })
    }

    fn cached_box(width: f32) -> StyleRefinement {
        StyleRefinement::default().w(px(width)).h(px(50.))
    }

    /// A cached view holding two cached leaves, as a screen holds its cards.
    struct Pane {
        leaves: [Entity<Leaf>; 2],
        renders: usize,
    }

    impl Render for Pane {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            div().size_full().flex().children(
                self.leaves
                    .iter()
                    .map(|leaf| leaf.clone().cached(cached_box(50.))),
            )
        }
    }

    /// The window root: a cached sibling drawn before the pane, and the pane under an opacity.
    struct Shell {
        before: Entity<Leaf>,
        pane: Entity<Pane>,
        opacity: f32,
        /// Prepaints the pane without painting it.
        hidden: bool,
        rem_size: Option<Pixels>,
    }

    impl Render for Shell {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let pane = div()
                .opacity(self.opacity)
                .child(self.pane.clone().cached(cached_box(100.)));
            div()
                .size_full()
                .flex()
                .child(self.before.clone().cached(cached_box(50.)))
                .child(Veil {
                    child: pane.into_any_element(),
                    hidden: self.hidden,
                    rem_size: self.rem_size,
                })
        }
    }

    struct Drawn {
        window: WindowHandle<Shell>,
        shell: Entity<Shell>,
        leaves: [Entity<Leaf>; 2],
    }

    impl Drawn {
        fn open(cx: &mut TestAppContext) -> Self {
            let window = cx.open_window(size(px(200.), px(50.)), |_, cx| {
                let leaves = [leaf(blue(), cx), leaf(green(), cx)];
                Shell {
                    before: leaf(red(), cx),
                    pane: cx.new(|_| Pane { leaves, renders: 0 }),
                    opacity: 1.,
                    hidden: false,
                    rem_size: None,
                }
            });
            let shell = window.root(cx).expect("the window has a root");
            let leaves = shell.read_with(cx, |shell, cx| shell.pane.read(cx).leaves.clone());
            Self {
                window,
                shell,
                leaves,
            }
        }

        /// Where every quad of the frame drawn last lands, and in what colour. The test
        /// platform draws a window as soon as a notify dirties it, as the real one does on its
        /// next frame, so the frames a test makes are the ones its notifies ask for.
        fn shown(&self, cx: &mut TestAppContext) -> Vec<String> {
            cx.update_window(self.window.into(), |_, window, _| {
                window
                    .rendered_frame
                    .scene
                    .quads
                    .iter()
                    .map(|quad| format!("{:?} {:?}", quad.bounds, quad.background))
                    .collect()
            })
            .expect("the window is open")
        }

        /// What the window shows once every view renders, which any frame has to match.
        fn refreshed(&self, cx: &mut TestAppContext) -> Vec<String> {
            cx.update_window(self.window.into(), |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx);
            })
            .expect("the window is open");
            self.shown(cx)
        }

        fn renders(&self, cx: &mut TestAppContext) -> [usize; 2] {
            self.leaves
                .each_ref()
                .map(|leaf| leaf.read_with(cx, |leaf, _| leaf.renders.get()))
        }
    }

    #[crate::test]
    fn a_rendered_cached_view_replays_the_cached_views_in_it(cx: &mut TestAppContext) {
        let drawn = Drawn::open(cx);
        for _ in 0..3 {
            let before = drawn.renders(cx);
            drawn.leaves[0].update(cx, |_, cx| cx.notify());
            assert_eq!(
                drawn.renders(cx),
                [before[0] + 1, before[1]],
                "only the leaf that notified renders, though the pane around both does"
            );
            assert_eq!(drawn.shown(cx), drawn.refreshed(cx));
        }
    }

    #[crate::test]
    fn a_cached_view_not_visited_last_frame_renders(cx: &mut TestAppContext) {
        let drawn = Drawn::open(cx);

        // The pane replays whole behind a sibling that now records more, so what the leaves
        // recorded two frames ago no longer lines up with the frame drawn last.
        let before = drawn.shell.read_with(cx, |shell, _| shell.before.clone());
        before.update(cx, |leaf, cx| {
            leaf.extra = true;
            cx.notify();
        });
        let replayed = drawn.renders(cx);

        drawn.leaves[0].update(cx, |_, cx| cx.notify());
        assert_eq!(drawn.renders(cx), [replayed[0] + 1, replayed[1] + 1]);
        assert_eq!(drawn.shown(cx), drawn.refreshed(cx));
    }

    #[crate::test]
    fn a_cached_view_replayed_under_a_new_opacity_renders_on_the_next_frame(
        cx: &mut TestAppContext,
    ) {
        let drawn = Drawn::open(cx);
        let pane = drawn.shell.read_with(cx, |shell, _| shell.pane.clone());
        let pane_renders = |cx: &mut TestAppContext| pane.read_with(cx, |pane, _| pane.renders);
        let before = pane_renders(cx);

        drawn.shell.update(cx, |shell, cx| {
            shell.opacity = 0.5;
            cx.notify();
        });
        // The pane replays on the frame the shell renders in, and asks for the next one.
        cx.run_until_parked();
        cx.update_window(drawn.window.into(), |_, window, cx| {
            if window.invalidator.is_dirty() {
                window.draw(cx).clear(cx);
            }
        })
        .expect("the window is open");
        assert_eq!(
            pane_renders(cx),
            before + 1,
            "the pane renders under the new opacity"
        );
        assert_eq!(drawn.shown(cx), drawn.refreshed(cx));
    }

    #[crate::test]
    fn a_cached_view_prepainted_without_painting_renders_when_shown_again(cx: &mut TestAppContext) {
        let drawn = Drawn::open(cx);
        let veil = |hidden: bool, cx: &mut TestAppContext| {
            drawn.shell.update(cx, |shell, cx| {
                shell.hidden = hidden;
                cx.notify();
            });
        };
        // The pane prepaints on the hidden frame, so its prepaint range is new, but its paint
        // range is still the one the frame before recorded.
        veil(true, cx);
        veil(false, cx);
        assert_eq!(drawn.shown(cx), drawn.refreshed(cx));
    }

    #[crate::test]
    fn a_cached_view_under_a_new_rem_size_renders(cx: &mut TestAppContext) {
        let drawn = Drawn::open(cx);
        let shown = drawn.shown(cx);
        drawn.shell.update(cx, |shell, cx| {
            shell.rem_size = Some(px(32.));
            cx.notify();
        });
        let resized = drawn.shown(cx);
        assert_ne!(resized, shown, "a rem wide box inside the pane grows");
        assert_eq!(resized, drawn.refreshed(cx));
    }
}
