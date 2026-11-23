// todo("windows"): remove
#![cfg_attr(windows, allow(dead_code))]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AtlasTextureId, AtlasTile, Background, Bounds, ContentMask, Corners, Edges, Hsla, Pixels,
    Point, Radians, ScaledPixels, ShaderId, Size, bounds_tree::BoundsTree, point,
};
use collections::FxHashMap;
use std::{
    fmt::Debug,
    iter::Peekable,
    ops::{Add, Range, Sub},
    slice,
};

#[allow(non_camel_case_types, unused)]
#[expect(missing_docs)]
pub type PathVertex_ScaledPixels = PathVertex<ScaledPixels>;

#[expect(missing_docs)]
pub type DrawOrder = u32;

/// Names a run of paint operations, so a later frame can find what an element drew and draw it
/// again.
pub type SpanId = u64;

/// Where a named span sits among a scene's paint operations, with the bounds it was given.
struct PaintSpan {
    operations: Range<usize>,
    bounds: Bounds<Pixels>,
}

/// A boolean stored as a `u32` so that GPU-facing structs contain no
/// compiler-inserted padding bytes, which would be undefined behavior to
/// reinterpret as `&[u8]` when writing instance buffers. Guaranteed to be
/// `0` or `1` by construction; shaders read it as a `u32`/`uint`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
#[repr(transparent)]
pub struct PaddedBool32(u32);

impl From<bool> for PaddedBool32 {
    fn from(value: bool) -> Self {
        PaddedBool32(value as u32)
    }
}

#[derive(Default)]
#[expect(missing_docs)]
pub struct Scene {
    pub(crate) paint_operations: Vec<PaintOperation>,
    primitive_bounds: BoundsTree<ScaledPixels>,
    layer_stack: Vec<DrawOrder>,
    spans: FxHashMap<SpanId, PaintSpan>,
    span_stack: Vec<(SpanId, usize, Bounds<Pixels>)>,
    pub shadows: Vec<Shadow>,
    pub backdrops: Vec<Backdrop>,
    pub effects: Vec<LayerEffect>,
    effect_stack: Vec<usize>,
    pub quads: Vec<Quad>,
    pub shaders: Vec<ShaderQuad>,
    pub paths: Vec<Path<ScaledPixels>>,
    pub underlines: Vec<Underline>,
    pub monochrome_sprites: Vec<MonochromeSprite>,
    pub subpixel_sprites: Vec<SubpixelSprite>,
    pub polychrome_sprites: Vec<PolychromeSprite>,
    pub surfaces: Vec<PaintSurface>,
}

#[expect(missing_docs)]
impl Scene {
    pub fn clear(&mut self) {
        self.paint_operations.clear();
        self.primitive_bounds.clear();
        self.layer_stack.clear();
        self.spans.clear();
        self.span_stack.clear();
        self.paths.clear();
        self.shadows.clear();
        self.backdrops.clear();
        self.effects.clear();
        self.effect_stack.clear();
        self.quads.clear();
        self.shaders.clear();
        self.underlines.clear();
        self.monochrome_sprites.clear();
        self.subpixel_sprites.clear();
        self.polychrome_sprites.clear();
        self.surfaces.clear();
    }

    pub fn len(&self) -> usize {
        self.paint_operations.len()
    }

    pub fn push_layer(&mut self, bounds: Bounds<ScaledPixels>) {
        let order = self.primitive_bounds.insert(bounds);
        self.layer_stack.push(order);
        self.paint_operations
            .push(PaintOperation::StartLayer(bounds));
    }

    pub fn pop_layer(&mut self) {
        self.layer_stack.pop();
        self.paint_operations.push(PaintOperation::EndLayer);
    }

    /// Starts a layer that is drawn offscreen, put through the given filter, and composited back.
    ///
    /// The layer claims a range of orders nothing outside it can share: the barriers on either end
    /// keep the rest of the frame out of it, while its contents order among themselves the way
    /// they would anywhere else, so a quad still covers an image painted before it.
    pub fn push_filter(
        &mut self,
        source_bounds: Bounds<ScaledPixels>,
        transform_origin: Point<ScaledPixels>,
        content_mask: Bounds<ScaledPixels>,
        filter: Filter,
    ) {
        let start = self.primitive_bounds.barrier();
        let mut destination_bounds =
            scale_bounds_around(source_bounds, transform_origin, filter.scale);
        destination_bounds.origin += filter.translate;
        self.effects.push(LayerEffect {
            start,
            end: DrawOrder::MAX,
            source_bounds,
            transform_origin,
            destination_bounds,
            content_mask,
            filter,
            parent: self.effect_stack.last().copied(),
        });
        self.effect_stack.push(self.effects.len() - 1);
        self.paint_operations.push(PaintOperation::StartFilter(
            source_bounds,
            transform_origin,
            content_mask,
            filter,
        ));
    }

    pub fn pop_filter(&mut self) {
        if let Some(index) = self.effect_stack.pop() {
            self.effects[index].end = self.primitive_bounds.barrier();
        }
        self.paint_operations.push(PaintOperation::EndFilter);
    }

    /// Starts a named span. Everything painted until [`Scene::pop_span`] can be found again under
    /// `id`, in this scene and in the next one that replays it.
    pub fn push_span(&mut self, id: SpanId, bounds: Bounds<Pixels>) {
        self.paint_operations
            .push(PaintOperation::StartSpan(id, bounds));
        self.span_stack
            .push((id, self.paint_operations.len(), bounds));
    }

    pub fn pop_span(&mut self) {
        if let Some((id, start, bounds)) = self.span_stack.pop() {
            let operations = start..self.paint_operations.len();
            self.spans.insert(id, PaintSpan { operations, bounds });
        }
        self.paint_operations.push(PaintOperation::EndSpan);
    }

    /// The bounds the span `id` was given, when this scene holds one.
    pub fn span(&self, id: SpanId) -> Option<Bounds<Pixels>> {
        self.spans.get(&id).map(|span| span.bounds)
    }

    /// Paints what `prev_scene` painted in its span `id` again, under the same name, with the
    /// alpha of every colour scaled by `fade`. Spans nested in it lose their names. Returns
    /// whether `prev_scene` held the span.
    pub fn replay_span(&mut self, id: SpanId, prev_scene: &Scene, fade: f32) -> bool {
        let Some(span) = prev_scene.spans.get(&id) else {
            return false;
        };
        self.push_span(id, span.bounds);
        // A span pushed by hand can open a layer or a filter it does not close, or close one
        // it did not open. The replay keeps count, so it never closes the caller's and never
        // leaves one of its own open.
        let (mut layers, mut filters) = (0usize, 0usize);
        for operation in &prev_scene.paint_operations[span.operations.clone()] {
            match operation {
                PaintOperation::Primitive(primitive) => {
                    self.insert_primitive(primitive.clone().faded(fade))
                }
                PaintOperation::StartLayer(bounds) => {
                    layers += 1;
                    self.push_layer(*bounds);
                }
                PaintOperation::EndLayer if layers > 0 => {
                    layers -= 1;
                    self.pop_layer();
                }
                PaintOperation::StartFilter(bounds, transform_origin, content_mask, filter) => {
                    filters += 1;
                    self.push_filter(*bounds, *transform_origin, *content_mask, *filter);
                }
                PaintOperation::EndFilter if filters > 0 => {
                    filters -= 1;
                    self.pop_filter();
                }
                PaintOperation::EndLayer
                | PaintOperation::EndFilter
                | PaintOperation::StartSpan(..)
                | PaintOperation::EndSpan => {}
            }
        }
        for _ in 0..filters {
            self.pop_filter();
        }
        for _ in 0..layers {
            self.pop_layer();
        }
        self.pop_span();
        true
    }

    /// The draw order of a batch's first primitive.
    pub fn batch_order(&self, batch: &PrimitiveBatch) -> DrawOrder {
        match batch {
            PrimitiveBatch::Shadows(range) => self.shadows[range.start].order,
            PrimitiveBatch::Backdrops(range) => self.backdrops[range.start].order,
            PrimitiveBatch::Quads(range) => self.quads[range.start].order,
            PrimitiveBatch::Shaders { range, .. } => self.shaders[range.start].order,
            PrimitiveBatch::Paths(range) => self.paths[range.start].order,
            PrimitiveBatch::Underlines(range) => self.underlines[range.start].order,
            PrimitiveBatch::MonochromeSprites { range, .. } => {
                self.monochrome_sprites[range.start].order
            }
            PrimitiveBatch::SubpixelSprites { range, .. } => {
                self.subpixel_sprites[range.start].order
            }
            PrimitiveBatch::PolychromeSprites { range, .. } => {
                self.polychrome_sprites[range.start].order
            }
            PrimitiveBatch::Surfaces(range) => self.surfaces[range.start].order,
        }
    }

    /// The innermost layer a primitive drawn at the given order belongs to, if any.
    ///
    /// Membership is a range rather than an exact order: elements open stacking layers of their
    /// own inside a filtered subtree, and their primitives carry those orders instead.
    pub fn filtered(&self, order: DrawOrder) -> Option<usize> {
        let after = self.effects.partition_point(|layer| layer.start <= order);
        (0..after)
            .rev()
            .find(|index| self.effects[*index].end > order)
    }

    /// A layer and everything that encloses it, outermost first.
    pub fn filter_chain(&self, index: usize) -> Vec<usize> {
        let mut chain = vec![index];
        let mut walk = self.effects[index].parent;
        while let Some(parent) = walk {
            chain.push(parent);
            walk = self.effects[parent].parent;
        }
        chain.reverse();
        chain
    }

    pub fn insert_primitive(&mut self, primitive: impl Into<Primitive>) {
        let mut primitive = primitive.into();
        let clipped_bounds = primitive
            .bounds()
            .intersect(&primitive.content_mask().bounds);

        if clipped_bounds.is_empty() {
            return;
        }

        let order = self
            .layer_stack
            .last()
            .copied()
            .unwrap_or_else(|| self.primitive_bounds.insert(clipped_bounds));
        match &mut primitive {
            Primitive::Shadow(shadow) => {
                shadow.order = order;
                self.shadows.push(*shadow);
            }
            Primitive::Backdrop(backdrop) => {
                backdrop.order = order;
                self.backdrops.push(*backdrop);
            }
            Primitive::Quad(quad) => {
                quad.order = order;
                self.quads.push(*quad);
            }
            Primitive::Shader(shader) => {
                shader.order = order;
                self.shaders.push(*shader);
            }
            Primitive::Path(path) => {
                path.order = order;
                path.id = PathId(self.paths.len());
                self.paths.push(path.clone());
            }
            Primitive::Underline(underline) => {
                underline.order = order;
                self.underlines.push(*underline);
            }
            Primitive::MonochromeSprite(sprite) => {
                sprite.order = order;
                self.monochrome_sprites.push(*sprite);
            }
            Primitive::SubpixelSprite(sprite) => {
                sprite.order = order;
                self.subpixel_sprites.push(*sprite);
            }
            Primitive::PolychromeSprite(sprite) => {
                sprite.order = order;
                self.polychrome_sprites.push(*sprite);
            }
            Primitive::Surface(surface) => {
                surface.order = order;
                self.surfaces.push(surface.clone());
            }
        }
        self.paint_operations
            .push(PaintOperation::Primitive(primitive));
    }

    pub fn replay(&mut self, range: Range<usize>, prev_scene: &Scene) {
        for operation in &prev_scene.paint_operations[range] {
            match operation {
                PaintOperation::Primitive(primitive) => self.insert_primitive(primitive.clone()),
                PaintOperation::StartLayer(bounds) => self.push_layer(*bounds),
                PaintOperation::EndLayer => self.pop_layer(),
                PaintOperation::StartFilter(bounds, transform_origin, content_mask, filter) => {
                    self.push_filter(*bounds, *transform_origin, *content_mask, *filter)
                }
                PaintOperation::EndFilter => self.pop_filter(),
                PaintOperation::StartSpan(id, bounds) => self.push_span(*id, *bounds),
                PaintOperation::EndSpan => self.pop_span(),
            }
        }
    }

    pub fn finish(&mut self) {
        self.span_stack.clear();
        // A layer that never closed would otherwise swallow everything painted after it.
        for index in self.effect_stack.drain(..) {
            log::warn!("scene: a filtered layer was left open");
            self.effects[index].end = self.effects[index].start;
        }
        self.shadows.sort_by_key(|shadow| shadow.order);
        self.backdrops.sort_by_key(|backdrop| backdrop.order);
        self.quads.sort_by_key(|quad| quad.order);
        self.shaders
            .sort_by_key(|shader| (shader.order, shader.shader));
        self.paths.sort_by_key(|path| path.order);
        self.underlines.sort_by_key(|underline| underline.order);
        self.monochrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.subpixel_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.polychrome_sprites
            .sort_by_key(|sprite| (sprite.order, sprite.tile.tile_id));
        self.surfaces.sort_by_key(|surface| surface.order);
    }

    #[cfg_attr(
        all(
            any(target_os = "linux", target_os = "freebsd"),
            not(any(feature = "x11", feature = "wayland"))
        ),
        allow(dead_code)
    )]
    pub fn batches(&self) -> impl Iterator<Item = PrimitiveBatch> + '_ {
        BatchIterator {
            layered: {
                // A layer whose contents were clipped away holds nothing to redirect, so it does
                // not get to split a batch.
                let mut edges: Vec<DrawOrder> = self
                    .effects
                    .iter()
                    .filter(|layer| layer.end > layer.start + 1)
                    .flat_map(|layer| [layer.start, layer.end])
                    .collect();
                edges.sort_unstable();
                edges
            },
            shadows_start: 0,
            shadows_iter: self.shadows.iter().peekable(),
            backdrops_start: 0,
            backdrops_iter: self.backdrops.iter().peekable(),
            quads_start: 0,
            quads_iter: self.quads.iter().peekable(),
            shaders_start: 0,
            shaders_iter: self.shaders.iter().peekable(),
            paths_start: 0,
            paths_iter: self.paths.iter().peekable(),
            underlines_start: 0,
            underlines_iter: self.underlines.iter().peekable(),
            monochrome_sprites_start: 0,
            monochrome_sprites_iter: self.monochrome_sprites.iter().peekable(),
            subpixel_sprites_start: 0,
            subpixel_sprites_iter: self.subpixel_sprites.iter().peekable(),
            polychrome_sprites_start: 0,
            polychrome_sprites_iter: self.polychrome_sprites.iter().peekable(),
            surfaces_start: 0,
            surfaces_iter: self.surfaces.iter().peekable(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Default)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
pub(crate) enum PrimitiveKind {
    Shadow,
    Backdrop,
    #[default]
    Quad,
    Shader,
    Path,
    Underline,
    MonochromeSprite,
    SubpixelSprite,
    PolychromeSprite,
    Surface,
}

pub(crate) enum PaintOperation {
    Primitive(Primitive),
    StartLayer(Bounds<ScaledPixels>),
    EndLayer,
    StartFilter(
        Bounds<ScaledPixels>,
        Point<ScaledPixels>,
        Bounds<ScaledPixels>,
        Filter,
    ),
    EndFilter,
    StartSpan(SpanId, Bounds<Pixels>),
    EndSpan,
}

/// A run of primitives that is drawn offscreen, put through [`Effect`], and composited back.
#[derive(Clone, Copy, Debug, PartialEq)]
#[expect(missing_docs)]
pub struct LayerEffect {
    /// The order the layer opened at. Everything it contains is drawn between this and `end`.
    pub start: DrawOrder,
    /// The order the layer closed at.
    pub end: DrawOrder,
    /// The area of the untransformed offscreen source, including blur reach.
    pub source_bounds: Bounds<ScaledPixels>,
    /// The center of the element's original bounds, before adding blur reach.
    pub transform_origin: Point<ScaledPixels>,
    /// The source bounds after applying the layer scale and translation.
    pub destination_bounds: Bounds<ScaledPixels>,
    /// The parent content mask that was in force when the layer opened.
    pub content_mask: Bounds<ScaledPixels>,
    pub filter: Filter,
    /// The enclosing layer, if this one is nested.
    pub parent: Option<usize>,
}

impl LayerEffect {
    /// The transformed destination area after clipping to the parent content mask.
    pub fn destination_clip(&self) -> Bounds<ScaledPixels> {
        self.destination_bounds.intersect(&self.content_mask)
    }

    /// The destination span rounded out to whole pixels when a transform made it fractional.
    pub fn destination_scissor_bounds(
        &self,
        destination_span: Bounds<ScaledPixels>,
    ) -> Bounds<ScaledPixels> {
        if !self.filter.transforms() {
            return destination_span;
        }

        Bounds::from_corners(
            point(
                ScaledPixels(destination_span.origin.x.0.floor()),
                ScaledPixels(destination_span.origin.y.0.floor()),
            ),
            point(
                ScaledPixels(destination_span.bottom_right().x.0.ceil()),
                ScaledPixels(destination_span.bottom_right().y.0.ceil()),
            ),
        )
    }

    /// The source area the blur passes need to update.
    ///
    /// Unscaled layers retain the previous clipped processing area. A transformed layer must keep
    /// its whole source because the destination clip is in a different coordinate space.
    pub fn blur_bounds(&self, destination_span: Bounds<ScaledPixels>) -> Bounds<ScaledPixels> {
        if self.filter.transforms() {
            self.source_bounds
        } else {
            destination_span
        }
    }

    /// Whether adjacent sibling layers may share one composite operation.
    pub fn can_merge_with(&self, next: &Self) -> bool {
        self.filter == next.filter
            && self.parent == next.parent
            && !next.filter.fades()
            && !self.filter.transforms()
    }
}

/// What a layer does to its contents on the way back into the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter {
    /// The standard deviation of the blur, in scaled pixels.
    pub blur: f32,
    /// How far the contents fade out at the top edge, in scaled pixels.
    pub fade_top: f32,
    /// How far the contents fade out at the bottom edge, in scaled pixels.
    pub fade_bottom: f32,
    /// How far the contents fade out at the left edge, in scaled pixels.
    pub fade_left: f32,
    /// How far the contents fade out at the right edge, in scaled pixels.
    pub fade_right: f32,
    /// Uniform compositor scale around the layer's transform origin.
    pub scale: f32,
    /// Compositor translation in scaled pixels.
    pub translate: Point<ScaledPixels>,
}

/// The widest blur, as a standard deviation in scaled pixels, that runs as a gaussian at full
/// resolution. A Kawase blur this narrow would pass through half resolution with too little
/// spread to hide it, and a thin line comes back from there visibly stepped.
pub const GAUSSIAN_SIGMA: f32 = 4.;

/// How far a gaussian pass reads to either side, in texels, before its weights stop mattering.
pub const GAUSSIAN_REACH: f32 = 3.;

/// How many times a dual Kawase blur can halve the image. Every level doubles the radius, so six
/// reach close to a hundred scaled pixels.
pub const KAWASE_LEVELS: usize = 6;

/// The fewest levels a Kawase blur uses, since the gaussian covers everything narrower.
const KAWASE_SHALLOWEST: usize = 2;

/// The distance between the offsets [`KAWASE_SPREAD`] was measured at.
const KAWASE_STEP: f32 = 0.25;

/// The standard deviation a dual Kawase blur spreads a point to, divided by `2^levels`, measured
/// at offsets of `0, 0.25, 0.5, ...` with the one up pass and the bilinear stretch the blur ends
/// on. Rows are the level counts `2` and `3`, and the last row serves every deeper count, since
/// the ratio stops moving by then. Each row ends where its kernel would go hollow in the middle
/// and show the blurred shape twice.
const KAWASE_SPREAD: [&[f32]; 3] = [
    &[
        0.484, 0.484, 0.484, 0.505, 0.525, 0.651, 0.741, 0.825, 0.896, 0.958, 1.015, 1.077, 1.136,
        1.242, 1.335, 1.423, 1.502,
    ],
    &[
        0.48, 0.48, 0.48, 0.501, 0.522, 0.659, 0.749, 0.831, 0.9, 0.962, 1.02, 1.085, 1.145, 1.251,
        1.343, 1.434, 1.516,
    ],
    &[
        0.479, 0.479, 0.479, 0.5, 0.521, 0.667, 0.756, 0.835, 0.901, 0.962, 1.02, 1.085, 1.148,
        1.253, 1.346, 1.437, 1.52,
    ],
];

/// How a renderer blurs by a given standard deviation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BlurPasses {
    /// Two separable gaussian passes at full resolution, across and then down, with this
    /// standard deviation in scaled pixels.
    Gaussian(f32),
    /// A dual Kawase blur.
    Kawase(Kawase),
}

impl BlurPasses {
    /// The source margin needed around a clipped blur, including downsample alignment.
    pub fn source_margin(self) -> ScaledPixels {
        ScaledPixels(match self {
            Self::Gaussian(sigma) => (sigma * GAUSSIAN_REACH).ceil() + 1.,
            Self::Kawase(kawase) => kawase.reach() + (1u32 << kawase.levels) as f32,
        })
    }

    /// The blur that spreads a point as far as a gaussian of standard deviation `sigma`, in
    /// scaled pixels.
    pub fn for_sigma(sigma: f32) -> Self {
        match sigma <= GAUSSIAN_SIGMA {
            true => Self::Gaussian(sigma.max(0.)),
            false => Self::Kawase(Kawase::for_sigma(sigma)),
        }
    }
}

/// The passes of a dual Kawase blur. It halves the image `levels` times with a five tap filter and
/// grows it back one level with an eight tap one, leaving the rest of the way to the bilinear
/// sample that composites it, so its cost barely grows with the radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Kawase {
    /// How many times the image is halved, at least two.
    pub levels: usize,
    /// How far the taps spread. A down pass puts its corner taps this many half texels out in
    /// the texture it reads, and an up pass this many quarter texels out, which keeps its ring of
    /// taps from going hollow.
    pub offset: f32,
    /// The level the blur ends on, one above the deepest, which the caller samples it from.
    pub settled: usize,
}

impl Kawase {
    /// The passes that spread a point as far as a gaussian of standard deviation `sigma`, in
    /// scaled pixels. It takes the fewest levels that reach, since every level costs a pass, and a
    /// radius past the deepest level is capped there. Anything under [`GAUSSIAN_SIGMA`] comes out
    /// wider than asked, so go through [`BlurPasses::for_sigma`].
    pub fn for_sigma(sigma: f32) -> Self {
        let sigma = sigma.max(0.);
        let mut levels = KAWASE_SHALLOWEST;
        loop {
            let spread = KAWASE_SPREAD[(levels - KAWASE_SHALLOWEST).min(KAWASE_SPREAD.len() - 1)];
            let scale = (1u32 << levels) as f32;
            let widest = spread[spread.len() - 1] * scale;
            if sigma <= widest || levels == KAWASE_LEVELS {
                let target = sigma / scale;
                let above = spread.partition_point(|&spread| spread < target);
                let step = match above {
                    0 => 0.,
                    above if above == spread.len() => (spread.len() - 1) as f32,
                    above => {
                        let (low, high) = (spread[above - 1], spread[above]);
                        (above - 1) as f32 + (target - low) / (high - low)
                    }
                };
                return Self {
                    levels,
                    offset: step * KAWASE_STEP,
                    settled: levels - 1,
                };
            }
            levels += 1;
        }
    }

    /// How far the passes read past a texel, in scaled pixels of the frame, so a caller can grow
    /// the region it blurs by this much and still have every tap land on written texels.
    pub fn reach(&self) -> f32 {
        // Every pass reaches half an offset and a bilinear texel out, in the texels it reads. The
        // down passes read levels `0` to `levels - 1` and the up pass the deepest one, and a texel
        // is `2^level` scaled pixels wide.
        let tap = self.offset * 0.5 + 1.;
        let deepest = (1u32 << self.levels) as f32;
        tap * (deepest * 2. - 1.)
    }
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            blur: 0.0,
            fade_top: 0.0,
            fade_bottom: 0.0,
            fade_left: 0.0,
            fade_right: 0.0,
            scale: 1.0,
            translate: Point::default(),
        }
    }
}

impl Filter {
    /// Whether the layer changes its contents at all.
    pub fn is_noop(&self) -> bool {
        self.blur <= 0. && !self.fades() && !self.transforms()
    }

    /// Whether the layer has to go through the blur passes.
    pub fn blurs(&self) -> bool {
        self.blur > 0.
    }

    /// Whether the layer is masked on its way back.
    pub fn fades(&self) -> bool {
        self.fade_top > 0. || self.fade_bottom > 0. || self.fade_left > 0. || self.fade_right > 0.
    }

    /// Whether the layer is transformed while it is composited.
    pub fn scales(&self) -> bool {
        self.scale != 1.0
    }

    /// Whether the layer is translated while it is composited.
    pub fn translates(&self) -> bool {
        self.translate != Point::default()
    }

    /// Whether the layer is transformed while it is composited.
    pub fn transforms(&self) -> bool {
        self.scales() || self.translates()
    }
}

pub(crate) fn scale_bounds_around(
    bounds: Bounds<ScaledPixels>,
    origin: Point<ScaledPixels>,
    scale: f32,
) -> Bounds<ScaledPixels> {
    let scaled_x = |value: ScaledPixels| ScaledPixels(origin.x.0 + (value.0 - origin.x.0) * scale);
    let scaled_y = |value: ScaledPixels| ScaledPixels(origin.y.0 + (value.0 - origin.y.0) * scale);

    Bounds::from_corners(
        point(scaled_x(bounds.origin.x), scaled_y(bounds.origin.y)),
        point(
            scaled_x(bounds.bottom_right().x),
            scaled_y(bounds.bottom_right().y),
        ),
    )
}

#[cfg(test)]
mod layer_filter_tests {
    use super::*;
    use crate::{bounds, size};

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
        bounds(
            point(ScaledPixels(x), ScaledPixels(y)),
            size(ScaledPixels(width), ScaledPixels(height)),
        )
    }

    fn origin(x: f32, y: f32) -> Point<ScaledPixels> {
        point(ScaledPixels(x), ScaledPixels(y))
    }

    fn filter(scale: f32) -> Filter {
        Filter {
            scale,
            ..Default::default()
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.0001,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_bounds_close(actual: Bounds<ScaledPixels>, expected: Bounds<ScaledPixels>) {
        assert_close(actual.origin.x.0, expected.origin.x.0);
        assert_close(actual.origin.y.0, expected.origin.y.0);
        assert_close(actual.size.width.0, expected.size.width.0);
        assert_close(actual.size.height.0, expected.size.height.0);
    }

    #[test]
    fn filter_scale_participates_in_noop_detection() {
        assert!(Filter::default().is_noop());
        assert!(filter(1.0).is_noop());
        assert!(!filter(0.99).is_noop());

        let translated = Filter {
            translate: origin(0.25, -0.75),
            ..Default::default()
        };
        assert!(!translated.is_noop());
        assert!(translated.transforms());
    }

    #[test]
    fn fractional_translation_moves_only_the_destination() {
        let source = rect(10.0, 20.0, 100.0, 40.0);
        let mut scene = Scene::default();
        scene.push_filter(
            source,
            origin(60.0, 40.0),
            rect(0.0, 0.0, 200.0, 100.0),
            Filter {
                translate: origin(0.25, -0.75),
                ..Default::default()
            },
        );
        scene.pop_filter();

        assert_eq!(scene.effects[0].source_bounds, source);
        assert_bounds_close(
            scene.effects[0].destination_bounds,
            rect(10.25, 19.25, 100.0, 40.0),
        );
    }

    #[test]
    fn center_based_scaling_keeps_the_origin_fixed() {
        let source = rect(10.0, 20.0, 100.0, 40.0);
        let transform_origin = origin(60.0, 40.0);
        let destination = scale_bounds_around(source, transform_origin, 0.99);

        assert_bounds_close(destination, rect(10.5, 20.2, 99.0, 39.6));
        assert_bounds_close(
            Bounds::centered_at(destination.center(), destination.size),
            destination,
        );
        assert_eq!(destination.center(), transform_origin);
    }

    #[test]
    fn fractional_scale_preserves_subpixel_destination_bounds() {
        let destination =
            scale_bounds_around(rect(4.25, 8.5, 137.0, 53.0), origin(72.75, 35.0), 0.9973);

        assert_close(destination.origin.x.0, 4.43495);
        assert_close(destination.origin.y.0, 8.57155);
        assert_close(destination.size.width.0, 136.6301);
        assert_close(destination.size.height.0, 52.8569);
    }

    #[test]
    fn layer_effect_separates_source_destination_and_parent_clip() {
        let source = rect(10.0, 20.0, 100.0, 40.0);
        let content_mask = rect(20.0, 10.0, 80.0, 80.0);
        let mut scene = Scene::default();
        scene.push_filter(source, origin(60.0, 40.0), content_mask, filter(1.1));
        scene.pop_filter();

        let effect = scene.effects[0];
        assert_eq!(effect.source_bounds, source);
        assert_eq!(effect.transform_origin, origin(60.0, 40.0));
        assert_bounds_close(effect.destination_bounds, rect(5.0, 18.0, 110.0, 44.0));
        assert_bounds_close(effect.destination_clip(), rect(20.0, 18.0, 80.0, 44.0));
        assert_eq!(effect.blur_bounds(effect.destination_clip()), source);

        let fractional_span = rect(20.25, 18.75, 79.5, 43.5);
        assert_eq!(
            effect.destination_scissor_bounds(fractional_span),
            rect(20.0, 18.0, 80.0, 45.0)
        );
    }

    #[test]
    fn nested_effects_keep_their_parent_and_geometry() {
        let mut scene = Scene::default();
        scene.push_filter(
            rect(0.0, 0.0, 200.0, 100.0),
            origin(100.0, 50.0),
            rect(0.0, 0.0, 300.0, 200.0),
            filter(0.99),
        );
        scene.push_filter(
            rect(20.0, 10.0, 50.0, 30.0),
            origin(45.0, 25.0),
            rect(0.0, 0.0, 200.0, 100.0),
            filter(0.9973),
        );
        scene.pop_filter();
        scene.pop_filter();

        assert_eq!(scene.effects[0].parent, None);
        assert_eq!(scene.effects[1].parent, Some(0));
        assert_eq!(scene.filter_chain(1), vec![0, 1]);
        assert_eq!(scene.effects[1].transform_origin, origin(45.0, 25.0));
    }

    #[test]
    fn scene_replay_restores_scaled_layer_effects() {
        let mut previous = Scene::default();
        previous.push_filter(
            rect(10.0, 20.0, 100.0, 40.0),
            origin(60.0, 40.0),
            rect(0.0, 0.0, 120.0, 80.0),
            Filter {
                blur: 3.0,
                fade_top: 2.0,
                scale: 0.9973,
                ..Default::default()
            },
        );
        previous.pop_filter();

        let mut replayed = Scene::default();
        replayed.replay(0..previous.len(), &previous);

        assert_eq!(replayed.effects, previous.effects);
    }

    #[test]
    fn transformed_sibling_layers_cannot_merge() {
        let bounds = rect(0.0, 0.0, 100.0, 50.0);
        let content_mask = rect(0.0, 0.0, 500.0, 500.0);
        let mut scene = Scene::default();
        scene.push_filter(bounds, origin(50.0, 25.0), content_mask, filter(0.99));
        scene.pop_filter();
        scene.push_filter(bounds, origin(150.0, 25.0), content_mask, filter(0.99));
        scene.pop_filter();

        assert!(!scene.effects[0].can_merge_with(&scene.effects[1]));

        let mut unscaled = scene.effects;
        unscaled[0].filter.scale = 1.0;
        unscaled[1].filter.scale = 1.0;
        assert!(unscaled[0].can_merge_with(&unscaled[1]));
    }
}

#[derive(Clone)]
#[expect(missing_docs)]
pub enum Primitive {
    Shadow(Shadow),
    Backdrop(Backdrop),
    Quad(Quad),
    Shader(ShaderQuad),
    Path(Path<ScaledPixels>),
    Underline(Underline),
    MonochromeSprite(MonochromeSprite),
    SubpixelSprite(SubpixelSprite),
    PolychromeSprite(PolychromeSprite),
    Surface(PaintSurface),
}

#[expect(missing_docs)]
impl Primitive {
    /// The primitive with the alpha of its colours scaled by `fade`. A surface has no colour
    /// of its own and comes back as it was.
    pub fn faded(mut self, fade: f32) -> Self {
        let fade = fade.clamp(0., 1.);
        match &mut self {
            Primitive::Shadow(shadow) => shadow.color = shadow.color.opacity(fade),
            Primitive::Backdrop(backdrop) => backdrop.opacity *= fade,
            Primitive::Quad(quad) => {
                quad.background = quad.background.opacity(fade);
                quad.border_color = quad.border_color.opacity(fade);
            }
            Primitive::Shader(shader) => shader.opacity *= fade,
            Primitive::Path(path) => path.color = path.color.opacity(fade),
            Primitive::Underline(underline) => underline.color = underline.color.opacity(fade),
            Primitive::MonochromeSprite(sprite) => sprite.color = sprite.color.opacity(fade),
            Primitive::SubpixelSprite(sprite) => sprite.color = sprite.color.opacity(fade),
            Primitive::PolychromeSprite(sprite) => sprite.opacity *= fade,
            Primitive::Surface(_) => {}
        }
        self
    }

    pub fn bounds(&self) -> &Bounds<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.bounds,
            Primitive::Backdrop(backdrop) => &backdrop.bounds,
            Primitive::Quad(quad) => &quad.bounds,
            Primitive::Shader(shader) => &shader.bounds,
            Primitive::Path(path) => &path.bounds,
            Primitive::Underline(underline) => &underline.bounds,
            Primitive::MonochromeSprite(sprite) => &sprite.bounds,
            Primitive::SubpixelSprite(sprite) => &sprite.bounds,
            Primitive::PolychromeSprite(sprite) => &sprite.bounds,
            Primitive::Surface(surface) => &surface.bounds,
        }
    }

    pub fn content_mask(&self) -> &ContentMask<ScaledPixels> {
        match self {
            Primitive::Shadow(shadow) => &shadow.content_mask,
            Primitive::Backdrop(backdrop) => &backdrop.content_mask,
            Primitive::Quad(quad) => &quad.content_mask,
            Primitive::Shader(shader) => &shader.content_mask,
            Primitive::Path(path) => &path.content_mask,
            Primitive::Underline(underline) => &underline.content_mask,
            Primitive::MonochromeSprite(sprite) => &sprite.content_mask,
            Primitive::SubpixelSprite(sprite) => &sprite.content_mask,
            Primitive::PolychromeSprite(sprite) => &sprite.content_mask,
            Primitive::Surface(surface) => &surface.content_mask,
        }
    }
}

#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
struct BatchIterator<'a> {
    /// Where filtered layers open and close, sorted. A batch never spans one of these.
    layered: Vec<DrawOrder>,
    shadows_start: usize,
    shadows_iter: Peekable<slice::Iter<'a, Shadow>>,
    backdrops_start: usize,
    backdrops_iter: Peekable<slice::Iter<'a, Backdrop>>,
    quads_start: usize,
    quads_iter: Peekable<slice::Iter<'a, Quad>>,
    shaders_start: usize,
    shaders_iter: Peekable<slice::Iter<'a, ShaderQuad>>,
    paths_start: usize,
    paths_iter: Peekable<slice::Iter<'a, Path<ScaledPixels>>>,
    underlines_start: usize,
    underlines_iter: Peekable<slice::Iter<'a, Underline>>,
    monochrome_sprites_start: usize,
    monochrome_sprites_iter: Peekable<slice::Iter<'a, MonochromeSprite>>,
    subpixel_sprites_start: usize,
    subpixel_sprites_iter: Peekable<slice::Iter<'a, SubpixelSprite>>,
    polychrome_sprites_start: usize,
    polychrome_sprites_iter: Peekable<slice::Iter<'a, PolychromeSprite>>,
    surfaces_start: usize,
    surfaces_iter: Peekable<slice::Iter<'a, PaintSurface>>,
}

impl<'a> Iterator for BatchIterator<'a> {
    type Item = PrimitiveBatch;

    fn next(&mut self) -> Option<Self::Item> {
        let mut orders_and_kinds = [
            (
                self.shadows_iter.peek().map(|s| s.order),
                PrimitiveKind::Shadow,
            ),
            (
                self.backdrops_iter.peek().map(|b| b.order),
                PrimitiveKind::Backdrop,
            ),
            (self.quads_iter.peek().map(|q| q.order), PrimitiveKind::Quad),
            (
                self.shaders_iter.peek().map(|s| s.order),
                PrimitiveKind::Shader,
            ),
            (self.paths_iter.peek().map(|q| q.order), PrimitiveKind::Path),
            (
                self.underlines_iter.peek().map(|u| u.order),
                PrimitiveKind::Underline,
            ),
            (
                self.monochrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::MonochromeSprite,
            ),
            (
                self.subpixel_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::SubpixelSprite,
            ),
            (
                self.polychrome_sprites_iter.peek().map(|s| s.order),
                PrimitiveKind::PolychromeSprite,
            ),
            (
                self.surfaces_iter.peek().map(|s| s.order),
                PrimitiveKind::Surface,
            ),
        ];
        orders_and_kinds.sort_by_key(|(order, kind)| (order.unwrap_or(u32::MAX), *kind));

        let first = orders_and_kinds[0];
        let second = orders_and_kinds[1];
        let (batch_kind, max_order_and_kind) = if first.0.is_some() {
            (first.1, (second.0.unwrap_or(u32::MAX), second.1))
        } else {
            return None;
        };

        let first_order = orders_and_kinds[0].0.unwrap_or_default();
        // A batch may not cross into or out of a filtered layer, or the layer's primitives could
        // not be redirected to its own target. Folding the nearest boundary into the threshold the
        // run already compares against keeps that free for every primitive that has no filter.
        let edge = self.layered.partition_point(|order| *order <= first_order);
        let limit = self.layered.get(edge).copied().unwrap_or(DrawOrder::MAX);
        let max_order_and_kind = max_order_and_kind.min((limit, PrimitiveKind::Shadow));

        match batch_kind {
            PrimitiveKind::Shadow => {
                let shadows_start = self.shadows_start;
                let mut shadows_end = shadows_start + 1;
                self.shadows_iter.next();
                while self
                    .shadows_iter
                    .next_if(|shadow| (shadow.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    shadows_end += 1;
                }
                self.shadows_start = shadows_end;
                Some(PrimitiveBatch::Shadows(shadows_start..shadows_end))
            }
            PrimitiveKind::Backdrop => {
                let backdrops_start = self.backdrops_start;
                let mut backdrops_end = backdrops_start + 1;
                self.backdrops_iter.next();
                while self
                    .backdrops_iter
                    .next_if(|backdrop| (backdrop.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    backdrops_end += 1;
                }
                self.backdrops_start = backdrops_end;
                Some(PrimitiveBatch::Backdrops(backdrops_start..backdrops_end))
            }
            PrimitiveKind::Quad => {
                let quads_start = self.quads_start;
                let mut quads_end = quads_start + 1;
                self.quads_iter.next();
                while self
                    .quads_iter
                    .next_if(|quad| (quad.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    quads_end += 1;
                }
                self.quads_start = quads_end;
                Some(PrimitiveBatch::Quads(quads_start..quads_end))
            }
            PrimitiveKind::Shader => {
                let shader = self.shaders_iter.peek().unwrap().shader;
                let shaders_start = self.shaders_start;
                let mut shaders_end = shaders_start + 1;
                self.shaders_iter.next();
                while self
                    .shaders_iter
                    .next_if(|instance| {
                        (instance.order, batch_kind) < max_order_and_kind
                            && instance.shader == shader
                    })
                    .is_some()
                {
                    shaders_end += 1;
                }
                self.shaders_start = shaders_end;
                Some(PrimitiveBatch::Shaders {
                    shader,
                    range: shaders_start..shaders_end,
                })
            }
            PrimitiveKind::Path => {
                let paths_start = self.paths_start;
                let mut paths_end = paths_start + 1;
                self.paths_iter.next();
                while self
                    .paths_iter
                    .next_if(|path| (path.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    paths_end += 1;
                }
                self.paths_start = paths_end;
                Some(PrimitiveBatch::Paths(paths_start..paths_end))
            }
            PrimitiveKind::Underline => {
                let underlines_start = self.underlines_start;
                let mut underlines_end = underlines_start + 1;
                self.underlines_iter.next();
                while self
                    .underlines_iter
                    .next_if(|underline| (underline.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    underlines_end += 1;
                }
                self.underlines_start = underlines_end;
                Some(PrimitiveBatch::Underlines(underlines_start..underlines_end))
            }
            PrimitiveKind::MonochromeSprite => {
                let texture_id = self.monochrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.monochrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.monochrome_sprites_iter.next();
                while self
                    .monochrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.monochrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::MonochromeSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::SubpixelSprite => {
                let texture_id = self.subpixel_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.subpixel_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.subpixel_sprites_iter.next();
                while self
                    .subpixel_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.subpixel_sprites_start = sprites_end;
                Some(PrimitiveBatch::SubpixelSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::PolychromeSprite => {
                let texture_id = self.polychrome_sprites_iter.peek().unwrap().tile.texture_id;
                let sprites_start = self.polychrome_sprites_start;
                let mut sprites_end = sprites_start + 1;
                self.polychrome_sprites_iter.next();
                while self
                    .polychrome_sprites_iter
                    .next_if(|sprite| {
                        (sprite.order, batch_kind) < max_order_and_kind
                            && sprite.tile.texture_id == texture_id
                    })
                    .is_some()
                {
                    sprites_end += 1;
                }
                self.polychrome_sprites_start = sprites_end;
                Some(PrimitiveBatch::PolychromeSprites {
                    texture_id,
                    range: sprites_start..sprites_end,
                })
            }
            PrimitiveKind::Surface => {
                let surfaces_start = self.surfaces_start;
                let mut surfaces_end = surfaces_start + 1;
                self.surfaces_iter.next();
                while self
                    .surfaces_iter
                    .next_if(|surface| (surface.order, batch_kind) < max_order_and_kind)
                    .is_some()
                {
                    surfaces_end += 1;
                }
                self.surfaces_start = surfaces_end;
                Some(PrimitiveBatch::Surfaces(surfaces_start..surfaces_end))
            }
        }
    }
}

#[derive(Debug)]
#[cfg_attr(
    all(
        any(target_os = "linux", target_os = "freebsd"),
        not(any(feature = "x11", feature = "wayland"))
    ),
    allow(dead_code)
)]
#[allow(missing_docs)]
pub enum PrimitiveBatch {
    Shadows(Range<usize>),
    Backdrops(Range<usize>),
    Quads(Range<usize>),
    /// Instances of one custom shader.
    Shaders {
        shader: ShaderId,
        range: Range<usize>,
    },
    Paths(Range<usize>),
    Underlines(Range<usize>),
    MonochromeSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    SubpixelSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    PolychromeSprites {
        texture_id: AtlasTextureId,
        range: Range<usize>,
    },
    Surfaces(Range<usize>),
}

impl PrimitiveBatch {
    #[expect(missing_docs)]
    pub fn label(&self) -> String {
        match self {
            Self::Shadows(range) => format!("shadows ({})", range.len()),
            Self::Backdrops(range) => format!("backdrops ({})", range.len()),
            Self::Quads(range) => format!("quads ({})", range.len()),
            Self::Shaders { shader, range } => {
                format!("shader {} ({})", shader.0, range.len())
            }
            Self::Paths(range) => format!("paths ({})", range.len()),
            Self::Underlines(range) => format!("underlines ({})", range.len()),
            Self::MonochromeSprites { texture_id, range } => {
                format!(
                    "monochrome sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::SubpixelSprites { texture_id, range } => {
                format!(
                    "subpixel sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::PolychromeSprites { texture_id, range } => {
                format!(
                    "polychrome sprites ({}) on atlas {}",
                    range.len(),
                    texture_id.index
                )
            }
            Self::Surfaces(range) => format!("surfaces ({})", range.len()),
        }
    }
}

#[derive(Default, Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Backdrop {
    pub order: DrawOrder,
    /// How far the corners are smoothed from circular arcs towards a squircle, from 0 to 1.
    pub corner_smoothing: f32,
    /// The standard deviation of the blur, in scaled pixels.
    pub blur: f32,
    /// How much of the blurred backdrop shows through.
    pub opacity: f32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
}

impl From<Backdrop> for Primitive {
    fn from(backdrop: Backdrop) -> Self {
        Primitive::Backdrop(backdrop)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Quad {
    pub order: DrawOrder,
    pub border_style: BorderStyle,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub background: Background,
    pub border_color: Hsla,
    pub corner_radii: Corners<ScaledPixels>,
    pub border_widths: Edges<ScaledPixels>,
    /// How far the corners are smoothed from circular arcs towards a squircle, from 0 to 1.
    pub corner_smoothing: f32,
    pub pad: u32, // align to 8 bytes
}

impl From<Quad> for Primitive {
    fn from(quad: Quad) -> Self {
        Primitive::Quad(quad)
    }
}

/// One instance of a custom shader, drawn over a rounded rectangle. Its layout is the
/// `GpuiShaderInstance` struct of the shader prelude, so the renderers upload it as is.
#[derive(Debug, Copy, Clone)]
#[repr(C)]
pub struct ShaderQuad {
    /// Where the instance falls among the frame's primitives.
    pub order: DrawOrder,
    /// The shader that colours it.
    pub shader: ShaderId,
    /// The element opacity it is faded by.
    pub opacity: f32,
    /// The time the app painted it with, in seconds.
    pub time: f32,
    /// Its bounds in device pixels.
    pub bounds: Bounds<ScaledPixels>,
    /// The area it is clipped to.
    pub content_mask: ContentMask<ScaledPixels>,
    /// The radii of its corners in device pixels.
    pub corner_radii: Corners<ScaledPixels>,
    /// Device pixels per logical pixel.
    pub scale_factor: f32,
    /// Keeps `params` on the 16 byte boundary the shader reads it at.
    pub pad: [u32; 3],
    /// The parameters the app painted it with.
    pub params: [f32; crate::SHADER_PARAM_COUNT],
    /// The colours the app painted it with, as straight alpha sRGB.
    pub colors: [[f32; 4]; crate::SHADER_COLOR_COUNT],
}

impl From<ShaderQuad> for Primitive {
    fn from(shader: ShaderQuad) -> Self {
        Primitive::Shader(shader)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Underline {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub thickness: ScaledPixels,
    pub wavy: PaddedBool32,
}

impl From<Underline> for Primitive {
    fn from(underline: Underline) -> Self {
        Primitive::Underline(underline)
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(C)]
#[expect(missing_docs)]
pub struct Shadow {
    pub order: DrawOrder,
    pub blur_radius: ScaledPixels,
    pub bounds: Bounds<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub element_bounds: Bounds<ScaledPixels>,
    pub element_corner_radii: Corners<ScaledPixels>,
    /// 0 = drop shadow (rendered outside the element), 1 = inset shadow (rendered inside).
    pub inset: u32,
    /// The corner smoothing of the element's own outline, which clips the shadow.
    pub element_corner_smoothing: f32,
}

impl From<Shadow> for Primitive {
    fn from(shadow: Shadow) -> Self {
        Primitive::Shadow(shadow)
    }
}

/// The style of a border.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[repr(C)]
pub enum BorderStyle {
    /// A solid border.
    #[default]
    Solid = 0,
    /// A dashed border.
    Dashed = 1,
}

/// A data type representing a 2 dimensional transformation that can be applied to an element.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct TransformationMatrix {
    /// 2x2 matrix containing rotation and scale,
    /// stored row-major
    pub rotation_scale: [[f32; 2]; 2],
    /// translation vector
    pub translation: [f32; 2],
}

impl Eq for TransformationMatrix {}

impl TransformationMatrix {
    /// The unit matrix, has no effect.
    pub fn unit() -> Self {
        Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [0.0, 0.0],
        }
    }

    /// Move the origin by a given point
    pub fn translate(mut self, point: Point<ScaledPixels>) -> Self {
        self.compose(Self {
            rotation_scale: [[1.0, 0.0], [0.0, 1.0]],
            translation: [point.x.0, point.y.0],
        })
    }

    /// Clockwise rotation in radians around the origin
    pub fn rotate(self, angle: Radians) -> Self {
        self.compose(Self {
            rotation_scale: [
                [angle.0.cos(), -angle.0.sin()],
                [angle.0.sin(), angle.0.cos()],
            ],
            translation: [0.0, 0.0],
        })
    }

    /// Scale around the origin
    pub fn scale(self, size: Size<f32>) -> Self {
        self.compose(Self {
            rotation_scale: [[size.width, 0.0], [0.0, size.height]],
            translation: [0.0, 0.0],
        })
    }

    /// Perform matrix multiplication with another transformation
    /// to produce a new transformation that is the result of
    /// applying both transformations: first, `other`, then `self`.
    #[inline]
    pub fn compose(self, other: TransformationMatrix) -> TransformationMatrix {
        if other == Self::unit() {
            return self;
        }
        // Perform matrix multiplication
        TransformationMatrix {
            rotation_scale: [
                [
                    self.rotation_scale[0][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][0],
                    self.rotation_scale[0][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[0][1] * other.rotation_scale[1][1],
                ],
                [
                    self.rotation_scale[1][0] * other.rotation_scale[0][0]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][0],
                    self.rotation_scale[1][0] * other.rotation_scale[0][1]
                        + self.rotation_scale[1][1] * other.rotation_scale[1][1],
                ],
            ],
            translation: [
                self.translation[0]
                    + self.rotation_scale[0][0] * other.translation[0]
                    + self.rotation_scale[0][1] * other.translation[1],
                self.translation[1]
                    + self.rotation_scale[1][0] * other.translation[0]
                    + self.rotation_scale[1][1] * other.translation[1],
            ],
        }
    }

    /// Apply transformation to a point, mainly useful for debugging
    pub fn apply(&self, point: Point<Pixels>) -> Point<Pixels> {
        let input = [point.x.0, point.y.0];
        let mut output = self.translation;
        for (i, output_cell) in output.iter_mut().enumerate() {
            for (k, input_cell) in input.iter().enumerate() {
                *output_cell += self.rotation_scale[i][k] * *input_cell;
            }
        }
        Point::new(output[0].into(), output[1].into())
    }
}

impl Default for TransformationMatrix {
    fn default() -> Self {
        Self::unit()
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct MonochromeSprite {
    pub order: DrawOrder,
    pub pad: u32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub tile: AtlasTile,
    pub transformation: TransformationMatrix,
}

impl From<MonochromeSprite> for Primitive {
    fn from(sprite: MonochromeSprite) -> Self {
        Primitive::MonochromeSprite(sprite)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct SubpixelSprite {
    pub order: DrawOrder,
    pub pad: u32, // align to 8 bytes
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub color: Hsla,
    pub tile: AtlasTile,
    pub transformation: TransformationMatrix,
}

impl From<SubpixelSprite> for Primitive {
    fn from(sprite: SubpixelSprite) -> Self {
        Primitive::SubpixelSprite(sprite)
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PolychromeSprite {
    pub order: DrawOrder,
    pub pad: u32,
    pub grayscale: PaddedBool32,
    pub opacity: f32,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    pub corner_radii: Corners<ScaledPixels>,
    pub tile: AtlasTile,
}

impl From<PolychromeSprite> for Primitive {
    fn from(sprite: PolychromeSprite) -> Self {
        Primitive::PolychromeSprite(sprite)
    }
}

#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct PaintSurface {
    pub order: DrawOrder,
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    pub image_buffer: objc2_core_foundation::CFRetained<objc2_core_video::CVPixelBuffer>,
}

impl From<PaintSurface> for Primitive {
    fn from(surface: PaintSurface) -> Self {
        Primitive::Surface(surface)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[expect(missing_docs)]
pub struct PathId(pub usize);

/// A line made up of a series of vertices and control points.
#[derive(Clone, Debug)]
#[expect(missing_docs)]
pub struct Path<P: Clone + Debug + Default + PartialEq> {
    pub id: PathId,
    pub order: DrawOrder,
    pub bounds: Bounds<P>,
    pub content_mask: ContentMask<P>,
    pub vertices: Vec<PathVertex<P>>,
    pub color: Background,
    start: Point<P>,
    current: Point<P>,
    contour_count: usize,
}

impl Path<Pixels> {
    /// Create a new path with the given starting point.
    pub fn new(start: Point<Pixels>) -> Self {
        Self {
            id: PathId(0),
            order: DrawOrder::default(),
            vertices: Vec::new(),
            start,
            current: start,
            bounds: Bounds {
                origin: start,
                size: Default::default(),
            },
            content_mask: Default::default(),
            color: Default::default(),
            contour_count: 0,
        }
    }

    /// Scale this path by the given factor.
    pub fn scale(&self, factor: f32) -> Path<ScaledPixels> {
        Path {
            id: self.id,
            order: self.order,
            bounds: self.bounds.scale(factor),
            content_mask: self.content_mask.scale(factor),
            vertices: self
                .vertices
                .iter()
                .map(|vertex| vertex.scale(factor))
                .collect(),
            start: self.start.map(|start| start.scale(factor)),
            current: self.current.scale(factor),
            contour_count: self.contour_count,
            color: self.color,
        }
    }

    /// Move the start, current point to the given point.
    pub fn move_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        self.start = to;
        self.current = to;
    }

    /// Draw a straight line from the current point to the given point.
    pub fn line_to(&mut self, to: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }
        self.current = to;
    }

    /// Draw a curve from the current point to the given point, using the given control point.
    pub fn curve_to(&mut self, to: Point<Pixels>, ctrl: Point<Pixels>) {
        self.contour_count += 1;
        if self.contour_count > 1 {
            self.push_triangle(
                (self.start, self.current, to),
                (point(0., 1.), point(0., 1.), point(0., 1.)),
            );
        }

        self.push_triangle(
            (self.current, ctrl, to),
            (point(0., 0.), point(0.5, 0.), point(1., 1.)),
        );
        self.current = to;
    }

    /// Push a triangle to the Path.
    pub fn push_triangle(
        &mut self,
        xy: (Point<Pixels>, Point<Pixels>, Point<Pixels>),
        st: (Point<f32>, Point<f32>, Point<f32>),
    ) {
        self.bounds = self
            .bounds
            .union(&Bounds {
                origin: xy.0,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.1,
                size: Default::default(),
            })
            .union(&Bounds {
                origin: xy.2,
                size: Default::default(),
            });

        self.vertices.push(PathVertex {
            xy_position: xy.0,
            st_position: st.0,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.1,
            st_position: st.1,
            content_mask: Default::default(),
        });
        self.vertices.push(PathVertex {
            xy_position: xy.2,
            st_position: st.2,
            content_mask: Default::default(),
        });
    }
}

impl<T> Path<T>
where
    T: Clone + Debug + Default + PartialEq + PartialOrd + Add<T, Output = T> + Sub<Output = T>,
{
    #[allow(unused)]
    #[expect(missing_docs)]
    pub fn clipped_bounds(&self) -> Bounds<T> {
        self.bounds.intersect(&self.content_mask.bounds)
    }
}

impl From<Path<ScaledPixels>> for Primitive {
    fn from(path: Path<ScaledPixels>) -> Self {
        Primitive::Path(path)
    }
}

#[derive(Clone, Debug)]
#[repr(C)]
#[expect(missing_docs)]
pub struct PathVertex<P: Clone + Debug + Default + PartialEq> {
    pub xy_position: Point<P>,
    pub st_position: Point<f32>,
    pub content_mask: ContentMask<P>,
}

#[expect(missing_docs)]
impl PathVertex<Pixels> {
    pub fn scale(&self, factor: f32) -> PathVertex<ScaledPixels> {
        PathVertex {
            xy_position: self.xy_position.scale(factor),
            st_position: self.st_position,
            content_mask: self.content_mask.scale(factor),
        }
    }
}
