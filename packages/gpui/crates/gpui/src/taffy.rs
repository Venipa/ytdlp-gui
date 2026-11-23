use crate::{
    AbsoluteLength, App, Bounds, DefiniteLength, Edges, GridTemplate, Length, Pixels, Point, Size,
    Style, Window, size,
    util::{
        ceil_to_device_pixel, round_half_toward_zero, round_stroke_to_device_pixel,
        round_to_device_pixel,
    },
};
use collections::{FxHashMap, FxHashSet};
use std::{any::Any, collections::hash_map::Entry, fmt::Debug, ops::Range, rc::Rc, sync::LazyLock};
use taffy::{
    TaffyTree, TraversePartialTree as _,
    geometry::{Point as TaffyPoint, Rect as TaffyRect, Size as TaffySize},
    prelude::{max_content, min_content},
    style::AvailableSpace as TaffyAvailableSpace,
    tree::NodeId,
};

#[cfg(feature = "stacker")]
type StackSafe<T> = stacksafe::StackSafe<T>;
#[cfg(not(feature = "stacker"))]
type StackSafe<T> = T;

type MeasureFn =
    dyn FnMut(Size<Option<Pixels>>, Size<AvailableSpace>, &mut Window, &mut App) -> Size<Pixels>;
type NodeMeasureFn = StackSafe<Box<MeasureFn>>;

struct NodeContext {
    measure: NodeMeasureFn,
}

/// Whether layout nodes are kept from one frame to the next. `GPUI_RETAINED_LAYOUT=0` turns it
/// off, and every frame then builds and lays out its tree from nothing.
static RETAIN: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("GPUI_RETAINED_LAYOUT").is_none_or(|value| value != "0"));

/// A layout node kept from one frame to the next, so a frame that asks for the same style and
/// children at the same place leaves it, and what Taffy cached for it, alone.
struct RetainedNode {
    id: LayoutId,
    /// The frame that last asked for the node. A node no request takes for a frame is released.
    claimed: u64,
    /// [`layout_fingerprint`] of the style the node was last given. A request whose style
    /// fingerprints the same leaves the node's style alone without reading it: a Taffy node is
    /// over a kilobyte, and reading it to compare was most of what keeping it cost.
    style: u64,
    /// [`children_fingerprint`] of the children the node was last given, for the same reason.
    children: u64,
    /// Whether the node measures itself.
    measured: bool,
    /// What the element that last measured the node left for the next one to compare itself
    /// against. See [`Window::retained_measure`].
    memo: Option<Rc<dyn Any>>,
}

/// What a request passes for a node that measures itself.
pub(crate) struct Measured {
    measure: Box<MeasureFn>,
    /// See [`RetainedNode::memo`].
    memo: Option<Rc<dyn Any>>,
    /// Whether the element vouches that it measures as the one that left the memo did, so a
    /// node kept for it keeps what Taffy cached.
    unchanged: bool,
}

impl Measured {
    pub(crate) fn new(
        measure: impl FnMut(
            Size<Option<Pixels>>,
            Size<AvailableSpace>,
            &mut Window,
            &mut App,
        ) -> Size<Pixels>
        + 'static,
    ) -> Self {
        Self {
            measure: Box::new(measure),
            memo: None,
            unchanged: false,
        }
    }

    /// Leaves `memo` with the node for the next frame, and when `unchanged` keeps what Taffy
    /// cached for a node that held a memo before.
    pub(crate) fn remembered(mut self, memo: Rc<dyn Any>, unchanged: bool) -> Self {
        self.memo = Some(memo);
        self.unchanged = unchanged;
        self
    }
}

pub struct TaffyLayoutEngine {
    taffy: TaffyTree<NodeContext>,
    /// The nodes kept across frames, by the key of the place that asks for them.
    retained: FxHashMap<u64, RetainedNode>,
    /// Nodes made this frame for a request with no key, or whose key another request had
    /// taken. They are released when the frame ends.
    transient: Vec<LayoutId>,
    /// Counts frames, to tell the nodes this frame asked for from the ones it did not.
    frame: u64,
    /// Nodes whose style [`Self::stretch_auto_size_to_fill`] rewrote, with the axes it
    /// stretched. Taffy holds the stretched style, which a request whose fingerprint matches
    /// leaves in place, so a window root that keeps its size is not written to twice a frame.
    stretched: FxHashMap<LayoutId, Size<bool>>,
    /// Kept nodes that lost a child outside their own request, when [`Self::compute_layout`]
    /// laid that child out as a root, so their children fingerprint no longer holds.
    uprooted: FxHashSet<LayoutId>,
    retain: bool,
    /// How many nodes the frame being drawn made or wrote to, which is what lays a tree out
    /// again. Tests read it to check that an unchanged frame leaves every node alone.
    #[cfg(test)]
    pub(crate) writes: usize,
    absolute_layout_bounds: FxHashMap<LayoutId, Bounds<Pixels>>,
    /// Unrounded absolute border-box top-left per-node coordinate in device pixels.
    absolute_outer_origins: FxHashMap<LayoutId, Point<f32>>,
    computed_layouts: FxHashSet<LayoutId>,
    layout_bounds_scratch_space: Vec<LayoutId>,
    /// The nodes whose style chose on or off the device pixel grid, `false` for off.
    pixel_snap: FxHashMap<LayoutId, bool>,
    /// For each node laid out off the grid, how far its subtree was shifted as one so the
    /// outermost such node's origin lands on a device pixel.
    grid_shifts: FxHashMap<LayoutId, Point<f32>>,
}

const EXPECT_MESSAGE: &str = "we should avoid taffy layout errors by construction if possible";

impl TaffyLayoutEngine {
    pub fn new() -> Self {
        let mut taffy = TaffyTree::new();
        taffy.disable_rounding();
        TaffyLayoutEngine {
            taffy,
            retained: FxHashMap::default(),
            transient: Vec::new(),
            frame: 0,
            stretched: FxHashMap::default(),
            uprooted: FxHashSet::default(),
            retain: *RETAIN,
            #[cfg(test)]
            writes: 0,
            absolute_layout_bounds: FxHashMap::default(),
            absolute_outer_origins: FxHashMap::default(),
            computed_layouts: FxHashSet::default(),
            layout_bounds_scratch_space: Vec::new(),
            pixel_snap: FxHashMap::default(),
            grid_shifts: FxHashMap::default(),
        }
    }

    pub fn clear(&mut self) {
        match self.retain {
            true => self.release_unclaimed(),
            false => self.taffy.clear(),
        }
        self.absolute_layout_bounds.clear();
        self.absolute_outer_origins.clear();
        self.computed_layouts.clear();
        self.pixel_snap.clear();
        self.grid_shifts.clear();
    }

    /// Releases every node no request took this frame, and starts the next frame.
    fn release_unclaimed(&mut self) {
        let frame = self.frame;
        let taffy = &mut self.taffy;
        let stretched = &mut self.stretched;
        let uprooted = &mut self.uprooted;
        for id in self.transient.drain(..) {
            Self::release(taffy, stretched, uprooted, id);
        }
        self.retained.retain(|_, node| {
            let keep = node.claimed == frame;
            if !keep {
                Self::release(taffy, stretched, uprooted, node.id);
            }
            keep
        });
        self.frame += 1;
    }

    /// Removes a node. A parent that outlives it was laid out around it, so it is laid out
    /// again.
    fn release(
        taffy: &mut TaffyTree<NodeContext>,
        stretched: &mut FxHashMap<LayoutId, Size<bool>>,
        uprooted: &mut FxHashSet<LayoutId>,
        id: LayoutId,
    ) {
        stretched.remove(&id);
        uprooted.remove(&id);
        if let Some(parent) = taffy.parent(id.0) {
            taffy.mark_dirty(parent).expect(EXPECT_MESSAGE);
        }
        // Taffy keeps a removed node's context until its slot is reused, and a measured one
        // holds its measure function and the text layouts it captured.
        taffy.set_node_context(id.0, None).expect(EXPECT_MESSAGE);
        taffy.remove(id.0).expect(EXPECT_MESSAGE);
    }

    /// What the element that last measured the node kept under `key` left with it, while no
    /// request has taken that node this frame.
    pub(crate) fn retained_memo(&self, key: u64) -> Option<Rc<dyn Any>> {
        let node = self.retained.get(&key)?;
        (node.claimed != self.frame).then(|| node.memo.clone())?
    }

    /// The node for a request: the one kept under `key` with only what differs written to it,
    /// or a new one. Taffy lays a node out again when its style, children or measure function
    /// is written, so a request that matches the last frame's costs the comparison alone.
    fn retained_layout(
        &mut self,
        key: Option<u64>,
        fingerprint: u64,
        style: impl Fn() -> taffy::style::Style,
        children: &[LayoutId],
        measured: Option<Measured>,
    ) -> LayoutId {
        let frame = self.frame;
        let taffy = &mut self.taffy;
        let stretched = &mut self.stretched;
        let children_print = children_fingerprint(children);
        let mut writes = 0;

        // The node, whether it was kept from an earlier frame, and the record a node with a key
        // of its own is kept by.
        let (id, kept, record) = match key.map(|key| self.retained.entry(key)) {
            Some(Entry::Occupied(entry)) if entry.get().claimed != frame => {
                let node = entry.into_mut();
                node.claimed = frame;
                let same = node.style == fingerprint;
                debug_assert!(
                    !same
                        || unstretched(
                            taffy.style(node.id.0).expect(EXPECT_MESSAGE),
                            stretched.get(&node.id)
                        ) == style(),
                    "a style that fingerprints the same converts to the same Taffy style"
                );
                if !same {
                    // The stretch is made again from the new style, if the node is still a root.
                    stretched.remove(&node.id);
                    let style = style();
                    if *taffy.style(node.id.0).expect(EXPECT_MESSAGE) != style {
                        taffy.set_style(node.id.0, style).expect(EXPECT_MESSAGE);
                        writes += 1;
                    }
                    node.style = fingerprint;
                }
                (node.id, true, Some(node))
            }
            Some(Entry::Vacant(entry)) => {
                let id = LayoutId::from(taffy.new_leaf(style()).expect(EXPECT_MESSAGE));
                writes += 1;
                let node = entry.insert(RetainedNode {
                    id,
                    claimed: frame,
                    style: fingerprint,
                    children: children_fingerprint(&[]),
                    measured: false,
                    memo: None,
                });
                (id, false, Some(node))
            }
            Some(Entry::Occupied(_)) | None => {
                let id = LayoutId::from(taffy.new_leaf(style()).expect(EXPECT_MESSAGE));
                writes += 1;
                self.transient.push(id);
                (id, false, None)
            }
        };

        // A node's children change only through its own request, a child only leaves a parent
        // whose next request no longer lists it, and a parent a root was taken from is
        // compared in full, so a fingerprint that matches the last request's matches the
        // children Taffy holds.
        // Almost always empty, and checked for every node a frame asks for, so the hash is
        // skipped while nothing is in it.
        let uprooted = !self.uprooted.is_empty() && self.uprooted.remove(&id);
        let same_children = match &record {
            Some(node) if kept && !uprooted && node.children == children_print => {
                debug_assert!(
                    taffy
                        .child_ids(id.0)
                        .eq(children.iter().map(|child| child.0)),
                    "a node keeps the children its fingerprint stands for"
                );
                true
            }
            _ => taffy
                .child_ids(id.0)
                .eq(children.iter().map(|child| child.0)),
        };
        // `set_children` takes a child away from the parent it had, which `new_with_children`
        // does not, and a kept child may come from any parent.
        if !same_children {
            // A child that was a stretched root last frame still holds the stretch, which its
            // request left in place on the bet that it would stay a root.
            if !stretched.is_empty() {
                for child in children {
                    if let Some(axes) = stretched.remove(child) {
                        let style =
                            unstretched(taffy.style(child.0).expect(EXPECT_MESSAGE), Some(&axes));
                        taffy.set_style(child.0, style).expect(EXPECT_MESSAGE);
                        writes += 1;
                    }
                }
            }
            taffy
                .set_children(id.0, LayoutId::to_taffy_slice(children))
                .expect(EXPECT_MESSAGE);
            writes += 1;
        }
        let mut record = record;
        if let Some(node) = record.as_mut() {
            node.children = children_print;
        }

        let was_measured = record.as_ref().is_some_and(|node| node.measured);
        match measured {
            Some(measured) => {
                #[cfg(feature = "stacker")]
                let measure = StackSafe::new(measured.measure);
                #[cfg(not(feature = "stacker"))]
                let measure = measured.measure;
                let context = NodeContext { measure };
                let unchanged = kept && was_measured && measured.unchanged;
                if let Some(node) = record {
                    node.measured = true;
                    node.memo = measured.memo;
                }
                match unchanged {
                    true => {
                        *taffy
                            .get_node_context_mut(id.0)
                            .expect("a measured node keeps its context") = context;
                    }
                    false => {
                        taffy
                            .set_node_context(id.0, Some(context))
                            .expect(EXPECT_MESSAGE);
                        writes += 1;
                    }
                }
            }
            None if was_measured => {
                if let Some(node) = record {
                    node.measured = false;
                    node.memo = None;
                }
                taffy.set_node_context(id.0, None).expect(EXPECT_MESSAGE);
                writes += 1;
            }
            None => {}
        }

        #[cfg(test)]
        {
            self.writes += writes;
        }
        #[cfg(not(test))]
        let _ = writes;
        id
    }

    /// `key` names the place the request comes from, see [`crate::window::LayoutKeys`].
    pub fn request_layout(
        &mut self,
        key: Option<u64>,
        style: Style,
        rem_size: Pixels,
        scale_factor: f32,
        children: &[LayoutId],
    ) -> LayoutId {
        let snap = style.pixel_snap;
        let grid = Grid::new(scale_factor, snap);

        if self.retain {
            let fingerprint = layout_fingerprint(&style, rem_size, grid);
            let id = self.retained_layout(
                key,
                fingerprint,
                || style.to_taffy(rem_size, grid),
                children,
                None,
            );
            self.note_pixel_snap(id, snap);
            return id;
        }
        let taffy_style = style.to_taffy(rem_size, grid);
        let id = if children.is_empty() {
            self.taffy
                .new_leaf(taffy_style)
                .expect(EXPECT_MESSAGE)
                .into()
        } else {
            self.taffy
                // This is safe because LayoutId is repr(transparent) to taffy::tree::NodeId.
                .new_with_children(taffy_style, LayoutId::to_taffy_slice(children))
                .expect(EXPECT_MESSAGE)
                .into()
        };
        self.note_pixel_snap(id, snap);
        id
    }

    /// Remembers whether `id` chose on or off the device pixel grid, for `layout_bounds`.
    fn note_pixel_snap(&mut self, id: LayoutId, snap: Option<bool>) {
        if let Some(snap) = snap {
            self.pixel_snap.insert(id, snap);
        }
    }

    /// `key` names the place the request comes from, see [`crate::window::LayoutKeys`].
    pub(crate) fn request_measured_layout(
        &mut self,
        key: Option<u64>,
        style: Style,
        rem_size: Pixels,
        scale_factor: f32,
        measured: Measured,
    ) -> LayoutId {
        let snap = style.pixel_snap;
        let grid = Grid::new(scale_factor, snap);

        let id = if self.retain {
            let fingerprint = layout_fingerprint(&style, rem_size, grid);
            self.retained_layout(
                key,
                fingerprint,
                || style.to_taffy(rem_size, grid),
                &[],
                Some(measured),
            )
        } else {
            let taffy_style = style.to_taffy(rem_size, grid);
            #[cfg(feature = "stacker")]
            let measure = StackSafe::new(measured.measure);
            #[cfg(not(feature = "stacker"))]
            let measure = measured.measure;
            self.taffy
                .new_leaf_with_context(taffy_style, NodeContext { measure })
                .expect(EXPECT_MESSAGE)
                .into()
        };
        self.note_pixel_snap(id, snap);
        id
    }

    /// Treats any `auto` dimension of the given node's style as filling `size`.
    ///
    /// This is applied to window roots before layout so they behave like the
    /// root element on the web, which stretches to fill the initial containing
    /// block (the viewport) unless given an explicit size. Explicitly styled
    /// dimensions are preserved.
    pub fn stretch_auto_size_to_fill(
        &mut self,
        id: LayoutId,
        size: Size<Pixels>,
        scale_factor: f32,
    ) {
        let current = self.taffy.style(id.0).expect(EXPECT_MESSAGE);
        // A kept root still holds last frame's stretch, so its authored axes are remembered.
        let axes = self.stretched.get(&id).copied().unwrap_or(Size {
            width: current.size.width.is_auto(),
            height: current.size.height.is_auto(),
        });
        if !axes.width && !axes.height {
            return;
        }
        let mut style = current.clone();
        if axes.width {
            style.size.width =
                taffy::style::Dimension::length(round_to_device_pixel(size.width.0, scale_factor));
        }
        if axes.height {
            style.size.height =
                taffy::style::Dimension::length(round_to_device_pixel(size.height.0, scale_factor));
        }
        if style != *current {
            self.taffy.set_style(id.0, style).expect(EXPECT_MESSAGE);
            #[cfg(test)]
            {
                self.writes += 1;
            }
        }
        if self.retain {
            self.stretched.insert(id, axes);
        }
    }

    // Used to understand performance
    #[allow(dead_code)]
    fn count_all_children(&self, parent: LayoutId) -> anyhow::Result<u32> {
        let mut count = 0;

        for child in self.taffy.children(parent.0)? {
            // Count this child.
            count += 1;

            // Count all of this child's children.
            count += self.count_all_children(LayoutId(child))?
        }

        Ok(count)
    }

    // Used to understand performance
    #[allow(dead_code)]
    fn max_depth(&self, depth: u32, parent: LayoutId) -> anyhow::Result<u32> {
        println!(
            "{parent:?} at depth {depth} has {} children",
            self.taffy.child_count(parent.0)
        );

        let mut max_child_depth = 0;

        for child in self.taffy.children(parent.0)? {
            max_child_depth = std::cmp::max(max_child_depth, self.max_depth(0, LayoutId(child))?);
        }

        Ok(depth + 1 + max_child_depth)
    }

    // Used to understand performance
    #[allow(dead_code)]
    fn get_edges(&self, parent: LayoutId) -> anyhow::Result<Vec<(LayoutId, LayoutId)>> {
        let mut edges = Vec::new();

        for child in self.taffy.children(parent.0)? {
            edges.push((parent, LayoutId(child)));

            edges.extend(self.get_edges(LayoutId(child))?);
        }

        Ok(edges)
    }

    #[cfg_attr(feature = "stacker", stacksafe::stacksafe)]
    pub fn compute_layout(
        &mut self,
        id: LayoutId,
        available_space: Size<AvailableSpace>,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Leaving this here until we have a better instrumentation approach.
        // println!("Laying out {} children", self.count_all_children(id)?);
        // println!("Max layout depth: {}", self.max_depth(0, id)?);

        // Output the edges (branches) of the tree in Mermaid format for visualization.
        // println!("Edges:");
        // for (a, b) in self.get_edges(id)? {
        //     println!("N{} --> N{}", u64::from(a), u64::from(b));
        // }
        //

        // A kept node laid out as a root may have been a child on an earlier frame, and the
        // parent it had then would place it.
        if self.retain
            && let Some(parent) = self.taffy.parent(id.0)
        {
            self.taffy.remove_child(parent, id.0).expect(EXPECT_MESSAGE);
            self.uprooted.insert(LayoutId(parent));
        }

        if !self.computed_layouts.insert(id) {
            let stack = &mut self.layout_bounds_scratch_space;
            stack.push(id);
            while let Some(id) = stack.pop() {
                self.absolute_layout_bounds.remove(&id);
                self.absolute_outer_origins.remove(&id);
                self.grid_shifts.remove(&id);
                stack.extend(
                    self.taffy
                        .children(id.into())
                        .expect(EXPECT_MESSAGE)
                        .into_iter()
                        .map(LayoutId::from),
                );
            }
        }

        let scale_factor = window.scale_factor();

        let transform = |v: AvailableSpace| match v {
            AvailableSpace::Definite(pixels) => {
                AvailableSpace::Definite(Pixels(pixels.0 * scale_factor))
            }
            AvailableSpace::MinContent => AvailableSpace::MinContent,
            AvailableSpace::MaxContent => AvailableSpace::MaxContent,
        };
        let available_space = size(
            transform(available_space.width),
            transform(available_space.height),
        );

        self.taffy
            .compute_layout_with_measure(
                id.into(),
                available_space.into(),
                |known_dimensions, available_space, _id, node_context, _style| {
                    let Some(node_context) = node_context else {
                        return taffy::geometry::Size::default();
                    };

                    let known_dimensions = Size {
                        width: known_dimensions.width.map(|e| Pixels(e / scale_factor)),
                        height: known_dimensions.height.map(|e| Pixels(e / scale_factor)),
                    };

                    let available_space: Size<AvailableSpace> = available_space.into();
                    let untransform = |ev: AvailableSpace| match ev {
                        AvailableSpace::Definite(pixels) => {
                            AvailableSpace::Definite(Pixels(pixels.0 / scale_factor))
                        }
                        AvailableSpace::MinContent => AvailableSpace::MinContent,
                        AvailableSpace::MaxContent => AvailableSpace::MaxContent,
                    };
                    let available_space = size(
                        untransform(available_space.width),
                        untransform(available_space.height),
                    );

                    let measured_size: Size<Pixels> =
                        (node_context.measure)(known_dimensions, available_space, window, cx);
                    snap_measured_size_to_device_pixels(measured_size, scale_factor).into()
                },
            )
            .expect(EXPECT_MESSAGE);
    }

    // Pixel snapping
    //
    // Painting primitives at non-integer pixel coordinates produces blurry
    // output. Pixel snapping converts layout coordinates into integer
    // device-pixel coordinates so painted edges land exactly on physical
    // pixel boundaries.
    //
    // Non-integer coordinates can arise for several reasons, including:
    //   - flex distribution, percentages, centering, and text measurement
    //     can produce fractional element sizes and positions;
    //   - at fractional scale factors (for example 125% or 150%), integer
    //     logical-pixel values can map to non-integer device-pixel values.
    //
    // We pixel-snap by rounding in device-pixel space, after multiplying
    // by `scale_factor`, so that snapping targets physical pixels. Bounds
    // are divided by `scale_factor` before being returned to GPUI.
    //
    // Midpoints are rounded toward zero. This is a stylistic choice: a
    // 1-logical-pixel line at 150% scale should render as 1 dp rather than
    // 2 dp.
    //
    // Pixel snapping is done in two phases:
    //
    //  1. Pre-layout metric snapping. Before Taffy computes layout, all
    //     authored absolute lengths are rounded in `to_taffy`. This
    //     includes borders, padding, gaps, and explicit sizes.
    //     Custom-measured leaf nodes have their measured sizes rounded up
    //     to integer device-pixel lengths.
    //
    //  2. Post-layout edge snapping. After Taffy resolves the tree, layout
    //     relationships such as flex shares, grid tracks, percentages, and
    //     centering can produce new fractional edge positions. Boxes now
    //     have edges in absolute coordinates, and snapping must decide
    //     where those edges land on the device-pixel grid.
    //
    // Ideally, post-layout snapping would satisfy:
    //
    //  - Edge closure. Two raw layout edges at the same absolute position
    //    should snap to the same pixel column.
    //  - Translation stability. A component's internal geometry should not
    //    change when it moves to a new absolute position.
    //
    // These goals are in tension because rounding is not associative.
    // The simple local schemes make different tradeoffs:
    //
    //  - Absolute edge rounding gives each window coordinate one answer,
    //    so coincident edges always close globally. But a span's snapped
    //    length is `round(far) - round(near)`, which may change by 1 dp
    //    as its absolute origin moves.
    //
    //  - Parent-relative edge rounding rounds each child inside its
    //    parent's coordinate space. This guarantees translation stability,
    //    but a shared edge reached through different parents can
    //    accumulate different rounding, causing non-closure between
    //    cousins.
    //
    //  - Length rounding rounds each width, height, and thickness
    //    independently and then places boxes from those rounded lengths.
    //    Sizes stay stable under translation, but neighboring boxes derive
    //    their shared boundary from different sources, so closure is not
    //    guaranteed.
    //
    // We apply absolute edge rounding for each element's outer box in
    // post-layout rounding to preserve closure. Border and padding widths
    // are not touched by post-layout rounding; they keep their pre-layout
    // rounded value so that they remain stable under translation.
    //
    // A subtree may opt out (`Styled::pixel_snap(false)`), for layout that
    // animates. Its root keeps its own authored lengths unrounded, and every
    // node inside keeps its unrounded edges, shifted as one by however far
    // the root's origin had to move to land on a device pixel. Whole-pixel
    // metrics inside such a subtree therefore still land on the grid.
    //
    // This gives both closure and translation stability in the case that
    // all local metrics are integer device-pixel lengths. Pre-layout
    // rounding covers that in most cases. The exception is metrics
    // resolved by layout relationships, such as percentages. Outer box
    // edges will still close globally, and painted border widths are still
    // snapped independently, but the raw content-box origin can carry a
    // 1dp residual into descendants.

    pub fn layout_bounds(&mut self, id: LayoutId, scale_factor: f32) -> Bounds<Pixels> {
        if let Some(layout) = self.absolute_layout_bounds.get(&id).cloned() {
            return layout;
        }

        let layout = self.taffy.layout(id.into()).expect(EXPECT_MESSAGE);
        let layout_location = layout.location;
        let layout_size = layout.size;
        let parent = self.taffy.parent(id.0);

        let absolute_outer_origin = match parent {
            Some(parent_id) => {
                let parent_id = LayoutId::from(parent_id);
                self.layout_bounds(parent_id, scale_factor);
                let parent_origin = *self
                    .absolute_outer_origins
                    .get(&parent_id)
                    .expect("parent absolute outer origin should be cached");
                parent_origin + Point::from(layout_location)
            }
            None => Point::from(layout_location),
        };
        self.absolute_outer_origins
            .insert(id, absolute_outer_origin);

        let absolute_far = absolute_outer_origin + Point::from(Size::from(layout_size));
        let parent_shift =
            parent.and_then(|parent_id| self.grid_shifts.get(&LayoutId::from(parent_id)).copied());
        let on_grid = match self.pixel_snap.get(&id) {
            Some(snap) => *snap,
            None => parent_shift.is_none(),
        };
        let device_bounds = match on_grid {
            true => Bounds::from_corners(
                absolute_outer_origin.map(round_half_toward_zero),
                absolute_far.map(round_half_toward_zero),
            ),
            false => {
                let shift = parent_shift.unwrap_or_else(|| {
                    absolute_outer_origin.map(round_half_toward_zero) - absolute_outer_origin
                });
                self.grid_shifts.insert(id, shift);
                Bounds::from_corners(absolute_outer_origin + shift, absolute_far + shift)
            }
        };

        let bounds = (device_bounds / scale_factor).map(Pixels);
        self.absolute_layout_bounds.insert(id, bounds);
        bounds
    }
}

/// A unique identifier for a layout node, generated when requesting a layout from Taffy
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[repr(transparent)]
pub struct LayoutId(NodeId);

impl LayoutId {
    fn to_taffy_slice(node_ids: &[Self]) -> &[taffy::NodeId] {
        // SAFETY: LayoutId is repr(transparent) to taffy::tree::NodeId.
        unsafe { std::mem::transmute::<&[LayoutId], &[taffy::NodeId]>(node_ids) }
    }
}

impl std::hash::Hash for LayoutId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        u64::from(self.0).hash(state);
    }
}

impl From<NodeId> for LayoutId {
    fn from(node_id: NodeId) -> Self {
        Self(node_id)
    }
}

impl From<LayoutId> for NodeId {
    fn from(layout_id: LayoutId) -> NodeId {
        layout_id.0
    }
}

fn snap_measured_size_to_device_pixels(size: Size<Pixels>, scale_factor: f32) -> Size<f32> {
    size.map(|d| ceil_to_device_pixel(d.0.max(0.0), scale_factor))
}

fn border_widths_to_taffy(
    widths: &Edges<AbsoluteLength>,
    rem_size: Pixels,
    scale_factor: f32,
) -> TaffyRect<taffy::style::LengthPercentage> {
    let snap = |w: &AbsoluteLength| {
        taffy::style::LengthPercentage::length(round_stroke_to_device_pixel(
            w.to_pixels(rem_size).0,
            scale_factor,
        ))
    };
    TaffyRect {
        top: snap(&widths.top),
        right: snap(&widths.right),
        bottom: snap(&widths.bottom),
        left: snap(&widths.left),
    }
}

/// A hash of everything [`ToTaffy::to_taffy`] reads from a style, and of what it converts
/// lengths with, so two styles that fingerprint the same convert to the same Taffy style. A
/// field `to_taffy` comes to read has to be added here; the debug assertion in
/// [`TaffyLayoutEngine::retained_layout`] catches one that is not.
fn layout_fingerprint(style: &Style, rem_size: Pixels, grid: Grid) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    use std::mem::discriminant;

    fn absolute(hasher: &mut collections::FxHasher, length: &AbsoluteLength) {
        use std::hash::{Hash as _, Hasher as _};
        match length {
            AbsoluteLength::Pixels(pixels) => {
                0u8.hash(hasher);
                hasher.write_u32(pixels.0.to_bits());
            }
            AbsoluteLength::Rems(rems) => {
                1u8.hash(hasher);
                hasher.write_u32(rems.0.to_bits());
            }
        }
    }
    fn definite(hasher: &mut collections::FxHasher, length: &DefiniteLength) {
        use std::hash::{Hash as _, Hasher as _};
        match length {
            DefiniteLength::Absolute(length) => {
                0u8.hash(hasher);
                absolute(hasher, length);
            }
            DefiniteLength::Fraction(fraction) => {
                1u8.hash(hasher);
                hasher.write_u32(fraction.to_bits());
            }
        }
    }
    fn length(hasher: &mut collections::FxHasher, length: &Length) {
        use std::hash::Hash as _;
        match length {
            Length::Definite(length) => {
                0u8.hash(hasher);
                definite(hasher, length);
            }
            Length::Auto => 1u8.hash(hasher),
        }
    }
    fn placement(hasher: &mut collections::FxHasher, placement: &crate::GridPlacement) {
        use std::hash::Hash as _;
        match placement {
            crate::GridPlacement::Line(line) => (0u8, *line).hash(hasher),
            crate::GridPlacement::Span(span) => (1u8, *span).hash(hasher),
            crate::GridPlacement::Auto => 2u8.hash(hasher),
        }
    }

    let mut hasher = collections::FxHasher::default();
    let h = &mut hasher;
    h.write_u32(rem_size.0.to_bits());
    h.write_u32(grid.scale_factor.to_bits());
    grid.snap.hash(h);

    discriminant(&style.display).hash(h);
    discriminant(&style.overflow.x).hash(h);
    discriminant(&style.overflow.y).hash(h);
    absolute(h, &style.scrollbar_width);
    discriminant(&style.position).hash(h);
    for edge in [
        &style.inset.top,
        &style.inset.right,
        &style.inset.bottom,
        &style.inset.left,
    ] {
        length(h, edge);
    }
    for size in [&style.size, &style.min_size, &style.max_size] {
        length(h, &size.width);
        length(h, &size.height);
    }
    style.aspect_ratio.map(f32::to_bits).hash(h);
    for edge in [
        &style.margin.top,
        &style.margin.right,
        &style.margin.bottom,
        &style.margin.left,
    ] {
        length(h, edge);
    }
    for edge in [
        &style.padding.top,
        &style.padding.right,
        &style.padding.bottom,
        &style.padding.left,
    ] {
        definite(h, edge);
    }
    for edge in [
        &style.border_widths.top,
        &style.border_widths.right,
        &style.border_widths.bottom,
        &style.border_widths.left,
    ] {
        absolute(h, edge);
    }
    style.align_items.map(|value| discriminant(&value)).hash(h);
    style.align_self.map(|value| discriminant(&value)).hash(h);
    style
        .align_content
        .map(|value| discriminant(&value))
        .hash(h);
    style
        .justify_content
        .map(|value| discriminant(&value))
        .hash(h);
    definite(h, &style.gap.width);
    definite(h, &style.gap.height);
    discriminant(&style.flex_direction).hash(h);
    discriminant(&style.flex_wrap).hash(h);
    length(h, &style.flex_basis);
    h.write_u32(style.flex_grow.to_bits());
    h.write_u32(style.flex_shrink.to_bits());
    for template in [&style.grid_rows, &style.grid_cols] {
        template
            .map(|template| (template.repeat, discriminant(&template.min_size)))
            .hash(h);
    }
    match &style.grid_location {
        Some(location) => {
            1u8.hash(h);
            for line in [
                &location.row.start,
                &location.row.end,
                &location.column.start,
                &location.column.end,
            ] {
                placement(h, line);
            }
        }
        None => 0u8.hash(h),
    }
    hasher.finish()
}

/// `style` with the axes [`TaffyLayoutEngine::stretch_auto_size_to_fill`] stretched set back to
/// `auto`, which is the style the node's request asked for.
fn unstretched(style: &taffy::style::Style, axes: Option<&Size<bool>>) -> taffy::style::Style {
    let mut style = style.clone();
    if let Some(axes) = axes {
        if axes.width {
            style.size.width = taffy::style::Dimension::auto();
        }
        if axes.height {
            style.size.height = taffy::style::Dimension::auto();
        }
    }
    style
}

/// A hash of a node's children in order.
fn children_fingerprint(children: &[LayoutId]) -> u64 {
    use std::hash::Hasher as _;
    let mut hasher = collections::FxHasher::default();
    hasher.write_usize(children.len());
    for child in children {
        hasher.write_u64(u64::from(child.0));
    }
    hasher.finish()
}

/// How authored lengths turn into device pixels: rounded onto the grid, or scaled with their
/// fractions kept for a node that chose to leave it.
#[derive(Clone, Copy)]
struct Grid {
    scale_factor: f32,
    snap: bool,
}

impl Grid {
    fn new(scale_factor: f32, snap: Option<bool>) -> Self {
        Self {
            scale_factor,
            snap: snap != Some(false),
        }
    }

    /// A logical length in device pixels.
    fn place(self, logical: f32) -> f32 {
        match self.snap {
            true => round_to_device_pixel(logical, self.scale_factor),
            false => logical * self.scale_factor,
        }
    }
}

trait ToTaffy<Output> {
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> Output;
}

impl ToTaffy<taffy::style::Style> for Style {
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> taffy::style::Style {
        use taffy::style_helpers::{fr, length, minmax, repeat};

        fn to_grid_line(
            placement: &Range<crate::GridPlacement>,
        ) -> taffy::Line<taffy::GridPlacement> {
            taffy::Line {
                start: placement.start.into(),
                end: placement.end.into(),
            }
        }

        fn to_grid_repeat<T: taffy::style::CheapCloneStr>(
            unit: &Option<GridTemplate>,
        ) -> Vec<taffy::GridTemplateComponent<T>> {
            unit.map(|template| {
                match template.min_size {
                    // grid-template-*: repeat(<number>, minmax(0, 1fr));
                    crate::GridTemplateMinSize::Zero => {
                        vec![repeat(
                            template.repeat,
                            vec![minmax(length(0.0_f32), fr(1.0_f32))],
                        )]
                    }
                    // grid-template-*: repeat(<number>, minmax(min-content, 1fr));
                    crate::GridTemplateMinSize::MinContent => {
                        vec![repeat(
                            template.repeat,
                            vec![minmax(min_content(), fr(1.0_f32))],
                        )]
                    }
                    // grid-template-*: repeat(<number>, minmax(0, max-content))
                    crate::GridTemplateMinSize::MaxContent => {
                        vec![repeat(
                            template.repeat,
                            vec![minmax(length(0.0_f32), max_content())],
                        )]
                    }
                }
            })
            .unwrap_or_default()
        }

        taffy::style::Style {
            display: self.display.into(),
            overflow: self.overflow.into(),
            scrollbar_width: self.scrollbar_width.to_taffy(rem_size, grid),
            position: self.position.into(),
            inset: self.inset.to_taffy(rem_size, grid),
            size: self.size.to_taffy(rem_size, grid),
            min_size: self.min_size.to_taffy(rem_size, grid),
            max_size: self.max_size.to_taffy(rem_size, grid),
            aspect_ratio: self.aspect_ratio,
            margin: self.margin.to_taffy(rem_size, grid),
            padding: self.padding.to_taffy(rem_size, grid),
            border: border_widths_to_taffy(&self.border_widths, rem_size, grid.scale_factor),
            align_items: self.align_items.map(|x| x.into()),
            align_self: self.align_self.map(|x| x.into()),
            align_content: self.align_content.map(|x| x.into()),
            justify_content: self.justify_content.map(|x| x.into()),
            gap: self.gap.to_taffy(rem_size, grid),
            flex_direction: self.flex_direction.into(),
            flex_wrap: self.flex_wrap.into(),
            flex_basis: self.flex_basis.to_taffy(rem_size, grid),
            flex_grow: self.flex_grow,
            flex_shrink: self.flex_shrink,
            grid_template_rows: to_grid_repeat(&self.grid_rows),
            grid_template_columns: to_grid_repeat(&self.grid_cols),
            grid_row: self
                .grid_location
                .as_ref()
                .map(|location| to_grid_line(&location.row))
                .unwrap_or_default(),
            grid_column: self
                .grid_location
                .as_ref()
                .map(|location| to_grid_line(&location.column))
                .unwrap_or_default(),
            ..Default::default()
        }
    }
}

impl ToTaffy<f32> for AbsoluteLength {
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> f32 {
        grid.place(self.to_pixels(rem_size).0)
    }
}

impl ToTaffy<taffy::style::LengthPercentageAuto> for Length {
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> taffy::prelude::LengthPercentageAuto {
        match self {
            Length::Definite(length) => length.to_taffy(rem_size, grid),
            Length::Auto => taffy::prelude::LengthPercentageAuto::auto(),
        }
    }
}

impl ToTaffy<taffy::style::Dimension> for Length {
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> taffy::prelude::Dimension {
        match self {
            Length::Definite(length) => length.to_taffy(rem_size, grid),
            Length::Auto => taffy::prelude::Dimension::auto(),
        }
    }
}

impl ToTaffy<taffy::style::LengthPercentage> for DefiniteLength {
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> taffy::style::LengthPercentage {
        match self {
            DefiniteLength::Absolute(length) => length.to_taffy(rem_size, grid),
            DefiniteLength::Fraction(fraction) => {
                taffy::style::LengthPercentage::percent(*fraction)
            }
        }
    }
}

impl ToTaffy<taffy::style::LengthPercentageAuto> for DefiniteLength {
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> taffy::style::LengthPercentageAuto {
        match self {
            DefiniteLength::Absolute(length) => length.to_taffy(rem_size, grid),
            DefiniteLength::Fraction(fraction) => {
                taffy::style::LengthPercentageAuto::percent(*fraction)
            }
        }
    }
}

impl ToTaffy<taffy::style::Dimension> for DefiniteLength {
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> taffy::style::Dimension {
        match self {
            DefiniteLength::Absolute(length) => length.to_taffy(rem_size, grid),
            DefiniteLength::Fraction(fraction) => taffy::style::Dimension::percent(*fraction),
        }
    }
}

impl ToTaffy<taffy::style::LengthPercentage> for AbsoluteLength {
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> taffy::style::LengthPercentage {
        taffy::style::LengthPercentage::length(self.to_taffy(rem_size, grid))
    }
}

impl ToTaffy<taffy::style::LengthPercentageAuto> for AbsoluteLength {
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> taffy::style::LengthPercentageAuto {
        taffy::style::LengthPercentageAuto::length(self.to_taffy(rem_size, grid))
    }
}

impl ToTaffy<taffy::style::Dimension> for AbsoluteLength {
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> taffy::style::Dimension {
        taffy::style::Dimension::length(self.to_taffy(rem_size, grid))
    }
}

impl<T, T2> From<TaffyPoint<T>> for Point<T2>
where
    T: Into<T2>,
    T2: Clone + Debug + Default + PartialEq,
{
    fn from(point: TaffyPoint<T>) -> Point<T2> {
        Point {
            x: point.x.into(),
            y: point.y.into(),
        }
    }
}

impl<T, T2> From<Point<T>> for TaffyPoint<T2>
where
    T: Into<T2> + Clone + Debug + Default + PartialEq,
{
    fn from(val: Point<T>) -> Self {
        TaffyPoint {
            x: val.x.into(),
            y: val.y.into(),
        }
    }
}

impl<T, U> ToTaffy<TaffySize<U>> for Size<T>
where
    T: ToTaffy<U> + Clone + Debug + Default + PartialEq,
{
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> TaffySize<U> {
        TaffySize {
            width: self.width.to_taffy(rem_size, grid),
            height: self.height.to_taffy(rem_size, grid),
        }
    }
}

impl<T, U> ToTaffy<TaffyRect<U>> for Edges<T>
where
    T: ToTaffy<U> + Clone + Debug + Default + PartialEq,
{
    fn to_taffy(&self, rem_size: Pixels, grid: Grid) -> TaffyRect<U> {
        TaffyRect {
            top: self.top.to_taffy(rem_size, grid),
            right: self.right.to_taffy(rem_size, grid),
            bottom: self.bottom.to_taffy(rem_size, grid),
            left: self.left.to_taffy(rem_size, grid),
        }
    }
}

impl<T, U> From<TaffySize<T>> for Size<U>
where
    T: Into<U>,
    U: Clone + Debug + Default + PartialEq,
{
    fn from(taffy_size: TaffySize<T>) -> Self {
        Size {
            width: taffy_size.width.into(),
            height: taffy_size.height.into(),
        }
    }
}

impl<T, U> From<Size<T>> for TaffySize<U>
where
    T: Into<U> + Clone + Debug + Default + PartialEq,
{
    fn from(size: Size<T>) -> Self {
        TaffySize {
            width: size.width.into(),
            height: size.height.into(),
        }
    }
}

/// The space available for an element to be laid out in
#[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
pub enum AvailableSpace {
    /// The amount of space available is the specified number of pixels
    Definite(Pixels),
    /// The amount of space available is indefinite and the node should be laid out under a min-content constraint
    #[default]
    MinContent,
    /// The amount of space available is indefinite and the node should be laid out under a max-content constraint
    MaxContent,
}

impl AvailableSpace {
    /// Returns a `Size` with both width and height set to `AvailableSpace::MinContent`.
    ///
    /// This function is useful when you want to create a `Size` with the minimum content constraints
    /// for both dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use gpui::AvailableSpace;
    /// let min_content_size = AvailableSpace::min_size();
    /// assert_eq!(min_content_size.width, AvailableSpace::MinContent);
    /// assert_eq!(min_content_size.height, AvailableSpace::MinContent);
    /// ```
    pub const fn min_size() -> Size<Self> {
        Size {
            width: Self::MinContent,
            height: Self::MinContent,
        }
    }
}

impl From<AvailableSpace> for TaffyAvailableSpace {
    fn from(space: AvailableSpace) -> TaffyAvailableSpace {
        match space {
            AvailableSpace::Definite(Pixels(value)) => TaffyAvailableSpace::Definite(value),
            AvailableSpace::MinContent => TaffyAvailableSpace::MinContent,
            AvailableSpace::MaxContent => TaffyAvailableSpace::MaxContent,
        }
    }
}

impl From<TaffyAvailableSpace> for AvailableSpace {
    fn from(space: TaffyAvailableSpace) -> AvailableSpace {
        match space {
            TaffyAvailableSpace::Definite(value) => AvailableSpace::Definite(Pixels(value)),
            TaffyAvailableSpace::MinContent => AvailableSpace::MinContent,
            TaffyAvailableSpace::MaxContent => AvailableSpace::MaxContent,
        }
    }
}

impl From<Pixels> for AvailableSpace {
    fn from(pixels: Pixels) -> Self {
        AvailableSpace::Definite(pixels)
    }
}

impl From<Size<Pixels>> for Size<AvailableSpace> {
    fn from(size: Size<Pixels>) -> Self {
        Size {
            width: AvailableSpace::Definite(size.width),
            height: AvailableSpace::Definite(size.height),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_widths_to_taffy_use_stroke_snapping() {
        let border_widths = Edges {
            top: Pixels(0.0).into(),
            right: Pixels(0.4).into(),
            bottom: Pixels(0.5).into(),
            left: Pixels(1.6).into(),
        };
        let taffy_border = border_widths_to_taffy(&border_widths, Pixels(16.0), 1.0);

        assert_eq!(
            taffy_border.top,
            taffy::style::LengthPercentage::length(0.0)
        );
        assert_eq!(
            taffy_border.right,
            taffy::style::LengthPercentage::length(1.0)
        );
        assert_eq!(
            taffy_border.bottom,
            taffy::style::LengthPercentage::length(1.0)
        );
        assert_eq!(
            taffy_border.left,
            taffy::style::LengthPercentage::length(2.0)
        );
    }

    /// What a test window shows. Each field changes one thing a kept layout node has to
    /// notice, and the test changes one field a frame, so nothing else lays the tree out again
    /// and hides a change that went unnoticed.
    #[derive(Clone, Copy, Default)]
    struct Shown {
        /// Widens the column the title wraps in.
        wide: bool,
        /// Swaps the title for a longer one.
        long_title: bool,
        /// Takes a box out of layout with `display: none`.
        hidden: bool,
        /// Adds a box without an id between the rows and the filler.
        extra: bool,
        /// Moves the window of rows on by one, so one row leaves and one arrives.
        scrolled: bool,
        /// Changes every row's text without changing its width class.
        relabeled: bool,
        /// Changes the colour of the text, which measures the same.
        recolored: bool,
    }

    impl Shown {
        const FIELDS: usize = 7;

        fn flip(&mut self, field: usize) {
            let field = match field {
                0 => &mut self.wide,
                1 => &mut self.long_title,
                2 => &mut self.hidden,
                3 => &mut self.extra,
                4 => &mut self.scrolled,
                5 => &mut self.relabeled,
                _ => &mut self.recolored,
            };
            *field = !*field;
        }
    }

    impl crate::Render for Shown {
        fn render(
            &mut self,
            _: &mut Window,
            _: &mut crate::Context<Self>,
        ) -> impl crate::IntoElement {
            use crate::{
                InteractiveElement as _, IntoElement as _, ParentElement as _, SharedString,
                Styled as _, blue, div, green, px, red, util::FluentBuilder as _,
            };

            let title = match self.long_title {
                true => "a title long enough to wrap onto more lines when its column narrows",
                false => "a title",
            };
            let first_row = usize::from(self.scrolled);
            let label = usize::from(self.relabeled);
            // Sized by nothing, so the window stretches it to fill on every frame.
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_2()
                .when(self.recolored, |this| this.text_color(red()))
                .child(
                    div()
                        .id("title")
                        .w(px(if self.wide { 300. } else { 120. }))
                        .bg(red())
                        .child(SharedString::from(title)),
                )
                .child(
                    div()
                        .when(self.hidden, |this| this.hidden())
                        .h(px(30.))
                        .bg(green())
                        .child("hidden now and then"),
                )
                .children((first_row..first_row + 5).map(|row| {
                    div()
                        .id(("row", row))
                        .h(px(20.))
                        .flex()
                        .gap_1()
                        .child(div().w(px(10. + row as f32)).h_full().bg(blue()))
                        .child(format!("row {row} labeled {label}"))
                        .into_any_element()
                }))
                .children(self.extra.then(|| div().h(px(15.)).bg(red())))
                .child(div().flex_1().bg(green()).child("fills what is left"))
        }
    }

    /// Where every quad and glyph of the frame a window draws now lands.
    fn painted(window: crate::AnyWindowHandle, cx: &mut crate::TestAppContext) -> Vec<String> {
        use crate::AppContext as _;

        cx.update_window(window, |_, window, cx| {
            let arena_clear_needed = window.draw(cx);
            let scene = &window.rendered_frame.scene;
            let painted = scene
                .quads
                .iter()
                .map(|quad| format!("quad {:?} {:?}", quad.bounds, quad.background))
                .chain(scene.monochrome_sprites.iter().map(|sprite| {
                    format!(
                        "glyph {:?} {:?} {:?}",
                        sprite.bounds, sprite.color, sprite.tile.tile_id
                    )
                }))
                .collect();
            arena_clear_needed.clear(cx);
            painted
        })
        .expect("the window stays open while it is drawn")
    }

    #[crate::test]
    fn kept_layout_nodes_lay_out_as_a_fresh_window_does(cx: &mut crate::TestAppContext) {
        use crate::{AppContext as _, px, size};

        let window_size = size(px(640.), px(480.));
        let kept = cx.open_window(window_size, |_, _| Shown::default());
        let root = kept.root(cx).expect("the window has a root");
        let writes = |cx: &mut crate::TestAppContext| {
            cx.update_window(kept.into(), |_, window, _| window.layout_writes())
                .expect("the kept window is open")
        };
        painted(kept.into(), cx);
        let mut written = writes(cx);
        // With retention turned off every frame builds the tree anew, which only the drawing
        // comparison applies to.
        let retained = *RETAIN;
        // Coprime with the field count, so every field is flipped on and off beside every
        // setting of its neighbours.
        for step in 0..Shown::FIELDS * 12 {
            let field = step * 3 % Shown::FIELDS;
            let shown = root.update(cx, |shown, cx| {
                shown.flip(field);
                cx.notify();
                *shown
            });
            let fresh = cx.open_window(window_size, move |_, _| shown);
            let expected = painted(fresh.into(), cx);
            cx.update_window(fresh.into(), |_, window, _| window.remove_window())
                .expect("the fresh window is open");
            assert!(!expected.is_empty());
            assert_eq!(
                painted(kept.into(), cx),
                expected,
                "after flipping field {field} at step {step}"
            );
            assert!(
                !retained || writes(cx) > written,
                "flipping field {field} writes to the nodes it changes"
            );
            written = writes(cx);
            assert_eq!(
                painted(kept.into(), cx),
                expected,
                "drawn again at step {step}"
            );
            if retained {
                assert_eq!(
                    writes(cx),
                    written,
                    "a frame that changes nothing leaves every node alone, at step {step}"
                );
            }
        }
    }

    #[crate::test]
    fn a_parent_gets_back_a_child_laid_out_as_a_root(cx: &mut crate::TestAppContext) {
        use crate::{px, size};

        let window = cx.open_window(size(px(100.), px(100.)), |_, _| Shown::default());
        window
            .update(cx, |_, window, cx| {
                let mut engine = TaffyLayoutEngine::new();
                engine.retain = true;
                let space = size(px(100.), px(100.)).into();
                let mut request = |engine: &mut TaffyLayoutEngine, key, children: &[LayoutId]| {
                    engine.request_layout(Some(key), Style::default(), px(16.), 1., children)
                };

                let child = request(&mut engine, 1, &[]);
                let parent = request(&mut engine, 2, &[child]);
                engine.compute_layout(parent, space, window, cx);
                engine.clear();

                let kept_child = request(&mut engine, 1, &[]);
                assert_eq!(kept_child, child, "the child is kept");
                engine.compute_layout(child, space, window, cx);
                let kept_parent = request(&mut engine, 2, &[child]);
                engine.compute_layout(kept_parent, space, window, cx);
                assert_eq!(
                    engine.taffy.children(parent.0).expect(EXPECT_MESSAGE),
                    [child.0],
                    "the parent lists its child again"
                );
            })
            .expect("the window is open");
    }

    #[crate::test]
    fn a_stretched_root_turned_child_gets_back_its_auto_size(cx: &mut crate::TestAppContext) {
        use crate::{px, size};

        let window = cx.open_window(size(px(100.), px(100.)), |_, _| Shown::default());
        window
            .update(cx, |_, window, cx| {
                let mut engine = TaffyLayoutEngine::new();
                engine.retain = true;
                let space = size(px(100.), px(100.)).into();

                let root = engine.request_layout(Some(1), Style::default(), px(16.), 1., &[]);
                engine.stretch_auto_size_to_fill(root, size(px(100.), px(100.)), 1.);
                engine.compute_layout(root, space, window, cx);
                engine.clear();

                let child = engine.request_layout(Some(1), Style::default(), px(16.), 1., &[]);
                assert_eq!(child, root, "the node is kept");
                let mut parent_style = Style::default();
                parent_style.size = size(px(50.).into(), px(50.).into());
                let parent = engine.request_layout(Some(2), parent_style, px(16.), 1., &[child]);
                engine.compute_layout(parent, space, window, cx);
                let fresh = Style::default().to_taffy(px(16.), Grid::new(1., None));
                assert_eq!(
                    engine.taffy.style(child.0).expect(EXPECT_MESSAGE).size,
                    fresh.size,
                    "the child is laid out with the size it asked for"
                );
            })
            .expect("the window is open");
    }
}
