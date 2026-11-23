use crate::{CompositorGpuHint, DeviceErrorState, WgpuAtlas, WgpuContext};
use anyhow::{Context as _, Result};
use bytemuck::{Pod, Zeroable};
use collections::FxHashMap;
use gpui::{
    AtlasTextureId, Background, BlurPasses, Bounds, DevicePixels, GAUSSIAN_REACH, GpuSpecs,
    KAWASE_LEVELS, Kawase, LayerEffect, Path, Point, PrimitiveBatch, ScaledPixels, Scene, ShaderId,
    Size, get_gamma_correction_ratios, point, size,
};
use log::warn;
#[cfg(not(target_family = "wasm"))]
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use smallvec::SmallVec;
use std::cell::RefCell;
use std::num::NonZeroU64;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

const MAX_INSTANCE_BUFFER_SIZE: u64 = 256 * 1024 * 1024;

const INITIAL_INSTANCE_DATA_SIZE: u64 = 2 * 1024 * 1024;

/// How long the instance data watches its peak use before deciding whether to shrink.
const INSTANCE_DATA_SETTLE: Duration = Duration::from_secs(5);

/// How long an offscreen target may go unused before its memory is given back. A frame with a
/// blur, layer or path on screen touches its targets every time, so only targets that nothing
/// on screen draws through are released.
const IDLE_RELEASE: Duration = Duration::from_secs(2);

const INSTANCE_TEXTURE_TEXEL_SIZE: u64 = 16;

/// Shader variant for backends with storage buffer support: the shared shader
/// logic plus the storage-buffer instance transport.
const STORAGE_BUFFER_SHADERS: &str = concat!(
    include_str!("shaders.wgsl"),
    include_str!("shaders_storage.wgsl"),
);

/// Shader variant for WebGL2, which has no storage buffers: the shared shader
/// logic plus the texture-based instance transport.
const WEBGL_SHADERS: &str = concat!(
    include_str!("shaders.wgsl"),
    include_str!("shaders_webgl.wgsl"),
);

/// Subpixel text rendering requires dual-source blending, which WebGL2 lacks, so
/// this variant only ever runs with the storage-buffer transport. The `enable`
/// directive must precede all declarations.
const SUBPIXEL_SHADERS: &str = concat!(
    "enable dual_source_blending;\n",
    include_str!("shaders.wgsl"),
    include_str!("shaders_storage.wgsl"),
    include_str!("shaders_subpixel.wgsl"),
);

/// How custom shaders read their instances on WebGL2, in place of the storage buffer transport.
const CUSTOM_SHADER_WEBGL_TRANSPORT: &str = include_str!("custom_shader_webgl.wgsl");

fn least_common_multiple(left: u64, right: u64) -> u64 {
    let mut first = left;
    let mut second = right;
    while second != 0 {
        let remainder = first % second;
        first = second;
        second = remainder;
    }
    left / first * right
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlobalParams {
    viewport_size: [f32; 2],
    premultiplied_alpha: u32,
    pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PodBounds {
    origin: [f32; 2],
    size: [f32; 2],
}

impl From<Bounds<ScaledPixels>> for PodBounds {
    fn from(bounds: Bounds<ScaledPixels>) -> Self {
        Self {
            origin: [bounds.origin.x.0, bounds.origin.y.0],
            size: [bounds.size.width.0, bounds.size.height.0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SurfaceParams {
    bounds: PodBounds,
    content_mask: PodBounds,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GammaParams {
    gamma_ratios: [f32; 4],
    grayscale_enhanced_contrast: f32,
    subpixel_enhanced_contrast: f32,
    is_bgr: u32,
    _pad: u32,
}

#[derive(Clone, Debug)]
#[repr(C)]
struct PathSprite {
    bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Debug)]
#[repr(C)]
struct PathRasterizationVertex {
    xy_position: Point<ScaledPixels>,
    st_position: Point<f32>,
    color: Background,
    bounds: Bounds<ScaledPixels>,
}

pub struct WgpuSurfaceConfig {
    pub size: Size<DevicePixels>,
    pub transparent: bool,
    /// Preferred presentation mode. When `Some`, the renderer will use this
    /// mode if supported by the surface, falling back to `Fifo`.
    /// When `None`, defaults to `Fifo` (VSync).
    ///
    /// Mobile platforms may prefer `Mailbox` (triple-buffering) to avoid
    /// blocking in `get_current_texture()` during lifecycle transitions.
    pub preferred_present_mode: Option<wgpu::PresentMode>,
}

struct WgpuPipelines {
    backdrops: wgpu::RenderPipeline,
    backdrop_punch: wgpu::RenderPipeline,
    gaussian: wgpu::RenderPipeline,
    kawase_down: wgpu::RenderPipeline,
    kawase_up: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    masked: wgpu::RenderPipeline,
    /// Composites a layer blurred across only, running the gaussian down on the way.
    blurred: wgpu::RenderPipeline,
    quads: wgpu::RenderPipeline,
    shadows: wgpu::RenderPipeline,
    path_rasterization: wgpu::RenderPipeline,
    paths: wgpu::RenderPipeline,
    underlines: wgpu::RenderPipeline,
    mono_sprites: wgpu::RenderPipeline,
    subpixel_sprites: Option<wgpu::RenderPipeline>,
    /// The same glyphs for when they land in an offscreen layer rather than the frame; see
    /// `fs_subpixel_sprite_layered`.
    subpixel_sprites_layered: Option<wgpu::RenderPipeline>,
    poly_sprites: wgpu::RenderPipeline,
    #[allow(dead_code)]
    surfaces: wgpu::RenderPipeline,
    /// The colour target custom shader pipelines are created against, the same as quads draw to.
    custom_target: wgpu::ColorTargetState,
    /// A pipeline per custom shader, created the first time a frame draws it. A shader that
    /// failed to compile is kept as `None`, so it is not compiled again every frame.
    custom: FxHashMap<ShaderId, Option<wgpu::RenderPipeline>>,
}

/// One frame allocation of instance data, ready to bind.
struct InstanceBinding {
    bind_group: wgpu::BindGroup,
    /// Index of the allocation's first instance within the bound data. Always
    /// zero on the storage-buffer path, where the binding offset already
    /// positions the array; on the WebGL texture path the shader indexes the
    /// shared instance texture absolutely, so draws must offset their
    /// instance (or vertex) ranges by this value.
    first_instance: u32,
}

struct InstanceBindings {
    backdrops: InstanceBinding,
    quads: InstanceBinding,
    shaders: InstanceBinding,
    shadows: InstanceBinding,
    underlines: InstanceBinding,
    monochrome_sprites: InstanceBinding,
    subpixel_sprites: InstanceBinding,
    polychrome_sprites: InstanceBinding,
}

struct WgpuBindGroupLayouts {
    globals: wgpu::BindGroupLayout,
    instances: wgpu::BindGroupLayout,
    texture: wgpu::BindGroupLayout,
    surfaces: wgpu::BindGroupLayout,
}

/// Shared GPU context reference, used to coordinate device recovery across multiple windows.
pub type GpuContext = Rc<RefCell<Option<WgpuContext>>>;

enum InstanceData {
    Storage(wgpu::Buffer),
    // WebGL2 has no storage buffers. A uint texture keeps the records available to both shader
    // stages while preserving integer and floating-point bit patterns exactly.
    Texture {
        texture: wgpu::Texture,
        view: wgpu::TextureView,
        width: u32,
        height: u32,
    },
}

/// GPU resources that must be dropped together during device recovery.
struct WgpuResources {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipelines: WgpuPipelines,
    bind_group_layouts: WgpuBindGroupLayouts,
    atlas_sampler: wgpu::Sampler,
    atlas_texture_bind_groups: FxHashMap<AtlasTextureId, CachedTextureBindGroup>,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    path_globals_bind_group: wgpu::BindGroup,
    instance_data: InstanceData,
    /// Where paths are rasterized before they are copied into the frame.
    path_intermediate: Option<Scratch>,
    /// The multisampled target paths are rasterized into and resolved from, when the target
    /// format supports multisampling.
    path_msaa: Option<Scratch>,
    filter_targets: FilterTargets,
}

struct CachedTextureBindGroup {
    texture_generation: u64,
    bind_group: wgpu::BindGroup,
}

#[repr(C, align(8))]
#[derive(Clone, Copy, Default)]
struct MaskParams {
    source_bounds: [f32; 4],
    fade_bounds: [f32; 4],
    transform_origin: [f32; 2],
    translation: [f32; 2],
    scale: f32,
    fade_top: f32,
    fade_bottom: f32,
    fade_left: f32,
    fade_right: f32,
    pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct BlurParams {
    direction: [f32; 2],
    radius: f32,
    pad: u32,
}

/// How deeply filtered layers can nest before the innermost ones stop being filtered.
const LAYER_DEPTH: usize = 4;

/// An offscreen render target that is created the first time a frame needs it and released
/// once it has gone `IDLE_RELEASE` without use.
struct Scratch {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    used: Instant,
}

impl Scratch {
    fn new(
        device: &wgpu::Device,
        label: &str,
        format: wgpu::TextureFormat,
        size: (u32, u32),
        sample_count: u32,
        usage: wgpu::TextureUsages,
        now: Instant,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0.max(1),
                height: size.1.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            used: now,
        }
    }

    /// Hands out the view and marks the target as in use for the frame drawn at `now`.
    fn touch(&mut self, now: Instant) -> wgpu::TextureView {
        self.used = now;
        self.view.clone()
    }
}

/// One of the offscreen targets filters draw through.
#[derive(Clone, Copy)]
enum FilterTarget {
    /// The accumulated background of all open layers before a backdrop samples it.
    Backdrop,
    /// The frame itself, drawn offscreen so a backdrop can sample what lies under it.
    Frame,
    /// The target a filtered layer is drawn into, by nesting depth.
    Layer(usize),
    /// One blur level, the frame's size halved as many times as its index. Level zero holds a
    /// finished gaussian.
    Level(usize),
    /// Where a gaussian keeps its pass across before the pass down.
    Scratch,
}

impl FilterTarget {
    /// How many times smaller than the frame the target is on each side.
    fn shrink(self) -> u32 {
        match self {
            Self::Level(level) => 1 << level,
            Self::Frame | Self::Layer(_) | Self::Scratch | Self::Backdrop => 1,
        }
    }

    fn label(self) -> String {
        match self {
            Self::Backdrop => "backdrop_source".into(),
            Self::Frame => "backdrop_frame".into(),
            Self::Layer(depth) => format!("filter_layer_{depth}"),
            Self::Level(level) => format!("blur_{level}"),
            Self::Scratch => "blur_scratch".into(),
        }
    }
}

/// Offscreen targets used to blur and mask what has already been drawn. Each one is allocated
/// the first time a frame draws through it, so a frame with one shallow blur pays for the
/// targets it touches rather than for every depth and level.
#[derive(Default)]
struct FilterTargets {
    frame: Option<Scratch>,
    backdrop: Option<Scratch>,
    layers: [Option<Scratch>; LAYER_DEPTH],
    levels: [Option<Scratch>; KAWASE_LEVELS + 1],
    scratch: Option<Scratch>,
}

impl FilterTargets {
    fn slot(&mut self, target: FilterTarget) -> &mut Option<Scratch> {
        match target {
            FilterTarget::Backdrop => &mut self.backdrop,
            FilterTarget::Frame => &mut self.frame,
            FilterTarget::Layer(depth) => &mut self.layers[depth],
            FilterTarget::Level(level) => &mut self.levels[level],
            FilterTarget::Scratch => &mut self.scratch,
        }
    }

    fn slots(&mut self) -> impl Iterator<Item = &mut Option<Scratch>> {
        std::iter::once(&mut self.frame)
            .chain(std::iter::once(&mut self.backdrop))
            .chain(self.layers.iter_mut())
            .chain(self.levels.iter_mut())
            .chain(std::iter::once(&mut self.scratch))
    }
}

impl WgpuResources {
    fn invalidate_intermediate_textures(&mut self) {
        let paths = [&mut self.path_intermediate, &mut self.path_msaa];
        for slot in paths.into_iter().chain(self.filter_targets.slots()) {
            if let Some(scratch) = slot.take() {
                scratch.texture.destroy();
            }
        }
    }

    /// Drops every offscreen target that no frame has drawn through within `IDLE_RELEASE` of
    /// `now`. Called after the frame is submitted, so a target the frame used is never idle.
    fn release_targets_idle_at(&mut self, now: Instant) {
        let paths = [&mut self.path_intermediate, &mut self.path_msaa];
        for slot in paths.into_iter().chain(self.filter_targets.slots()) {
            if slot
                .as_ref()
                .is_some_and(|scratch| now.duration_since(scratch.used) >= IDLE_RELEASE)
            {
                *slot = None;
            }
        }
    }
}

struct WgpuRendererCore {
    resources: WgpuResources,
    atlas: Arc<WgpuAtlas>,
    path_globals_offset: u64,
    gamma_offset: u64,
    instance_data_capacity: u64,
    max_instance_data_size: u64,
    instance_data_alignment: u64,
    /// The most instance data any frame has written since `instance_data_window` began.
    instance_data_peak: u64,
    instance_data_window: Instant,
    uses_webgl_instance_data: bool,
    /// When the frame being recorded started, which stamps every offscreen target it touches.
    frame_time: Instant,
    /// The size of the target the frame being recorded draws into. Offscreen targets match it,
    /// so they are dropped whenever it changes.
    frame_size: (u32, u32),
    rendering_params: RenderingParameters,
    is_bgr: bool,
    dual_source_blending: bool,
    adapter_info: wgpu::AdapterInfo,
    target_format: wgpu::TextureFormat,
    max_texture_size: u32,
}

/// GPU resources of a windowed renderer. A surface is only ever configured against the
/// device that owns `core`, so it cannot outlive it: there is no surface-only state.
enum RendererState {
    /// Frames can be drawn.
    Ready {
        surface: wgpu::Surface<'static>,
        core: WgpuRendererCore,
    },
    /// The native surface is gone (Android `TerminateWindow`, browser context loss) but
    /// the device, pipelines, and atlas remain so `replace_surface` can resume without
    /// re-uploading cached textures.
    Unconfigured { core: WgpuRendererCore },
    /// Released by `destroy`, or dropped for a device recovery that has not succeeded yet.
    Released,
}

pub struct WgpuRenderer {
    /// Shared GPU context for device recovery coordination (unused on WASM).
    #[allow(dead_code)]
    context: Option<GpuContext>,
    /// Compositor GPU hint for adapter selection (unused on WASM).
    #[allow(dead_code)]
    compositor_gpu: Option<CompositorGpuHint>,
    /// A frame that `draw_held` rendered and submitted but has not presented. It is declared
    /// before `state` so it drops before the surface it came from.
    held_frame: Option<wgpu::SurfaceTexture>,
    state: RendererState,
    surface_config: wgpu::SurfaceConfiguration,
    atlas: Arc<WgpuAtlas>,
    transparent_alpha_mode: wgpu::CompositeAlphaMode,
    opaque_alpha_mode: wgpu::CompositeAlphaMode,
    max_texture_size: u32,
    is_bgr: bool,
    failed_frame_count: u32,
    device_errors: Arc<DeviceErrorState>,
    observed_error_generation: u64,
    last_surface_error: Option<String>,
    needs_redraw: bool,
}

impl WgpuRenderer {
    fn core(&self) -> Option<&WgpuRendererCore> {
        match &self.state {
            RendererState::Ready { core, .. } | RendererState::Unconfigured { core } => Some(core),
            RendererState::Released => None,
        }
    }

    fn core_mut(&mut self) -> Option<&mut WgpuRendererCore> {
        match &mut self.state {
            RendererState::Ready { core, .. } | RendererState::Unconfigured { core } => Some(core),
            RendererState::Released => None,
        }
    }

    /// Creates a new WgpuRenderer from raw window handles.
    ///
    /// The `gpu_context` is a shared reference that coordinates GPU context across
    /// multiple windows. The first window to create a renderer will initialize the
    /// context; subsequent windows will share it.
    ///
    /// # Safety
    /// The caller must ensure that the window handle remains valid for the lifetime
    /// of the returned renderer.
    #[cfg(not(target_family = "wasm"))]
    pub fn new<W>(
        gpu_context: GpuContext,
        window: &W,
        config: WgpuSurfaceConfig,
        compositor_gpu: Option<CompositorGpuHint>,
    ) -> anyhow::Result<Self>
    where
        W: HasWindowHandle + HasDisplayHandle + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;

        let target = wgpu::SurfaceTargetUnsafe::RawHandle {
            // Fall back to the display handle already provided via InstanceDescriptor::display.
            raw_display_handle: None,
            raw_window_handle: window_handle.as_raw(),
        };

        // Use the existing context's instance if available, otherwise create a new one.
        // The surface must be created with the same instance that will be used for
        // adapter selection, otherwise wgpu will panic.
        let instance = gpu_context
            .borrow()
            .as_ref()
            .map(|ctx| ctx.instance.clone())
            .unwrap_or_else(|| WgpuContext::instance(Some(Box::new(window.clone()))));

        // Safety: The caller guarantees that the window handle is valid for the
        // lifetime of this renderer. In practice, the RawWindow struct is created
        // from the native window handles and the surface is dropped before the window.
        let surface = unsafe {
            instance
                .create_surface_unsafe(target)
                .map_err(|e| anyhow::anyhow!("Failed to create surface: {e}"))?
        };

        let mut ctx_ref = gpu_context.borrow_mut();
        let context = match ctx_ref.as_mut() {
            Some(context) => {
                context.check_compatible_with_surface(&surface)?;
                context
            }
            None => ctx_ref.insert(WgpuContext::new(instance, &surface, compositor_gpu)?),
        };

        let atlas = Arc::new(WgpuAtlas::from_context(context));

        Self::new_internal(
            Some(Rc::clone(&gpu_context)),
            context,
            surface,
            config,
            compositor_gpu,
            atlas,
        )
    }

    #[cfg(target_family = "wasm")]
    pub fn new_from_canvas(
        context: &WgpuContext,
        canvas: &web_sys::HtmlCanvasElement,
        config: WgpuSurfaceConfig,
    ) -> anyhow::Result<Self> {
        let surface = context
            .instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|e| anyhow::anyhow!("Failed to create surface: {e}"))?;
        Self::new_from_surface(context, surface, config)
    }

    #[cfg(target_family = "wasm")]
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new_from_surface(
        context: &WgpuContext,
        surface: wgpu::Surface<'static>,
        config: WgpuSurfaceConfig,
    ) -> anyhow::Result<Self> {
        let atlas = Arc::new(WgpuAtlas::from_context(context));
        Self::new_internal(None, context, surface, config, None, atlas)
    }

    fn new_internal(
        gpu_context: Option<GpuContext>,
        context: &WgpuContext,
        surface: wgpu::Surface<'static>,
        config: WgpuSurfaceConfig,
        compositor_gpu: Option<CompositorGpuHint>,
        atlas: Arc<WgpuAtlas>,
    ) -> anyhow::Result<Self> {
        let surface_caps = surface.get_capabilities(&context.adapter);
        let preferred_formats = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8Unorm,
        ];
        let surface_format = preferred_formats
            .iter()
            .find(|f| surface_caps.formats.contains(f))
            .copied()
            .or_else(|| {
                surface_caps
                    .formats
                    .iter()
                    .find(|format| !format.has_srgb_suffix())
                    .copied()
            })
            .or_else(|| surface_caps.formats.first().copied())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Surface reports no supported texture formats for adapter {:?}",
                    context.adapter.get_info().name
                )
            })?;

        let pick_alpha_mode =
            |preferences: &[wgpu::CompositeAlphaMode]| -> anyhow::Result<wgpu::CompositeAlphaMode> {
                preferences
                    .iter()
                    .find(|p| surface_caps.alpha_modes.contains(p))
                    .copied()
                    .or_else(|| surface_caps.alpha_modes.first().copied())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Surface reports no supported alpha modes for adapter {:?}",
                            context.adapter.get_info().name
                        )
                    })
            };

        let transparent_alpha_mode = pick_alpha_mode(&[
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ])?;

        let opaque_alpha_mode = pick_alpha_mode(&[
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::CompositeAlphaMode::Inherit,
        ])?;

        let alpha_mode = if config.transparent {
            transparent_alpha_mode
        } else {
            opaque_alpha_mode
        };

        let device = Arc::clone(&context.device);
        let max_texture_size = device.limits().max_texture_dimension_2d;

        let requested_width = config.size.width.0 as u32;
        let requested_height = config.size.height.0 as u32;
        let clamped_width = requested_width.min(max_texture_size);
        let clamped_height = requested_height.min(max_texture_size);

        if clamped_width != requested_width || clamped_height != requested_height {
            warn!(
                "Requested surface size ({}, {}) exceeds maximum texture dimension {}. \
                 Clamping to ({}, {}). Window content may not fill the entire window.",
                requested_width, requested_height, max_texture_size, clamped_width, clamped_height
            );
        }

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: clamped_width.max(1),
            height: clamped_height.max(1),
            present_mode: config
                .preferred_present_mode
                .filter(|mode| surface_caps.present_modes.contains(mode))
                .unwrap_or(wgpu::PresentMode::Fifo),
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        // Configure the surface immediately. The adapter selection process already validated
        // that this adapter can successfully configure this surface.
        surface.configure(&context.device, &surface_config);

        let core = WgpuRendererCore::new(context, atlas.clone(), surface_format, alpha_mode);

        Ok(Self {
            context: gpu_context,
            compositor_gpu,
            held_frame: None,
            state: RendererState::Ready { surface, core },
            surface_config,
            atlas,
            transparent_alpha_mode,
            opaque_alpha_mode,
            max_texture_size,
            is_bgr: false,
            failed_frame_count: 0,
            device_errors: Arc::clone(context.errors()),
            observed_error_generation: 0,
            last_surface_error: None,
            needs_redraw: false,
        })
    }
}

impl WgpuRendererCore {
    fn create_bind_group_layouts(
        device: &wgpu::Device,
        uses_webgl_instance_data: bool,
    ) -> WgpuBindGroupLayouts {
        let globals =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("globals_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<GlobalParams>() as u64
                            ),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                std::mem::size_of::<GammaParams>() as u64
                            ),
                        },
                        count: None,
                    },
                ],
            });

        let instance_data_entry = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: if uses_webgl_instance_data {
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                }
            } else {
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                }
            },
            count: None,
        };

        let instances = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("instances_layout"),
            entries: &[instance_data_entry],
        });

        let texture = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let surfaces = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("surfaces_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<SurfaceParams>() as u64
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        WgpuBindGroupLayouts {
            globals,
            instances,
            texture,
            surfaces,
        }
    }

    fn create_instance_texture(
        device: &wgpu::Device,
        requested_capacity: u64,
        max_texture_dimension: u32,
    ) -> (InstanceData, u64) {
        let texel_count = requested_capacity.div_ceil(INSTANCE_TEXTURE_TEXEL_SIZE);
        let width = texel_count.min(u64::from(max_texture_dimension)).max(1) as u32;
        let height = texel_count
            .div_ceil(u64::from(width))
            .min(u64::from(max_texture_dimension))
            .max(1) as u32;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("instance_texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let capacity = u64::from(width) * u64::from(height) * INSTANCE_TEXTURE_TEXEL_SIZE;
        (
            InstanceData::Texture {
                texture,
                view,
                width,
                height,
            },
            capacity,
        )
    }

    fn create_pipelines(
        device: &wgpu::Device,
        layouts: &WgpuBindGroupLayouts,
        surface_format: wgpu::TextureFormat,
        alpha_mode: wgpu::CompositeAlphaMode,
        path_sample_count: u32,
        dual_source_blending: bool,
        uses_webgl_instance_data: bool,
    ) -> WgpuPipelines {
        // Diagnostic guard: verify the device actually has
        // DUAL_SOURCE_BLENDING. We have a crash report (ZED-5G1) where a
        // feature mismatch caused a wgpu-hal abort, but we haven't
        // identified the code path that produces the mismatch. This
        // guard prevents the crash and logs more evidence.
        // Remove this check once:
        // a) We find and fix the root cause, or
        // b) There are no reports of this warning appearing for some time.
        let device_has_feature = device
            .features()
            .contains(wgpu::Features::DUAL_SOURCE_BLENDING);
        if dual_source_blending && !device_has_feature {
            log::error!(
                "BUG: dual_source_blending flag is true but device does not \
                 have DUAL_SOURCE_BLENDING enabled (device features: {:?}). \
                 Falling back to mono text rendering. Please report this at \
                 https://github.com/zed-industries/zed/issues",
                device.features(),
            );
        }
        let dual_source_blending =
            dual_source_blending && device_has_feature && !uses_webgl_instance_data;

        let shader_source = if uses_webgl_instance_data {
            WEBGL_SHADERS
        } else {
            STORAGE_BUFFER_SHADERS
        };
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpui_shaders"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let subpixel_shader_module = if dual_source_blending {
            Some(device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("gpui_subpixel_shaders"),
                source: wgpu::ShaderSource::Wgsl(SUBPIXEL_SHADERS.into()),
            }))
        } else {
            None
        };

        let blend_mode = match alpha_mode {
            wgpu::CompositeAlphaMode::PreMultiplied => {
                wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
            }
            _ => wgpu::BlendState::ALPHA_BLENDING,
        };

        let color_target = wgpu::ColorTargetState {
            format: surface_format,
            blend: Some(blend_mode),
            write_mask: wgpu::ColorWrites::ALL,
        };

        // Offscreen layers and the frame itself are drawn onto a transparent clear, so what they
        // hold is already premultiplied however the surface wants its colours.
        let premultiplied_target = wgpu::ColorTargetState {
            format: surface_format,
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        };

        let create_pipeline = |name: &str,
                               vs_entry: &str,
                               fs_entry: &str,
                               globals_layout: &wgpu::BindGroupLayout,
                               data_layout: &wgpu::BindGroupLayout,
                               texture_layout: Option<&wgpu::BindGroupLayout>,
                               topology: wgpu::PrimitiveTopology,
                               color_targets: &[Option<wgpu::ColorTargetState>],
                               sample_count: u32,
                               module: &wgpu::ShaderModule| {
            let mut bind_group_layouts = vec![Some(globals_layout), Some(data_layout)];
            bind_group_layouts.extend(texture_layout.map(Some));
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(&format!("{name}_layout")),
                bind_group_layouts: &bind_group_layouts,
                immediate_size: 0,
            });

            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vs_entry),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs_entry),
                    targets: color_targets,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview_mask: None,
                cache: None,
            })
        };

        // A backdrop mixes the blurred copy into what it covers instead of laying it over:
        // the punch pass scales the destination by `1 - k` and the draw over it adds
        // `blurred * k`. Laying it over cost a see-through window its transparency, since a
        // copy of alpha `a` over the same `a` leaves `a + a(1 - a)`.
        let backdrop_punch_target = wgpu::ColorTargetState {
            format: surface_format,
            blend: Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
            write_mask: wgpu::ColorWrites::ALL,
        };

        let backdrop_target = wgpu::ColorTargetState {
            format: surface_format,
            blend: Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
            write_mask: wgpu::ColorWrites::ALL,
        };

        let backdrop_punch = create_pipeline(
            "backdrop_punch",
            "vs_backdrop",
            "fs_backdrop_punch",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(backdrop_punch_target)],
            1,
            &shader_module,
        );

        let backdrops = create_pipeline(
            "backdrops",
            "vs_backdrop",
            "fs_backdrop",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(backdrop_target)],
            1,
            &shader_module,
        );

        let opaque_target = wgpu::ColorTargetState {
            format: surface_format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        };

        let gaussian = create_pipeline(
            "gaussian",
            "vs_blur",
            "fs_gaussian",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(opaque_target.clone())],
            1,
            &shader_module,
        );

        let kawase_down = create_pipeline(
            "kawase_down",
            "vs_blur",
            "fs_kawase_down",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(opaque_target.clone())],
            1,
            &shader_module,
        );

        let kawase_up = create_pipeline(
            "kawase_up",
            "vs_blur",
            "fs_kawase_up",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(opaque_target.clone())],
            1,
            &shader_module,
        );

        let blit = create_pipeline(
            "blit",
            "vs_blur",
            "fs_blit",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(opaque_target)],
            1,
            &shader_module,
        );

        let composite = create_pipeline(
            "composite",
            "vs_blur",
            "fs_mask",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(premultiplied_target.clone())],
            1,
            &shader_module,
        );

        let masked = create_pipeline(
            "masked",
            "vs_blur",
            "fs_mask",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(premultiplied_target.clone())],
            1,
            &shader_module,
        );

        let blurred = create_pipeline(
            "blurred",
            "vs_blur",
            "fs_mask_gaussian",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(premultiplied_target)],
            1,
            &shader_module,
        );

        let quads = create_pipeline(
            "quads",
            "vs_quad",
            "fs_quad",
            &layouts.globals,
            &layouts.instances,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let shadows = create_pipeline(
            "shadows",
            "vs_shadow",
            "fs_shadow",
            &layouts.globals,
            &layouts.instances,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let path_rasterization = create_pipeline(
            "path_rasterization",
            "vs_path_rasterization",
            "fs_path_rasterization",
            &layouts.globals,
            &layouts.instances,
            None,
            wgpu::PrimitiveTopology::TriangleList,
            &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            path_sample_count,
            &shader_module,
        );

        let paths = create_pipeline(
            "paths",
            "vs_path",
            "fs_path",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            1,
            &shader_module,
        );

        let underlines = create_pipeline(
            "underlines",
            "vs_underline",
            "fs_underline",
            &layouts.globals,
            &layouts.instances,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let mono_sprites = create_pipeline(
            "mono_sprites",
            "vs_mono_sprite",
            "fs_mono_sprite",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let subpixel_sprites = if let Some(subpixel_module) = &subpixel_shader_module {
            let subpixel_blend = wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Src1,
                    dst_factor: wgpu::BlendFactor::OneMinusSrc1,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            };

            Some(create_pipeline(
                "subpixel_sprites",
                "vs_subpixel_sprite",
                "fs_subpixel_sprite",
                &layouts.globals,
                &layouts.instances,
                Some(&layouts.texture),
                wgpu::PrimitiveTopology::TriangleStrip,
                &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(subpixel_blend),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
                1,
                subpixel_module,
            ))
        } else {
            None
        };

        let subpixel_sprites_layered = subpixel_shader_module.as_ref().map(|subpixel_module| {
            create_pipeline(
                "subpixel_sprites_layered",
                "vs_subpixel_sprite",
                "fs_subpixel_sprite_layered",
                &layouts.globals,
                &layouts.instances,
                Some(&layouts.texture),
                wgpu::PrimitiveTopology::TriangleStrip,
                &[Some(color_target.clone())],
                1,
                subpixel_module,
            )
        });

        let poly_sprites = create_pipeline(
            "poly_sprites",
            "vs_poly_sprite",
            "fs_poly_sprite",
            &layouts.globals,
            &layouts.instances,
            Some(&layouts.texture),
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        let surfaces = create_pipeline(
            "surfaces",
            "vs_surface",
            "fs_surface",
            &layouts.globals,
            &layouts.surfaces,
            None,
            wgpu::PrimitiveTopology::TriangleStrip,
            &[Some(color_target.clone())],
            1,
            &shader_module,
        );

        WgpuPipelines {
            backdrops,
            backdrop_punch,
            gaussian,
            kawase_down,
            kawase_up,
            blit,
            composite,
            masked,
            blurred,
            quads,
            shadows,
            path_rasterization,
            paths,
            underlines,
            mono_sprites,
            subpixel_sprites,
            subpixel_sprites_layered,
            poly_sprites,
            surfaces,
            custom_target: color_target,
            custom: FxHashMap::default(),
        }
    }

    fn resume_pass<'a>(
        encoder: &'a mut wgpu::CommandEncoder,
        label: &str,
        view: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
    ) -> wgpu::RenderPass<'a> {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        })
    }

    /// A bounds rectangle as a scissor for a target shrunk by the given factor.
    fn scissor(&self, bounds: Bounds<ScaledPixels>, shrink: u32) -> [u32; 4] {
        let shrink = shrink as f32;
        let width = (self.frame_size.0 as f32 / shrink).floor();
        let height = (self.frame_size.1 as f32 / shrink).floor();
        let left = (bounds.origin.x.0 / shrink).floor().clamp(0., width);
        let top = (bounds.origin.y.0 / shrink).floor().clamp(0., height);
        let right = ((bounds.origin.x.0 + bounds.size.width.0) / shrink)
            .ceil()
            .clamp(left, width);
        let bottom = ((bounds.origin.y.0 + bounds.size.height.0) / shrink)
            .ceil()
            .clamp(top, height);

        [
            left as u32,
            top as u32,
            (right - left) as u32,
            (bottom - top) as u32,
        ]
    }

    /// Draws a filtered layer back into its parent, clipped to the layer's bounds.
    fn composite_layer(
        &self,
        source: &wgpu::TextureView,
        bounds: Bounds<ScaledPixels>,
        composite: Composite,
        instances: &InstanceBinding,
        pass: &mut wgpu::RenderPass<'_>,
    ) {
        let width = self.frame_size.0 as f32;
        let height = self.frame_size.1 as f32;
        let left = bounds.origin.x.0.max(0.).min(width);
        let top = bounds.origin.y.0.max(0.).min(height);
        let right = (bounds.origin.x.0 + bounds.size.width.0).max(0.).min(width);
        let bottom = (bounds.origin.y.0 + bounds.size.height.0)
            .max(0.)
            .min(height);
        if right <= left || bottom <= top {
            return;
        }

        let texture = self.create_texture_bind_group("layer_bind_group", source);
        let resources = self.resources();
        pass.set_pipeline(match composite {
            Composite::Plain => &resources.pipelines.composite,
            Composite::Masked => &resources.pipelines.masked,
            Composite::Blurred => &resources.pipelines.blurred,
        });
        pass.set_bind_group(0, &self.resources().globals_bind_group, &[]);
        pass.set_bind_group(1, &instances.bind_group, &[]);
        pass.set_bind_group(2, &texture, &[]);
        pass.set_scissor_rect(
            left as u32,
            top as u32,
            (right - left) as u32,
            (bottom - top) as u32,
        );
        pass.draw(0..4, instances.first_instance..instances.first_instance + 1);
        pass.set_scissor_rect(0, 0, self.frame_size.0, self.frame_size.1);
    }

    /// Flattens open layers in source coordinates so nested backdrops can sample their ancestors.
    /// The original targets stay unchanged for their eventual filtered composites.
    fn backdrop_source(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::TextureView,
        depth: usize,
        clip: Option<Bounds<ScaledPixels>>,
        instance_offset: &mut u64,
    ) -> Result<wgpu::TextureView> {
        if depth == 0 {
            return Ok(frame.clone());
        }
        let target = self.filter_view(FilterTarget::Backdrop);
        let bounds = [0., 0., self.frame_size.0 as f32, self.frame_size.1 as f32];
        let params = self.write_instance_binding(
            "backdrop_source_params",
            instance_offset,
            &[MaskParams {
                source_bounds: bounds,
                fade_bounds: bounds,
                scale: 1.,
                ..Default::default()
            }],
        )?;
        let mut sources = vec![frame.clone()];
        sources.extend((0..depth).map(|level| self.filter_view(FilterTarget::Layer(level))));
        let clip = clip.unwrap_or_else(|| {
            Bounds::new(
                point(ScaledPixels(0.), ScaledPixels(0.)),
                size(ScaledPixels(bounds[2]), ScaledPixels(bounds[3])),
            )
        });
        let mut pass = Self::resume_pass(
            encoder,
            "backdrop_source",
            &target,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        for source in sources {
            self.composite_layer(&source, clip, Composite::Plain, &params, &mut pass);
        }
        drop(pass);
        Ok(target)
    }

    /// Blurs a source and returns the target holding the result. A narrow blur comes back at
    /// full size, a wide one at the level [`Kawase::settled`] names, and whatever samples it
    /// stretches it back up. A `spare` source is a layer nothing reads again, so a gaussian
    /// writes its result back into it rather than into a full-size target of its own.
    fn blur_source(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        spare: bool,
        sigma: f32,
        clip: Option<Bounds<ScaledPixels>>,
        instance_offset: &mut u64,
    ) -> Result<wgpu::TextureView> {
        match BlurPasses::for_sigma(sigma) {
            BlurPasses::Gaussian(sigma) => {
                self.gaussian_blur(encoder, source, spare, sigma, clip, instance_offset)
            }
            BlurPasses::Kawase(kawase) => {
                self.kawase_blur(encoder, source, kawase, clip, instance_offset)
            }
        }
    }

    /// Blurs a source across into the scratch target and down into level zero, or back into a
    /// `spare` source, both at full size. The pass across covers the clip grown by the kernel,
    /// since the pass down reads that far above and below it.
    fn gaussian_blur(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        spare: bool,
        sigma: f32,
        clip: Option<Bounds<ScaledPixels>>,
        instance_offset: &mut u64,
    ) -> Result<wgpu::TextureView> {
        let scratch = self.gaussian_across(encoder, source, sigma, clip, instance_offset)?;
        let down = self.write_instance_binding(
            "gaussian_down_bind_group",
            instance_offset,
            &[BlurParams {
                direction: [0., 1.],
                radius: sigma,
                pad: 0,
            }],
        )?;
        let settled = match spare {
            true => source.clone(),
            false => self.filter_view(FilterTarget::Level(0)),
        };
        let gaussian = self.resources().pipelines.gaussian.clone();
        let within = clip.map(|clip| self.scissor(clip.dilate(ScaledPixels(1.)), 1));
        self.fullscreen_pass(
            encoder,
            "gaussian_down",
            &gaussian,
            &scratch,
            &settled,
            &down,
            within,
        );
        Ok(settled)
    }

    /// Blurs a source across into the scratch target and returns it. The pass covers the clip
    /// grown by the kernel, since the pass down reads that far above and below it.
    fn gaussian_across(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        sigma: f32,
        clip: Option<Bounds<ScaledPixels>>,
        instance_offset: &mut u64,
    ) -> Result<wgpu::TextureView> {
        let across = self.write_instance_binding(
            "gaussian_across_bind_group",
            instance_offset,
            &[BlurParams {
                direction: [1., 0.],
                radius: sigma,
                pad: 0,
            }],
        )?;
        let scratch = self.filter_view(FilterTarget::Scratch);
        let gaussian = self.resources().pipelines.gaussian.clone();
        let reach = (sigma * GAUSSIAN_REACH).ceil() + 1.;
        let within = clip.map(|clip| self.scissor(clip.dilate(ScaledPixels(reach)), 1));
        self.fullscreen_pass(
            encoder,
            "gaussian_across",
            &gaussian,
            source,
            &scratch,
            &across,
            within,
        );
        Ok(scratch)
    }

    /// Blurs a source with a dual Kawase blur and returns the level it settled at.
    fn kawase_blur(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        kawase: Kawase,
        clip: Option<Bounds<ScaledPixels>>,
        instance_offset: &mut u64,
    ) -> Result<wgpu::TextureView> {
        let params = self.write_instance_binding(
            "kawase_bind_group",
            instance_offset,
            &[BlurParams {
                direction: [0., 0.],
                radius: kawase.offset,
                pad: 0,
            }],
        )?;
        let levels: Vec<wgpu::TextureView> = (1..=kawase.levels)
            .map(|level| self.filter_view(FilterTarget::Level(level)))
            .collect();
        let down = self.resources().pipelines.kawase_down.clone();
        let up = self.resources().pipelines.kawase_up.clone();

        // Every pass covers the clip grown by what all the passes together read. The targets are
        // never cleared, so a pass reads stale texels past the edge of what the one before it
        // wrote, but they only creep inwards by what the later passes read, which this margin
        // already holds. A texel more keeps the final bilinear sample on the clip's edge clean.
        let reach = kawase.reach();
        let within = |level: usize| {
            let shrink = 1u32 << level;
            clip.map(|clip| self.scissor(clip.dilate(ScaledPixels(reach + shrink as f32)), shrink))
        };

        let mut from = source;
        for (level, to) in (1..).zip(&levels) {
            self.fullscreen_pass(
                encoder,
                "kawase_down",
                &down,
                from,
                to,
                &params,
                within(level),
            );
            from = to;
        }
        let settled = &levels[kawase.settled - 1];
        self.fullscreen_pass(
            encoder,
            "kawase_up",
            &up,
            from,
            settled,
            &params,
            within(kawase.settled),
        );
        Ok(settled.clone())
    }

    /// Draws one full-screen pass from `source` into `target`, within `within` when it is given.
    /// The target is loaded rather than cleared, since the pass writes every texel it covers.
    fn fullscreen_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        pipeline: &wgpu::RenderPipeline,
        source: &wgpu::TextureView,
        target: &wgpu::TextureView,
        instances: &InstanceBinding,
        within: Option<[u32; 4]>,
    ) {
        let texture = self.create_texture_bind_group("backdrop_texture_bind_group", source);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        });

        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.resources().globals_bind_group, &[]);
        pass.set_bind_group(1, &instances.bind_group, &[]);
        pass.set_bind_group(2, &texture, &[]);
        if let Some([left, top, width, height]) = within {
            pass.set_scissor_rect(left, top, width, height);
        }
        pass.draw(0..4, instances.first_instance..instances.first_instance + 1);
    }

    fn draw_backdrops(
        &self,
        binding: &InstanceBinding,
        blurred: &wgpu::TextureView,
        range: Range<u32>,
        pass: &mut wgpu::RenderPass<'_>,
    ) {
        if range.is_empty() {
            return;
        }
        let texture = self.create_texture_bind_group("backdrop_blurred_bind_group", blurred);
        let instances = binding.first_instance + range.start..binding.first_instance + range.end;
        pass.set_bind_group(0, &self.resources().globals_bind_group, &[]);
        pass.set_bind_group(1, &binding.bind_group, &[]);
        pass.set_bind_group(2, &texture, &[]);
        // The hole first, the blurred copy into it second: together they are
        // `dst = mix(dst, blurred, k)`, which leaves a see-through window as clear as it was.
        pass.set_pipeline(&self.resources().pipelines.backdrop_punch);
        pass.draw(0..4, instances.clone());
        pass.set_pipeline(&self.resources().pipelines.backdrops);
        pass.draw(0..4, instances);
    }

    /// The view of one filter target, created at the surface's size (shrunk for a blur level) if
    /// no frame has drawn through it since it was last released.
    fn filter_view(&mut self, target: FilterTarget) -> wgpu::TextureView {
        let format = self.target_format;
        let size = (
            self.frame_size.0.max(1) / target.shrink(),
            self.frame_size.1.max(1) / target.shrink(),
        );
        let now = self.frame_time;
        let resources = self.resources_mut();
        let device = &resources.device;
        resources
            .filter_targets
            .slot(target)
            .get_or_insert_with(|| {
                Scratch::new(
                    device,
                    &target.label(),
                    format,
                    size,
                    1,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    now,
                )
            })
            .touch(now)
    }

    /// The path intermediate and, when paths are multisampled, the target they are rasterized
    /// into, created if no frame has drawn a path since they were last released.
    fn path_views(&mut self) -> (wgpu::TextureView, Option<wgpu::TextureView>) {
        let format = self.target_format;
        let size = (self.frame_size.0, self.frame_size.1);
        let sample_count = self.rendering_params.path_sample_count;
        let now = self.frame_time;
        let resources = self.resources_mut();
        let device = &resources.device;
        let intermediate = resources
            .path_intermediate
            .get_or_insert_with(|| {
                Scratch::new(
                    device,
                    "path_intermediate",
                    format,
                    size,
                    1,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    now,
                )
            })
            .touch(now);
        // The samples are resolved into the intermediate and never read again, so the driver
        // may keep them in tile memory where it has any.
        let msaa = (sample_count > 1).then(|| {
            resources
                .path_msaa
                .get_or_insert_with(|| {
                    Scratch::new(
                        device,
                        "path_msaa",
                        format,
                        size,
                        sample_count,
                        wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TRANSIENT_ATTACHMENT,
                        now,
                    )
                })
                .touch(now)
        });
        (intermediate, msaa)
    }
}

impl WgpuRenderer {
    /// Records the new drawable size and reconfigures the surface when one is present.
    /// The size is kept even without GPU resources so that recovery restores it.
    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        let width = size.width.0 as u32;
        let height = size.height.0 as u32;

        if width == self.surface_config.width && height == self.surface_config.height {
            return;
        }

        let clamped_width = width.min(self.max_texture_size);
        let clamped_height = height.min(self.max_texture_size);
        if clamped_width != width || clamped_height != height {
            warn!(
                "Requested surface size ({}, {}) exceeds maximum texture dimension {}. \
                 Clamping to ({}, {}). Window content may not fill the entire window.",
                width, height, self.max_texture_size, clamped_width, clamped_height
            );
        }
        self.surface_config.width = clamped_width.max(1);
        self.surface_config.height = clamped_height.max(1);
        self.discard_held_frame();

        let Some(core) = self.core_mut() else {
            return;
        };
        let resources = &mut core.resources;

        // Wait for any in-flight GPU work to complete before destroying textures
        if let Err(e) = resources.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        }) {
            warn!("Failed to poll device during resize: {e:?}");
        }

        // Destroy the old targets before configuring the surface to avoid GPU memory spikes.
        // The next frame that needs them recreates them at the new size.
        resources.invalidate_intermediate_textures();

        if let RendererState::Ready { surface, core } = &self.state {
            surface.configure(&core.resources.device, &self.surface_config);
        }
    }

    pub fn set_subpixel_layout(&mut self, is_bgr: bool) {
        self.is_bgr = is_bgr;
        if let Some(core) = self.core_mut() {
            core.is_bgr = is_bgr;
        }
    }

    pub fn update_transparency(&mut self, transparent: bool) {
        let new_alpha_mode = if transparent {
            self.transparent_alpha_mode
        } else {
            self.opaque_alpha_mode
        };
        if new_alpha_mode == self.surface_config.alpha_mode {
            return;
        }
        self.surface_config.alpha_mode = new_alpha_mode;
        let format = self.surface_config.format;
        self.discard_held_frame();

        let Some(core) = self.core_mut() else {
            return;
        };
        let resources = &mut core.resources;
        resources.pipelines = WgpuRendererCore::create_pipelines(
            &resources.device,
            &resources.bind_group_layouts,
            format,
            new_alpha_mode,
            core.rendering_params.path_sample_count,
            core.dual_source_blending,
            core.uses_webgl_instance_data,
        );

        if let RendererState::Ready { surface, core } = &self.state {
            surface.configure(&core.resources.device, &self.surface_config);
        }
    }

    #[allow(dead_code)]
    pub fn viewport_size(&self) -> Size<DevicePixels> {
        Size {
            width: DevicePixels(self.surface_config.width as i32),
            height: DevicePixels(self.surface_config.height as i32),
        }
    }

    pub fn sprite_atlas(&self) -> &Arc<WgpuAtlas> {
        &self.atlas
    }

    pub fn supports_dual_source_blending(&self) -> bool {
        self.core().is_some_and(|core| core.dual_source_blending)
    }

    /// Returns `None` once GPU resources have been released by `destroy` or a pending
    /// device recovery.
    pub fn gpu_specs(&self) -> Option<GpuSpecs> {
        let adapter_info = &self.core()?.adapter_info;
        Some(GpuSpecs {
            is_software_emulated: adapter_info.device_type == wgpu::DeviceType::Cpu,
            device_name: adapter_info.name.clone(),
            driver_name: adapter_info.driver.clone(),
            driver_info: adapter_info.driver_info.clone(),
        })
    }

    pub fn max_texture_size(&self) -> u32 {
        self.max_texture_size
    }

    /// Renders the scene and presents it at once. Returns false when no frame reached the
    /// surface, in which case the caller should try again later.
    pub fn draw(&mut self, scene: &Scene) -> bool {
        self.held_frame = None;
        let Some(frame) = self.render(scene) else {
            return false;
        };
        self.present(frame);
        true
    }

    /// Renders the scene and submits its GPU work but keeps the frame back until
    /// [`Self::present_held`]. Drawing ahead like this gives a frame the whole interval up to
    /// its present, rather than whatever the compositor leaves after it asks for one.
    pub fn draw_held(&mut self, scene: &Scene) -> bool {
        self.held_frame = None;
        let Some(frame) = self.render(scene) else {
            return false;
        };
        self.held_frame = Some(frame);
        true
    }

    /// Whether a frame from [`Self::draw_held`] is waiting to be presented.
    pub fn has_held_frame(&self) -> bool {
        self.held_frame.is_some()
    }

    /// Presents the frame that [`Self::draw_held`] kept back. Returns false when there was none.
    pub fn present_held(&mut self) -> bool {
        let Some(frame) = self.held_frame.take() else {
            return false;
        };
        self.present(frame);
        true
    }

    /// Drops a held frame unpresented, because the surface is about to change under it. The
    /// next [`Self::needs_redraw`] then asks for a new one.
    fn discard_held_frame(&mut self) {
        if self.held_frame.take().is_some() {
            self.needs_redraw = true;
        }
    }

    fn present(&mut self, frame: wgpu::SurfaceTexture) {
        let RendererState::Ready { core, .. } = &self.state else {
            return;
        };
        let present_start = gpui::frame_stats::start();
        core.resources.queue.present(frame);
        gpui::frame_stats::record(gpui::frame_stats::Phase::QueuePresent, present_start);
        gpui::frame_stats::mark_presented();
    }

    /// Acquires a surface texture, renders the scene into it and submits the work. Returns
    /// None when the surface could not give a texture or the frame failed.
    fn render(&mut self, scene: &Scene) -> Option<wgpu::SurfaceTexture> {
        #[cfg(target_family = "wasm")]
        if self.device_lost() {
            if matches!(self.state, RendererState::Ready { .. }) {
                log::error!(
                    "Browser graphics context was lost; rendering has stopped. Reload the page to recover."
                );
                self.unconfigure_surface();
            }
            return None;
        }

        // Bail out early if the surface has been unconfigured (e.g. during
        // Android background/rotation transitions).  Attempting to acquire
        // a texture from an unconfigured surface can block indefinitely on
        // some drivers (Adreno).
        let RendererState::Ready { surface, core } = &mut self.state else {
            return None;
        };

        if let Some(error) = self.last_surface_error.take().or_else(|| {
            self.device_errors
                .observe_error(&mut self.observed_error_generation)
        }) {
            self.failed_frame_count += 1;
            log::error!(
                "GPU error during frame (failure {} of 10): {error}",
                self.failed_frame_count
            );

            // TBD. Does retrying more actually help?
            if self.failed_frame_count > 10 {
                panic!("Too many consecutive GPU errors. Last error: {error}");
            } else if self.failed_frame_count > 5 {
                core.resources.invalidate_intermediate_textures();
                self.atlas.clear();
                self.needs_redraw = true;
                self.failed_frame_count = 0;
                return None;
            }
        } else {
            self.failed_frame_count = 0;
        }

        let acquire_start = gpui::frame_stats::start();
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                // Textures must be destroyed before the surface can be reconfigured.
                drop(frame);
                surface.configure(&core.resources.device, &self.surface_config);
                return None;
            }
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                surface.configure(&core.resources.device, &self.surface_config);
                return None;
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return None;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                self.last_surface_error = Some("Surface texture validation error".to_string());
                return None;
            }
        };

        gpui::frame_stats::record(gpui::frame_stats::Phase::Acquire, acquire_start);
        // The acquired texture is the authority on frame dimensions; the surface
        // configuration is only a request.
        let size = Size {
            width: DevicePixels(frame.texture.width() as i32),
            height: DevicePixels(frame.texture.height() as i32),
        };
        let frame_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let premultiplied_alpha =
            self.surface_config.alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied;
        let encode_start = gpui::frame_stats::start();
        if let Err(error) = core.render_frame(
            scene,
            &frame_view,
            size,
            premultiplied_alpha,
            wgpu::Color::TRANSPARENT,
        ) {
            log::error!("{error:#}");
            return None;
        }

        gpui::frame_stats::record(gpui::frame_stats::Phase::Encode, encode_start);
        Some(frame)
    }
}

impl WgpuRendererCore {
    fn new(
        context: &WgpuContext,
        atlas: Arc<WgpuAtlas>,
        target_format: wgpu::TextureFormat,
        alpha_mode: wgpu::CompositeAlphaMode,
    ) -> Self {
        let device = Arc::clone(&context.device);
        let queue = Arc::clone(&context.queue);
        let rendering_params = RenderingParameters::new(&context.adapter, target_format);
        let uses_webgl_instance_data = context.uses_webgl_instance_data();
        let dual_source_blending =
            context.supports_dual_source_blending() && !uses_webgl_instance_data;
        let bind_group_layouts = Self::create_bind_group_layouts(&device, uses_webgl_instance_data);
        let pipelines = Self::create_pipelines(
            &device,
            &bind_group_layouts,
            target_format,
            alpha_mode,
            rendering_params.path_sample_count,
            dual_source_blending,
            uses_webgl_instance_data,
        );
        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform_alignment = device.limits().min_uniform_buffer_offset_alignment as u64;
        let globals_size = std::mem::size_of::<GlobalParams>() as u64;
        let gamma_size = std::mem::size_of::<GammaParams>() as u64;
        let path_globals_offset = globals_size.next_multiple_of(uniform_alignment);
        let gamma_offset = (path_globals_offset + globals_size).next_multiple_of(uniform_alignment);
        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals_buffer"),
            size: gamma_offset + gamma_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (
            instance_data,
            instance_data_capacity,
            max_instance_data_size,
            instance_data_alignment,
        ) = if uses_webgl_instance_data {
            let max_texture_dimension = device.limits().max_texture_dimension_2d;
            let max_instance_data_size = (u64::from(max_texture_dimension).pow(2)
                * INSTANCE_TEXTURE_TEXEL_SIZE)
                .min(MAX_INSTANCE_BUFFER_SIZE);
            let initial_capacity = INITIAL_INSTANCE_DATA_SIZE.min(max_instance_data_size);
            let (instance_data, capacity) =
                Self::create_instance_texture(&device, initial_capacity, max_texture_dimension);
            (
                instance_data,
                capacity,
                max_instance_data_size,
                INSTANCE_TEXTURE_TEXEL_SIZE,
            )
        } else {
            let max_buffer_size = device
                .limits()
                .max_buffer_size
                .min(device.limits().max_storage_buffer_binding_size)
                .min(MAX_INSTANCE_BUFFER_SIZE);
            let initial_capacity = INITIAL_INSTANCE_DATA_SIZE.min(max_buffer_size);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instance_buffer"),
                size: initial_capacity,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            (
                InstanceData::Storage(buffer),
                initial_capacity,
                max_buffer_size,
                device.limits().min_storage_buffer_offset_alignment as u64,
            )
        };
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals_bind_group"),
            layout: &bind_group_layouts.globals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buffer,
                        offset: 0,
                        size: NonZeroU64::new(globals_size),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buffer,
                        offset: gamma_offset,
                        size: NonZeroU64::new(gamma_size),
                    }),
                },
            ],
        });
        let path_globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("path_globals_bind_group"),
            layout: &bind_group_layouts.globals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buffer,
                        offset: path_globals_offset,
                        size: NonZeroU64::new(globals_size),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &globals_buffer,
                        offset: gamma_offset,
                        size: NonZeroU64::new(gamma_size),
                    }),
                },
            ],
        });
        let max_texture_size = device.limits().max_texture_dimension_2d;

        Self {
            resources: WgpuResources {
                device,
                queue,
                pipelines,
                bind_group_layouts,
                atlas_sampler,
                atlas_texture_bind_groups: FxHashMap::default(),
                globals_buffer,
                globals_bind_group,
                path_globals_bind_group,
                instance_data,
                // Offscreen targets are created by the first frame that draws through them,
                // once the target is known to be healthy, and released again when they go unused.
                path_intermediate: None,
                path_msaa: None,
                filter_targets: FilterTargets::default(),
            },
            atlas,
            path_globals_offset,
            gamma_offset,
            instance_data_capacity,
            max_instance_data_size,
            instance_data_alignment,
            instance_data_peak: 0,
            instance_data_window: Instant::now(),
            uses_webgl_instance_data,
            frame_time: Instant::now(),
            frame_size: (0, 0),
            rendering_params,
            is_bgr: false,
            dual_source_blending,
            adapter_info: context.adapter.get_info(),
            target_format,
            max_texture_size,
        }
    }

    fn resources(&self) -> &WgpuResources {
        &self.resources
    }

    fn resources_mut(&mut self) -> &mut WgpuResources {
        &mut self.resources
    }

    fn render_frame(
        &mut self,
        scene: &Scene,
        target_view: &wgpu::TextureView,
        size: Size<DevicePixels>,
        premultiplied_alpha: bool,
        clear_color: wgpu::Color,
    ) -> Result<wgpu::SubmissionIndex> {
        anyhow::ensure!(
            size.width.0 > 0 && size.height.0 > 0,
            "invalid render target size: {size:?}"
        );
        anyhow::ensure!(
            size.width.0 as u32 <= self.max_texture_size
                && size.height.0 as u32 <= self.max_texture_size,
            "render target size {size:?} exceeds maximum texture dimension {}",
            self.max_texture_size
        );

        let frame_size = (size.width.0 as u32, size.height.0 as u32);
        if frame_size != self.frame_size {
            self.resources.invalidate_intermediate_textures();
            self.frame_size = frame_size;
        }
        self.frame_time = Instant::now();

        let gamma_params = GammaParams {
            gamma_ratios: self.rendering_params.gamma_ratios,
            grayscale_enhanced_contrast: self.rendering_params.grayscale_enhanced_contrast,
            subpixel_enhanced_contrast: self.rendering_params.subpixel_enhanced_contrast,
            is_bgr: self.is_bgr as u32,
            _pad: 0,
        };
        let globals = GlobalParams {
            viewport_size: [size.width.0 as f32, size.height.0 as f32],
            premultiplied_alpha: premultiplied_alpha as u32,
            pad: 0,
        };
        let path_globals = GlobalParams {
            premultiplied_alpha: 0,
            ..globals
        };
        self.resources.queue.write_buffer(
            &self.resources.globals_buffer,
            0,
            bytemuck::bytes_of(&globals),
        );
        self.resources.queue.write_buffer(
            &self.resources.globals_buffer,
            self.path_globals_offset,
            bytemuck::bytes_of(&path_globals),
        );
        self.resources.queue.write_buffer(
            &self.resources.globals_buffer,
            self.gamma_offset,
            bytemuck::bytes_of(&gamma_params),
        );

        self.record_frame(scene, target_view, clear_color)
            .inspect_err(|_| {
                // Queue writes are staged before encoding; flush them even if the frame fails.
                self.resources.queue.submit(std::iter::empty());
            })
    }

    fn record_frame(
        &mut self,
        scene: &Scene,
        frame_view: &wgpu::TextureView,
        clear_color: wgpu::Color,
    ) -> Result<wgpu::SubmissionIndex> {
        let mut instance_offset = 0;
        let filtered = !(scene.backdrops.is_empty() && scene.effects.is_empty())
            && !self.uses_webgl_instance_data;
        let instance_bindings = self
            .write_instances(scene, &mut instance_offset)
            .with_context(|| {
                format!(
                    "scene too large: {} paths, {} shadows, {} quads, {} underlines, {} monochrome sprites, {} subpixel sprites, {} polychrome sprites",
                    scene.paths.len(),
                    scene.shadows.len(),
                    scene.quads.len(),
                    scene.underlines.len(),
                    scene.monochrome_sprites.len(),
                    scene.subpixel_sprites.len(),
                    scene.polychrome_sprites.len(),
                )
            })?;
        self.prepare_texture_bind_groups(scene);

        let mut encoder =
            self.resources()
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("main_encoder"),
                });

        // Layers composite into whatever they are drawn over, so only a backdrop — which samples
        // the frame itself — is worth routing the whole frame through a texture for.
        let mirrored = filtered && !scene.backdrops.is_empty();
        let main_view = match mirrored {
            true => self.filter_view(FilterTarget::Frame),
            false => frame_view.clone(),
        };

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &main_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            // Where the pass in hand is drawing: the frame, or the layer being filled when one
            // is open. Anything that has to break the pass and resume it has to resume it here,
            // or its primitives land in the window while the rest of the frame is still being
            // assembled somewhere else.
            let mut target = main_view.clone();
            let mut stack: Vec<usize> = Vec::new();
            // What each open layer will be composited through: neighbours that ask for the same
            // filter share one target, so a list of separately blurred rows costs one pass, not one
            // per row.
            let mut spans: Vec<Bounds<ScaledPixels>> = Vec::new();
            for batch in scene.batches() {
                let wanted = filtered
                    .then(|| scene.filtered(scene.batch_order(&batch)))
                    .flatten()
                    .map(|index| scene.filter_chain(index))
                    .unwrap_or_default();

                let shared = stack
                    .iter()
                    .zip(wanted.iter())
                    .take_while(|(open, next)| open == next)
                    .count();

                let mut merged = false;
                while stack.len() > shared {
                    if stack.len() == shared + 1
                        && wanted.len() == stack.len()
                        && let Some(open) = stack.last_mut()
                        && let Some(span) = spans.last_mut()
                    {
                        let next = scene.effects[wanted[shared]];
                        let held = scene.effects[*open];
                        if held.can_merge_with(&next) {
                            *open = wanted[shared];
                            *span = span.union(&next.destination_clip());
                            merged = true;
                            break;
                        }
                    }

                    let index = stack.pop().expect("the stack is not empty");
                    let layer = scene.effects[index];
                    let clip = spans.pop().unwrap_or_else(|| layer.destination_clip());

                    drop(pass);
                    let (onto, blurred, params, composite) = self.close_layer(
                        &mut encoder,
                        layer,
                        clip,
                        stack.len(),
                        &main_view,
                        &mut instance_offset,
                    )?;
                    target = onto;
                    pass = Self::resume_pass(
                        &mut encoder,
                        "layer_composite_pass",
                        &target,
                        wgpu::LoadOp::Load,
                    );
                    self.composite_layer(
                        &blurred,
                        layer.destination_scissor_bounds(clip),
                        composite,
                        &params,
                        &mut pass,
                    );
                }

                for index in wanted.iter().skip(shared + usize::from(merged)) {
                    if stack.len() >= LAYER_DEPTH {
                        break;
                    }
                    let layer = scene.effects[*index];
                    let depth = stack.len();
                    let load = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);

                    drop(pass);
                    target = self.filter_view(FilterTarget::Layer(depth));
                    pass = Self::resume_pass(&mut encoder, "layer_pass", &target, load);
                    stack.push(*index);
                    spans.push(layer.destination_clip());
                }

                match batch {
                    PrimitiveBatch::Quads(range) => self.draw_instances(
                        &instance_bindings.quads,
                        &self.resources().pipelines.quads,
                        instance_range(range),
                        &mut pass,
                    ),
                    PrimitiveBatch::Shaders { shader, range } => {
                        if let Some(pipeline) = self.custom_pipeline(shader) {
                            self.draw_instances(
                                &instance_bindings.shaders,
                                &pipeline,
                                instance_range(range),
                                &mut pass,
                            );
                        }
                    }
                    PrimitiveBatch::Shadows(range) => self.draw_instances(
                        &instance_bindings.shadows,
                        &self.resources().pipelines.shadows,
                        instance_range(range),
                        &mut pass,
                    ),
                    PrimitiveBatch::Paths(range) => {
                        let paths = &scene.paths[range];
                        if paths.is_empty() {
                            continue;
                        }

                        drop(pass);
                        let rasterized = self.draw_paths_to_intermediate(
                            &mut encoder,
                            paths,
                            &mut instance_offset,
                        )?;

                        pass = Self::resume_pass(
                            &mut encoder,
                            "main_pass_continued",
                            &target,
                            wgpu::LoadOp::Load,
                        );

                        if rasterized {
                            self.draw_paths_from_intermediate(
                                paths,
                                &mut instance_offset,
                                &mut pass,
                            )?;
                        }
                    }
                    PrimitiveBatch::Underlines(range) => self.draw_instances(
                        &instance_bindings.underlines,
                        &self.resources().pipelines.underlines,
                        instance_range(range),
                        &mut pass,
                    ),
                    PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                        self.draw_sprites(
                            &instance_bindings.monochrome_sprites,
                            texture_id,
                            &self.resources().pipelines.mono_sprites,
                            instance_range(range),
                            &mut pass,
                        )?;
                    }
                    PrimitiveBatch::SubpixelSprites { texture_id, range } => {
                        let resources = self.resources();
                        // See `fs_subpixel_sprite_layered` for why a layer needs a different
                        // blend than the frame.
                        let pipeline = match stack.is_empty() {
                            true => resources.pipelines.subpixel_sprites.as_ref(),
                            false => resources.pipelines.subpixel_sprites_layered.as_ref(),
                        };
                        self.draw_sprites(
                            &instance_bindings.subpixel_sprites,
                            texture_id,
                            pipeline.unwrap_or(&resources.pipelines.mono_sprites),
                            instance_range(range),
                            &mut pass,
                        )?;
                    }
                    PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                        self.draw_sprites(
                            &instance_bindings.polychrome_sprites,
                            texture_id,
                            &self.resources().pipelines.poly_sprites,
                            instance_range(range),
                            &mut pass,
                        )?;
                    }
                    PrimitiveBatch::Backdrops(range) => {
                        if !filtered {
                            continue;
                        }
                        let backdrops = &scene.backdrops[range.clone()];
                        let sigma = backdrops
                            .iter()
                            .fold(0., |widest: f32, backdrop| widest.max(backdrop.blur));
                        // A backdrop samples the blur only within its own bounds, so the
                        // passes are scissored to the union of the batch's, grown by the
                        // kernel inside `blur_source`. Blurring the whole frame for a strip
                        // of it made every frame with a backdrop on it cost the full screen.
                        let clip = backdrops
                            .iter()
                            .map(|backdrop| backdrop.bounds)
                            .reduce(|union, bounds| union.union(&bounds));
                        drop(pass);
                        let source = self.backdrop_source(
                            &mut encoder,
                            &main_view,
                            stack.len(),
                            clip.map(|clip| {
                                clip.dilate(BlurPasses::for_sigma(sigma).source_margin())
                            }),
                            &mut instance_offset,
                        )?;
                        let blurred = self.blur_source(
                            &mut encoder,
                            &source,
                            false,
                            sigma,
                            clip,
                            &mut instance_offset,
                        )?;
                        pass = Self::resume_pass(
                            &mut encoder,
                            "main_pass_continued",
                            &target,
                            wgpu::LoadOp::Load,
                        );

                        self.draw_backdrops(
                            &instance_bindings.backdrops,
                            &blurred,
                            instance_range(range),
                            &mut pass,
                        );
                    }
                    // Surfaces are macOS-only for video playback and are not
                    // implemented by the WGPU renderer.
                    PrimitiveBatch::Surfaces(_surfaces) => {}
                }
            }

            while let Some(index) = stack.pop() {
                let layer = scene.effects[index];
                let clip = spans.pop().unwrap_or_else(|| layer.destination_clip());

                drop(pass);
                let (onto, blurred, params, composite) = self.close_layer(
                    &mut encoder,
                    layer,
                    clip,
                    stack.len(),
                    &main_view,
                    &mut instance_offset,
                )?;
                target = onto;
                pass = Self::resume_pass(
                    &mut encoder,
                    "layer_composite_pass",
                    &target,
                    wgpu::LoadOp::Load,
                );
                self.composite_layer(
                    &blurred,
                    layer.destination_scissor_bounds(clip),
                    composite,
                    &params,
                    &mut pass,
                );
            }
        }

        if mirrored {
            let plain = self.write_instance_binding(
                "frame_blit_bind_group",
                &mut instance_offset,
                &[BlurParams::default()],
            )?;
            let blit = self.resources().pipelines.blit.clone();
            self.fullscreen_pass(
                &mut encoder,
                "frame_blit",
                &blit,
                &main_view,
                frame_view,
                &plain,
                None,
            );
        }

        let submission = self
            .resources()
            .queue
            .submit(std::iter::once(encoder.finish()));

        let now = self.frame_time;
        self.resources_mut().release_targets_idle_at(now);
        self.settle_instance_data(instance_offset, now);

        Ok(submission)
    }

    /// Blurs a filtered layer that has just closed, when its filter asks for a blur, and
    /// returns the target it composites onto, the view it composites from and the parameters
    /// of the composite. `depth` is the layer's own nesting depth.
    fn close_layer(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        layer: LayerEffect,
        clip: Bounds<ScaledPixels>,
        depth: usize,
        main_view: &wgpu::TextureView,
        instance_offset: &mut u64,
    ) -> Result<(
        wgpu::TextureView,
        wgpu::TextureView,
        InstanceBinding,
        Composite,
    )> {
        let source = self.filter_view(FilterTarget::Layer(depth.min(LAYER_DEPTH - 1)));
        let onto = match depth.checked_sub(1) {
            Some(parent) => self.filter_view(FilterTarget::Layer(parent.min(LAYER_DEPTH - 1))),
            None => main_view.clone(),
        };
        let mut mask = mask_params(layer, clip);
        // A narrow blur leaves its pass down to the composite, which saves a pass of its own.
        let folded = match BlurPasses::for_sigma(layer.filter.blur) {
            BlurPasses::Gaussian(sigma) if layer.filter.blurs() => Some(sigma),
            _ => None,
        };
        let (blurred, composite) = match (folded, layer.filter.blurs()) {
            (Some(sigma), _) => {
                mask.pad = sigma;
                let across = self.gaussian_across(
                    encoder,
                    &source,
                    sigma,
                    Some(layer.blur_bounds(clip)),
                    instance_offset,
                )?;
                (across, Composite::Blurred)
            }
            (None, true) => (
                self.blur_source(
                    encoder,
                    &source,
                    true,
                    layer.filter.blur,
                    Some(layer.blur_bounds(clip)),
                    instance_offset,
                )?,
                Composite::of(layer),
            ),
            (None, false) => (source, Composite::of(layer)),
        };
        let params =
            self.write_instance_binding("layer_composite_bind_group", instance_offset, &[mask])?;
        Ok((onto, blurred, params, composite))
    }

    fn write_instances(
        &mut self,
        scene: &Scene,
        instance_offset: &mut u64,
    ) -> Result<InstanceBindings> {
        Ok(InstanceBindings {
            backdrops: self.write_instance_binding(
                "backdrops_bind_group",
                instance_offset,
                &scene.backdrops,
            )?,
            quads: self.write_instance_binding(
                "quads_bind_group",
                instance_offset,
                &scene.quads,
            )?,
            shaders: self.write_instance_binding(
                "shaders_bind_group",
                instance_offset,
                &scene.shaders,
            )?,
            shadows: self.write_instance_binding(
                "shadows_bind_group",
                instance_offset,
                &scene.shadows,
            )?,
            underlines: self.write_instance_binding(
                "underlines_bind_group",
                instance_offset,
                &scene.underlines,
            )?,
            monochrome_sprites: self.write_instance_binding(
                "monochrome_sprites_bind_group",
                instance_offset,
                &scene.monochrome_sprites,
            )?,
            subpixel_sprites: self.write_instance_binding(
                "subpixel_sprites_bind_group",
                instance_offset,
                &scene.subpixel_sprites,
            )?,
            polychrome_sprites: self.write_instance_binding(
                "polychrome_sprites_bind_group",
                instance_offset,
                &scene.polychrome_sprites,
            )?,
        })
    }

    fn create_texture_bind_group(
        &self,
        label: &str,
        texture_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let resources = self.resources();
        resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &resources.bind_group_layouts.texture,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&resources.atlas_sampler),
                    },
                ],
            })
    }

    fn prepare_texture_bind_groups(&mut self, scene: &Scene) {
        let mut texture_ids = SmallVec::<[AtlasTextureId; 8]>::new();
        for batch in scene.batches() {
            let texture_id = match batch {
                PrimitiveBatch::MonochromeSprites { texture_id, .. }
                | PrimitiveBatch::SubpixelSprites { texture_id, .. }
                | PrimitiveBatch::PolychromeSprites { texture_id, .. } => texture_id,
                _ => continue,
            };
            if !texture_ids.contains(&texture_id) {
                texture_ids.push(texture_id);
            }
        }

        self.resources_mut()
            .atlas_texture_bind_groups
            .retain(|texture_id, _| texture_ids.contains(texture_id));

        for texture_id in texture_ids {
            let Some(texture_info) = self.atlas.get_texture_info(texture_id) else {
                self.resources_mut()
                    .atlas_texture_bind_groups
                    .remove(&texture_id);
                continue;
            };
            let is_current = self
                .resources()
                .atlas_texture_bind_groups
                .get(&texture_id)
                .is_some_and(|cached| cached.texture_generation == texture_info.generation);
            if is_current {
                continue;
            }

            let bind_group =
                self.create_texture_bind_group("atlas_texture_bind_group", &texture_info.view);
            self.resources_mut().atlas_texture_bind_groups.insert(
                texture_id,
                CachedTextureBindGroup {
                    texture_generation: texture_info.generation,
                    bind_group,
                },
            );
        }
    }

    fn draw_instances(
        &self,
        instances: &InstanceBinding,
        pipeline: &wgpu::RenderPipeline,
        range: Range<u32>,
        pass: &mut wgpu::RenderPass<'_>,
    ) {
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.resources().globals_bind_group, &[]);
        pass.set_bind_group(1, &instances.bind_group, &[]);
        pass.draw(
            0..4,
            instances.first_instance + range.start..instances.first_instance + range.end,
        );
    }

    /// The pipeline of a custom shader, compiled the first time a frame draws it. `None` means
    /// it failed to compile, which is logged once and leaves its instances undrawn.
    fn custom_pipeline(&mut self, shader: ShaderId) -> Option<wgpu::RenderPipeline> {
        if let Some(pipeline) = self.resources.pipelines.custom.get(&shader) {
            return pipeline.clone();
        }
        let pipeline = self.create_custom_pipeline(shader);
        self.resources
            .pipelines
            .custom
            .insert(shader, pipeline.clone());
        pipeline
    }

    fn create_custom_pipeline(&self, shader: ShaderId) -> Option<wgpu::RenderPipeline> {
        let Some(program) = gpui::shader_program(shader) else {
            log::error!("custom shader {} was never registered", shader.0);
            return None;
        };
        let transport = match self.uses_webgl_instance_data {
            true => CUSTOM_SHADER_WEBGL_TRANSPORT,
            false => gpui::SHADER_STORAGE_TRANSPORT,
        };
        let resources = self.resources();
        let device = &resources.device;

        // The source was validated when it was registered, by the naga wgpu runs, so this
        // only catches what the device itself rejects.
        #[cfg(not(target_family = "wasm"))]
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&program.label),
            source: wgpu::ShaderSource::Wgsl(program.compose(transport).into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(&program.label),
            bind_group_layouts: &[
                Some(&resources.bind_group_layouts.globals),
                Some(&resources.bind_group_layouts.instances),
            ],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&program.label),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(gpui::SHADER_VERTEX_ENTRY),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(gpui::SHADER_FRAGMENT_ENTRY),
                targets: &[Some(resources.pipelines.custom_target.clone())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        #[cfg(not(target_family = "wasm"))]
        if let Some(error) = gpui::block_on(scope.pop()) {
            log::error!(
                "custom shader `{}` failed to compile: {error}",
                program.label
            );
            return None;
        }
        Some(pipeline)
    }

    fn draw_sprites(
        &self,
        sprite_instances: &InstanceBinding,
        texture_id: AtlasTextureId,
        pipeline: &wgpu::RenderPipeline,
        range: Range<u32>,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> Result<()> {
        if range.is_empty() {
            return Ok(());
        }
        let resources = self.resources();
        // The atlas has released this texture; the batch belongs to a stale
        // paint that will be replaced once its view re-renders.
        let Some(texture) = resources.atlas_texture_bind_groups.get(&texture_id) else {
            return Ok(());
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &resources.globals_bind_group, &[]);
        pass.set_bind_group(1, &sprite_instances.bind_group, &[]);
        pass.set_bind_group(2, &texture.bind_group, &[]);
        pass.draw(
            0..4,
            sprite_instances.first_instance + range.start
                ..sprite_instances.first_instance + range.end,
        );
        Ok(())
    }

    unsafe fn instance_bytes<T>(instances: &[T]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                instances.as_ptr() as *const u8,
                std::mem::size_of_val(instances),
            )
        }
    }

    fn draw_paths_from_intermediate(
        &mut self,
        paths: &[Path<ScaledPixels>],
        instance_offset: &mut u64,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> Result<()> {
        let first_path = &paths[0];
        let sprites: Vec<PathSprite> = if paths.last().map(|p| &p.order) == Some(&first_path.order)
        {
            paths
                .iter()
                .map(|p| PathSprite {
                    bounds: p.clipped_bounds(),
                })
                .collect()
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            vec![PathSprite { bounds }]
        };

        let Some(path_intermediate_view) = self
            .resources()
            .path_intermediate
            .as_ref()
            .map(|scratch| scratch.view.clone())
        else {
            return Ok(());
        };
        let instances =
            self.write_instance_binding("path_sprites_bind_group", instance_offset, &sprites)?;
        let texture = self.create_texture_bind_group(
            "path_intermediate_texture_bind_group",
            &path_intermediate_view,
        );
        let resources = self.resources();
        pass.set_pipeline(&resources.pipelines.paths);
        pass.set_bind_group(0, &resources.globals_bind_group, &[]);
        pass.set_bind_group(1, &instances.bind_group, &[]);
        pass.set_bind_group(2, &texture, &[]);
        pass.draw(
            0..4,
            instances.first_instance..instances.first_instance + sprites.len() as u32,
        );
        Ok(())
    }

    fn draw_paths_to_intermediate(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        paths: &[Path<ScaledPixels>],
        instance_offset: &mut u64,
    ) -> Result<bool> {
        let mut vertices = Vec::new();
        for path in paths {
            let bounds = path.clipped_bounds();
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationVertex {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds,
            }));
        }

        if vertices.is_empty() {
            return Ok(false);
        }

        let vertex_binding = self.write_instance_binding(
            "path_rasterization_bind_group",
            instance_offset,
            &vertices,
        )?;

        let (path_intermediate_view, path_msaa_view) = self.path_views();
        // A multisampled target is only read by its resolve, so its samples are dropped at the
        // end of the pass instead of being written back.
        let (target_view, resolve_target, store) = match &path_msaa_view {
            Some(msaa_view) => (
                msaa_view,
                Some(&path_intermediate_view),
                wgpu::StoreOp::Discard,
            ),
            None => (&path_intermediate_view, None, wgpu::StoreOp::Store),
        };
        let resources = self.resources();

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("path_rasterization_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });

            pass.set_pipeline(&resources.pipelines.path_rasterization);
            pass.set_bind_group(0, &resources.path_globals_bind_group, &[]);
            pass.set_bind_group(1, &vertex_binding.bind_group, &[]);
            // The path rasterization shader loads records by vertex index
            // rather than instance index, so the allocation's base shifts the
            // vertex range here.
            pass.draw(
                vertex_binding.first_instance
                    ..vertex_binding.first_instance + vertices.len() as u32,
                0..1,
            );
        }

        Ok(true)
    }

    fn write_instance_binding<T>(
        &mut self,
        label: &str,
        instance_offset: &mut u64,
        instances: &[T],
    ) -> Result<InstanceBinding> {
        let data = unsafe { Self::instance_bytes(instances) };
        // wgpu rejects zero-sized bindings, so empty primitive arrays still
        // reserve the 16-byte minimum.
        let size = (data.len() as u64).max(16);
        let stride = (std::mem::size_of::<T>() as u64).max(1);
        let (alignment, allocation_size) = if self.uses_webgl_instance_data {
            // The texture transport has no binding offset: the shader indexes
            // the instance texture absolutely, so each allocation must start on
            // a whole instance (a stride multiple) and a whole texel, and must
            // end on a texel boundary so the zero padding of its final partial
            // texel cannot overlap the next allocation.
            (
                least_common_multiple(self.instance_data_alignment, stride),
                size.next_multiple_of(INSTANCE_TEXTURE_TEXEL_SIZE),
            )
        } else {
            (self.instance_data_alignment.max(1), size)
        };
        let mut offset = (*instance_offset).next_multiple_of(alignment);
        if offset + allocation_size > self.instance_data_capacity {
            self.grow_instance_data(allocation_size)?;
            offset = 0;
        }
        *instance_offset = offset + allocation_size;

        let first_instance = if self.uses_webgl_instance_data {
            u32::try_from(offset / stride).context("instance index exceeds u32 range")?
        } else {
            0
        };

        let resources = self.resources();
        if !data.is_empty() {
            match &resources.instance_data {
                InstanceData::Storage(buffer) => resources.queue.write_buffer(buffer, offset, data),
                InstanceData::Texture { .. } => {
                    Self::write_instance_texture(resources, offset, data)
                }
            }
        }
        let bind_group = resources
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &resources.bind_group_layouts.instances,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: match &resources.instance_data {
                        InstanceData::Storage(buffer) => {
                            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer,
                                offset,
                                size: NonZeroU64::new(size),
                            })
                        }
                        InstanceData::Texture { view, .. } => {
                            wgpu::BindingResource::TextureView(view)
                        }
                    },
                }],
            });
        Ok(InstanceBinding {
            bind_group,
            first_instance,
        })
    }

    fn write_instance_texture(resources: &WgpuResources, offset: u64, data: &[u8]) {
        let InstanceData::Texture {
            texture,
            width,
            height,
            ..
        } = &resources.instance_data
        else {
            return;
        };
        let mut byte_offset = 0usize;
        let mut texel_offset = offset / INSTANCE_TEXTURE_TEXEL_SIZE;
        while byte_offset < data.len() {
            let x = (texel_offset % u64::from(*width)) as u32;
            let y = (texel_offset / u64::from(*width)) as u32;
            if y >= *height {
                // The capacity check in write_instance_binding should make this
                // unreachable. Truncating silently would leave stale bytes in the
                // texture and draw garbage for the remaining instances.
                debug_assert!(
                    false,
                    "instance texture write out of bounds: row {y} >= height {}",
                    *height
                );
                log::error!(
                    "instance texture write out of bounds; dropping {} bytes of instance data",
                    data.len() - byte_offset
                );
                return;
            }
            let available_texels = u64::from(*width - x);
            let remaining_bytes = data.len() - byte_offset;
            let complete_texels = remaining_bytes as u64 / INSTANCE_TEXTURE_TEXEL_SIZE;
            let texels = complete_texels.min(available_texels);
            if texels > 0 {
                let byte_count = (texels * INSTANCE_TEXTURE_TEXEL_SIZE) as usize;
                resources.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &data[byte_offset..byte_offset + byte_count],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(byte_count as u32),
                        rows_per_image: None,
                    },
                    wgpu::Extent3d {
                        width: texels as u32,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                );
                byte_offset += byte_count;
                texel_offset += texels;
                continue;
            }

            let mut final_texel = [0; INSTANCE_TEXTURE_TEXEL_SIZE as usize];
            final_texel[..remaining_bytes].copy_from_slice(&data[byte_offset..]);
            resources.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &final_texel,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(INSTANCE_TEXTURE_TEXEL_SIZE as u32),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            break;
        }
    }

    fn grow_instance_data(&mut self, required: u64) -> Result<()> {
        let capacity = (self.instance_data_capacity * 2)
            .max(required.next_power_of_two())
            .min(self.max_instance_data_size);
        anyhow::ensure!(
            capacity >= required,
            "instance data needs {required} bytes, above the maximum of {}",
            self.max_instance_data_size
        );
        anyhow::ensure!(
            capacity > self.instance_data_capacity,
            "frame instance data exceeds the {}-byte maximum",
            self.max_instance_data_size
        );
        log::debug!(
            "instance data grown from {} to {capacity}",
            self.instance_data_capacity
        );
        // Bind groups created earlier in the frame keep the previous buffer or
        // texture alive, so allocations written before the grow remain valid;
        // only subsequent writes land in the new allocation.
        self.replace_instance_data(capacity);
        Ok(())
    }

    /// Shrinks the instance data once every frame over the last `INSTANCE_DATA_SETTLE` has
    /// needed under a quarter of it. `used` is what the frame just submitted wrote, and the
    /// new capacity keeps twice the peak so the next busy frame does not grow it straight back.
    fn settle_instance_data(&mut self, used: u64, now: Instant) {
        self.instance_data_peak = self.instance_data_peak.max(used);
        if now.duration_since(self.instance_data_window) < INSTANCE_DATA_SETTLE {
            return;
        }
        let peak = std::mem::take(&mut self.instance_data_peak);
        self.instance_data_window = now;

        let capacity = (peak * 2)
            .next_power_of_two()
            .max(INITIAL_INSTANCE_DATA_SIZE)
            .min(self.max_instance_data_size);
        if peak * 4 > self.instance_data_capacity || capacity >= self.instance_data_capacity {
            return;
        }
        log::debug!(
            "instance data shrunk from {} to {capacity}",
            self.instance_data_capacity
        );
        self.replace_instance_data(capacity);
    }

    /// Swaps the instance data for a fresh allocation of at least `capacity` bytes.
    fn replace_instance_data(&mut self, capacity: u64) {
        let uses_webgl_instance_data = self.uses_webgl_instance_data;
        let resources = self.resources_mut();
        if uses_webgl_instance_data {
            let max_texture_dimension = resources.device.limits().max_texture_dimension_2d;
            let (instance_data, actual_capacity) =
                Self::create_instance_texture(&resources.device, capacity, max_texture_dimension);
            resources.instance_data = instance_data;
            self.instance_data_capacity = actual_capacity;
        } else {
            resources.instance_data =
                InstanceData::Storage(resources.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("instance_buffer"),
                    size: capacity,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            self.instance_data_capacity = capacity;
        }
    }
}

impl WgpuRenderer {
    /// Mark the surface as unconfigured so rendering is skipped until a new
    /// surface is provided via [`replace_surface`](Self::replace_surface).
    ///
    /// This does **not** drop the renderer — the device, queue, atlas, and
    /// pipelines stay alive.  Use this when the native window is destroyed
    /// (e.g. Android `TerminateWindow`) but you intend to re-create the
    /// surface later without losing cached atlas textures.
    pub fn unconfigure_surface(&mut self) {
        self.discard_held_frame();
        self.state = match std::mem::replace(&mut self.state, RendererState::Released) {
            RendererState::Ready { core, surface } => {
                drop(surface);
                RendererState::Unconfigured { core }
            }
            state @ (RendererState::Unconfigured { .. } | RendererState::Released) => state,
        };
        // Drop intermediate textures since they reference the old surface size.
        if let Some(core) = self.core_mut() {
            core.resources.invalidate_intermediate_textures();
        }
    }

    /// Replace the wgpu surface with a new one (e.g. after Android destroys
    /// and recreates the native window).  Keeps the device, queue, atlas, and
    /// all pipelines intact so cached `AtlasTextureId`s remain valid.
    ///
    /// The `instance` **must** be the same [`wgpu::Instance`] that was used to
    /// create the adapter and device (i.e. from the [`WgpuContext`]).  Using a
    /// different instance will cause a "Device does not exist" panic because
    /// the wgpu device is bound to its originating instance.
    #[cfg(not(target_family = "wasm"))]
    pub fn replace_surface<W: HasWindowHandle>(
        &mut self,
        window: &W,
        config: WgpuSurfaceConfig,
        instance: &wgpu::Instance,
    ) -> anyhow::Result<()> {
        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;

        let surface = create_surface(instance, window_handle.as_raw())?;
        self.discard_held_frame();

        let width = (config.size.width.0 as u32).max(1);
        let height = (config.size.height.0 as u32).max(1);

        let alpha_mode = if config.transparent {
            self.transparent_alpha_mode
        } else {
            self.opaque_alpha_mode
        };

        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface_config.alpha_mode = alpha_mode;
        if let Some(mode) = config.preferred_present_mode {
            self.surface_config.present_mode = mode;
        }

        let mut core = match std::mem::replace(&mut self.state, RendererState::Released) {
            RendererState::Ready {
                core,
                surface: old_surface,
            } => {
                drop(old_surface);
                core
            }
            RendererState::Unconfigured { core } => core,
            RendererState::Released => {
                anyhow::bail!("Cannot replace the surface: GPU resources have been released")
            }
        };
        surface.configure(&core.resources.device, &self.surface_config);
        core.resources.invalidate_intermediate_textures();
        self.state = RendererState::Ready { surface, core };

        Ok(())
    }

    /// Frees offscreen targets that no frame has drawn through for `IDLE_RELEASE`. Every frame
    /// checks this itself, so a caller only needs it for a window that has stopped drawing.
    pub fn release_idle_targets(&mut self) {
        let Some(core) = self.core_mut() else {
            return;
        };
        let resources = &mut core.resources;
        resources.release_targets_idle_at(Instant::now());
        // A dropped texture is only freed when the device is next maintained, which a frame
        // does on submit. With no frame coming, the device is polled here instead, which also
        // frees whatever the last frame released.
        if let Err(error) = resources.device.poll(wgpu::PollType::Poll) {
            warn!("Failed to poll device after releasing idle targets: {error:?}");
        }
    }

    pub fn destroy(&mut self) {
        self.held_frame = None;
        // Release surface-bound GPU resources eagerly so the underlying native
        // window can be destroyed before the renderer itself is dropped.
        self.state = RendererState::Released;
    }

    /// Returns true if the GPU device was lost and recovery is needed.
    pub fn device_lost(&self) -> bool {
        self.device_errors.device_lost()
    }

    /// Returns true if a redraw is needed because GPU state was cleared.
    /// Calling this method clears the flag.
    pub fn needs_redraw(&mut self) -> bool {
        std::mem::take(&mut self.needs_redraw)
    }

    /// Recovers from a lost GPU device by recreating the renderer with a new context.
    ///
    /// Call this after detecting `device_lost()` returns true.
    ///
    /// This method coordinates recovery across multiple windows:
    /// - The first window to call this will recreate the shared context
    /// - Subsequent windows will adopt the already-recovered context
    #[cfg(not(target_family = "wasm"))]
    pub fn recover<W>(&mut self, window: &W) -> anyhow::Result<()>
    where
        W: HasWindowHandle + HasDisplayHandle + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        self.held_frame = None;
        let gpu_context = self.context.as_ref().expect("recover requires gpu_context");

        // Check if another window already recovered the context
        let needs_new_context = gpu_context
            .borrow()
            .as_ref()
            .is_none_or(|ctx| ctx.device_lost());

        let window_handle = window
            .window_handle()
            .map_err(|e| anyhow::anyhow!("Failed to get window handle: {e}"))?;

        let surface = if needs_new_context {
            log::warn!("GPU device lost, recreating context...");

            // Drop old resources to release Arc<Device>/Arc<Queue> and GPU resources
            self.state = RendererState::Released;
            *gpu_context.borrow_mut() = None;

            // Wait briefly for the GPU driver to stabilize, then try to
            // recreate the context without software renderers. If this fails
            // the caller should request another frame and retry — the real GPU
            // may need more time to come back (e.g. after suspend/resume).
            std::thread::sleep(std::time::Duration::from_millis(350));

            let instance = WgpuContext::instance(Some(Box::new(window.clone())));
            let surface = create_surface(&instance, window_handle.as_raw())?;
            let new_context =
                WgpuContext::new_rejecting_software(instance, &surface, self.compositor_gpu)?;
            *gpu_context.borrow_mut() = Some(new_context);
            surface
        } else {
            let ctx_ref = gpu_context.borrow();
            let instance = &ctx_ref.as_ref().unwrap().instance;
            create_surface(instance, window_handle.as_raw())?
        };

        let config = WgpuSurfaceConfig {
            size: gpui::Size {
                width: gpui::DevicePixels(self.surface_config.width as i32),
                height: gpui::DevicePixels(self.surface_config.height as i32),
            },
            transparent: self.surface_config.alpha_mode != wgpu::CompositeAlphaMode::Opaque,
            preferred_present_mode: Some(self.surface_config.present_mode),
        };
        let gpu_context = Rc::clone(gpu_context);
        let ctx_ref = gpu_context.borrow();
        let context = ctx_ref.as_ref().expect("context should exist");

        self.state = RendererState::Released;
        self.atlas.handle_device_lost(context);

        let is_bgr = self.is_bgr;
        *self = Self::new_internal(
            Some(gpu_context.clone()),
            context,
            surface,
            config,
            self.compositor_gpu,
            self.atlas.clone(),
        )?;
        self.set_subpixel_layout(is_bgr);

        log::info!("GPU recovery complete");
        Ok(())
    }
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
struct HeadlessRenderTarget {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
impl HeadlessRenderTarget {
    fn size(&self) -> Size<DevicePixels> {
        Size {
            width: DevicePixels(self.texture.width() as i32),
            height: DevicePixels(self.texture.height() as i32),
        }
    }
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
pub struct WgpuHeadlessRenderer {
    context: WgpuContext,
    core: WgpuRendererCore,
    render_target: Option<HeadlessRenderTarget>,
    observed_error_generation: u64,
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
impl WgpuHeadlessRenderer {
    pub fn new() -> anyhow::Result<Self> {
        let (context, target_format) = WgpuContext::new_headless()?;
        // The device may be shared with earlier renderers; observe only errors raised from
        // here on, including any while this renderer builds its pipelines.
        let observed_error_generation = context.errors().current_generation();
        let atlas = Arc::new(WgpuAtlas::from_context(&context));
        let core = WgpuRendererCore::new(
            &context,
            atlas,
            target_format,
            wgpu::CompositeAlphaMode::Opaque,
        );

        Ok(Self {
            context,
            core,
            render_target: None,
            observed_error_generation,
        })
    }

    fn ensure_render_target(&mut self, size: Size<DevicePixels>) -> anyhow::Result<()> {
        anyhow::ensure!(
            size.width.0 > 0 && size.height.0 > 0,
            "invalid headless render target size: {size:?}"
        );
        anyhow::ensure!(
            size.width.0 as u32 <= self.core.max_texture_size
                && size.height.0 as u32 <= self.core.max_texture_size,
            "headless render target size {size:?} exceeds maximum texture dimension {}",
            self.core.max_texture_size
        );
        if self
            .render_target
            .as_ref()
            .is_some_and(|target| target.size() == size)
        {
            return Ok(());
        }

        let texture = self
            .core
            .resources
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("headless_render_target"),
                size: wgpu::Extent3d {
                    width: size.width.0 as u32,
                    height: size.height.0 as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.core.target_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.render_target = Some(HeadlessRenderTarget { texture, view });
        Ok(())
    }

    fn check_gpu_errors(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.context.device_lost(),
            "GPU device was lost during headless rendering"
        );
        if let Some(error) = self
            .context
            .errors()
            .observe_error(&mut self.observed_error_generation)
        {
            anyhow::bail!("GPU error during headless rendering: {error}");
        }
        Ok(())
    }

    fn render(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.check_gpu_errors()?;
        self.ensure_render_target(size)?;
        let view = self
            .render_target
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Headless render target was not created"))?
            .view
            .clone();
        self.core
            .render_frame(scene, &view, size, false, wgpu::Color::BLACK)?;
        Ok(())
    }

    /// Copies the current render target back to the CPU. Dimensions come from the
    /// target texture itself, so the copy can never disagree with what was rendered.
    fn read_image(&mut self) -> anyhow::Result<image::RgbaImage> {
        let target = self
            .render_target
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Headless render target was not created"))?;
        let width = target.texture.width();
        let height = target.texture.height();
        let bytes_per_row = width
            .checked_mul(4)
            .ok_or_else(|| anyhow::anyhow!("Headless render target row size overflowed"))?;
        let padded_bytes_per_row = bytes_per_row
            .checked_next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            .ok_or_else(|| anyhow::anyhow!("Headless padded row size overflowed"))?;
        let buffer_size = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(height))
            .ok_or_else(|| anyhow::anyhow!("Headless readback buffer size overflowed"))?;
        anyhow::ensure!(
            buffer_size <= self.core.resources.device.limits().max_buffer_size,
            "Headless readback buffer size {buffer_size} exceeds maximum buffer size {}",
            self.core.resources.device.limits().max_buffer_size
        );
        let readback_buffer = self
            .core
            .resources
            .device
            .create_buffer(&wgpu::BufferDescriptor {
                label: Some("headless_readback_buffer"),
                size: buffer_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
        let mut encoder =
            self.core
                .resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("headless_readback_encoder"),
                });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let submission = self
            .core
            .resources
            .queue
            .submit(std::iter::once(encoder.finish()));
        let (sender, receiver) = std::sync::mpsc::channel();
        readback_buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                if sender.send(result).is_err() {
                    log::error!("Headless readback receiver was dropped before mapping completed");
                }
            });
        self.core
            .resources
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .map_err(|error| anyhow::anyhow!("Failed to wait for headless rendering: {error}"))?;
        receiver
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|error| anyhow::anyhow!("Failed to receive headless mapping result: {error}"))?
            .map_err(|error| anyhow::anyhow!("Failed to map headless readback buffer: {error}"))?;
        self.check_gpu_errors()?;

        let mapped_data = readback_buffer.slice(..).get_mapped_range()?;
        let pixel_capacity = usize::try_from(u64::from(bytes_per_row) * u64::from(height))
            .map_err(|_| anyhow::anyhow!("Headless image size exceeds addressable memory"))?;
        let mut pixels = Vec::with_capacity(pixel_capacity);
        for row in mapped_data
            .chunks_exact(padded_bytes_per_row as usize)
            .take(height as usize)
        {
            pixels.extend_from_slice(&row[..bytes_per_row as usize]);
        }
        drop(mapped_data);
        readback_buffer.unmap();

        if self.core.target_format == wgpu::TextureFormat::Bgra8Unorm {
            for pixel in pixels.chunks_exact_mut(4) {
                pixel.swap(0, 2);
            }
        }

        image::RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| anyhow::anyhow!("Failed to create image from headless pixel data"))
    }
}

#[cfg(all(
    not(target_family = "wasm"),
    any(test, feature = "bench-support", feature = "test-support")
))]
impl gpui::PlatformHeadlessRenderer for WgpuHeadlessRenderer {
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> anyhow::Result<image::RgbaImage> {
        self.render(scene, size)?;
        self.read_image()
    }

    fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.render(scene, size)
    }

    fn sprite_atlas(&self) -> Arc<dyn gpui::PlatformAtlas> {
        self.core.atlas.clone()
    }
}

/// Which pipeline draws a closed layer back into what it was opened over.
#[derive(Clone, Copy)]
enum Composite {
    Plain,
    /// The layer fades towards its edges.
    Masked,
    /// The layer has been blurred across only, and the composite blurs it down.
    Blurred,
}

impl Composite {
    fn of(layer: LayerEffect) -> Self {
        match layer.filter.fades() {
            true => Self::Masked,
            false => Self::Plain,
        }
    }
}

fn mask_params(layer: LayerEffect, destination_span: Bounds<ScaledPixels>) -> MaskParams {
    let source_bounds = if layer.filter.transforms() {
        layer.source_bounds
    } else {
        destination_span
    };
    MaskParams {
        source_bounds: [
            source_bounds.origin.x.0,
            source_bounds.origin.y.0,
            source_bounds.size.width.0,
            source_bounds.size.height.0,
        ],
        // WGPU has historically based fade_edges on the blur-expanded layer bounds.
        fade_bounds: [
            layer.source_bounds.origin.x.0,
            layer.source_bounds.origin.y.0,
            layer.source_bounds.size.width.0,
            layer.source_bounds.size.height.0,
        ],
        transform_origin: [layer.transform_origin.x.0, layer.transform_origin.y.0],
        translation: [layer.filter.translate.x.0, layer.filter.translate.y.0],
        scale: layer.filter.scale,
        fade_top: layer.filter.fade_top,
        fade_bottom: layer.filter.fade_bottom,
        fade_left: layer.filter.fade_left,
        fade_right: layer.filter.fade_right,
        pad: 0.0,
    }
}

fn instance_range(range: Range<usize>) -> Range<u32> {
    range.start as u32..range.end as u32
}

#[cfg(not(target_family = "wasm"))]
fn create_surface(
    instance: &wgpu::Instance,
    raw_window_handle: raw_window_handle::RawWindowHandle,
) -> anyhow::Result<wgpu::Surface<'static>> {
    unsafe {
        instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                // Fall back to the display handle already provided via InstanceDescriptor::display.
                raw_display_handle: None,
                raw_window_handle,
            })
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

struct RenderingParameters {
    path_sample_count: u32,
    gamma_ratios: [f32; 4],
    grayscale_enhanced_contrast: f32,
    subpixel_enhanced_contrast: f32,
}

impl RenderingParameters {
    fn new(adapter: &wgpu::Adapter, surface_format: wgpu::TextureFormat) -> Self {
        use std::env;

        let format_features = adapter.get_texture_format_features(surface_format);
        let path_sample_count = [4, 2, 1]
            .into_iter()
            .find(|&n| format_features.flags.sample_count_supported(n))
            .unwrap_or(1);

        let gamma = env::var("ZED_FONTS_GAMMA")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.8_f32)
            .clamp(1.0, 2.2);
        let gamma_ratios = get_gamma_correction_ratios(gamma);

        let grayscale_enhanced_contrast = env::var("ZED_FONTS_GRAYSCALE_ENHANCED_CONTRAST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0_f32)
            .max(0.0);

        let subpixel_enhanced_contrast = env::var("ZED_FONTS_SUBPIXEL_ENHANCED_CONTRAST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.5_f32)
            .max(0.0);

        Self {
            path_sample_count,
            gamma_ratios,
            grayscale_enhanced_contrast,
            subpixel_enhanced_contrast,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        BorderStyle, ColorSpace, ContentMask, Corners, Edges, Hsla, MonochromeSprite,
        PolychromeSprite, Quad, Shadow, Size, SubpixelSprite, Underline, linear_color_stop,
        linear_gradient,
    };
    #[cfg(target_os = "linux")]
    use gpui::{DevicePixels, PlatformHeadlessRenderer, Scene};

    #[cfg(target_os = "linux")]
    fn device_size(width: i32, height: i32) -> Size<DevicePixels> {
        Size {
            width: DevicePixels(width),
            height: DevicePixels(height),
        }
    }

    #[cfg(target_os = "linux")]
    fn solid_quad(x: f32, y: f32, width: f32, height: f32, color: Hsla) -> Quad {
        let bounds = Bounds {
            origin: Point {
                x: x.into(),
                y: y.into(),
            },
            size: Size {
                width: width.into(),
                height: height.into(),
            },
        };
        Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds,
            content_mask: ContentMask { bounds },
            background: color.into(),
            border_color: color,
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
            corner_smoothing: 0.0,
            pad: 0,
        }
    }

    /// Channels are compared with a small tolerance so the assertions hold across
    /// drivers without pinning exact rasterizer output.
    #[cfg(target_os = "linux")]
    fn assert_pixel(image: &image::RgbaImage, x: u32, y: u32, expected: [u8; 4]) {
        let actual = image.get_pixel(x, y).0;
        assert!(
            actual
                .iter()
                .zip(expected)
                .all(|(actual, expected)| actual.abs_diff(expected) <= 3),
            "pixel ({x}, {y}) was {actual:?}, expected {expected:?}"
        );
    }

    #[cfg(target_os = "linux")]
    const RED: [u8; 4] = [255, 0, 0, 255];
    #[cfg(target_os = "linux")]
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    #[cfg(target_os = "linux")]
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_draws_quads_with_distinct_colors() -> anyhow::Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let mut scene = Scene::default();
        scene.insert_primitive(solid_quad(0.0, 0.0, 32.0, 32.0, gpui::red()));
        scene.insert_primitive(solid_quad(32.0, 0.0, 32.0, 32.0, gpui::blue()));
        scene.finish();

        let image = renderer.render_scene_to_image(&scene, device_size(64, 32))?;
        assert_eq!(image.dimensions(), (64, 32));
        assert_pixel(&image, 8, 16, RED);
        assert_pixel(&image, 24, 16, RED);
        assert_pixel(&image, 40, 16, BLUE);
        assert_pixel(&image, 56, 16, BLUE);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_draws_custom_shaders_between_quads() -> anyhow::Result<()> {
        let shader = gpui::Shader::new(gpui::ShaderSource::new(
            "flat",
            "fn shade(input: ShaderInput) -> vec4<f32> { return input.colors[1]; }",
        ))?;
        let bounds = Bounds {
            origin: Point {
                x: 0.0.into(),
                y: 0.0.into(),
            },
            size: Size {
                width: 32.0.into(),
                height: 32.0.into(),
            },
        };
        let instance = |shader: gpui::ShaderId| gpui::ShaderQuad {
            order: 0,
            shader,
            opacity: 1.0,
            time: 0.0,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: Corners::all(16.0.into()),
            scale_factor: 1.0,
            pad: [0; 3],
            params: [0.0; 16],
            colors: [[0.0; 4], [0.0, 0.0, 1.0, 1.0], [0.0; 4], [0.0; 4]],
        };

        let mut renderer = WgpuHeadlessRenderer::new()?;
        let mut scene = Scene::default();
        scene.insert_primitive(solid_quad(0.0, 0.0, 64.0, 32.0, gpui::red()));
        scene.insert_primitive(instance(shader.id()));
        scene.insert_primitive(solid_quad(24.0, 0.0, 40.0, 32.0, gpui::red()));
        let mut unknown = instance(gpui::ShaderId(u32::MAX));
        unknown.bounds.origin.x = 40.0.into();
        unknown.content_mask.bounds.origin.x = 40.0.into();
        scene.insert_primitive(unknown);
        scene.finish();

        let image = renderer.render_scene_to_image(&scene, device_size(64, 32))?;
        assert_pixel(&image, 12, 16, BLUE);
        assert_pixel(&image, 1, 1, RED);
        assert_pixel(&image, 28, 16, RED);
        assert_pixel(&image, 56, 16, RED);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_captures_each_requested_size() -> anyhow::Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let mut scene = Scene::default();
        scene.insert_primitive(solid_quad(2.0, 2.0, 4.0, 3.0, gpui::red()));
        scene.finish();

        // 13 px rows are 52 bytes, forcing readback to strip copy-row padding.
        let image = renderer.render_scene_to_image(&scene, device_size(13, 7))?;
        assert_eq!(image.dimensions(), (13, 7));
        assert_pixel(&image, 0, 0, BLACK);
        assert_pixel(&image, 3, 3, RED);
        assert_pixel(&image, 12, 6, BLACK);

        let image = renderer.render_scene_to_image(&scene, device_size(17, 9))?;
        assert_eq!(image.dimensions(), (17, 9));
        assert_pixel(&image, 3, 3, RED);
        assert_pixel(&image, 16, 8, BLACK);

        let image = renderer.render_scene_to_image(&Scene::default(), device_size(13, 7))?;
        assert_eq!(image.dimensions(), (13, 7));
        assert!(image.pixels().all(|pixel| pixel.0 == BLACK));
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_reuses_target_for_same_size() -> anyhow::Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let target_texture = |renderer: &WgpuHeadlessRenderer| {
            renderer
                .render_target
                .as_ref()
                .map(|target| target.texture.clone())
        };

        renderer.render_scene(&Scene::default(), device_size(16, 16))?;
        let first = target_texture(&renderer);
        assert!(first.is_some());

        renderer.render_scene(&Scene::default(), device_size(16, 16))?;
        assert_eq!(target_texture(&renderer), first);

        renderer.render_scene(&Scene::default(), device_size(16, 17))?;
        assert_ne!(target_texture(&renderer), first);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn headless_renderer_rejects_invalid_sizes() -> anyhow::Result<()> {
        let mut renderer = WgpuHeadlessRenderer::new()?;
        let too_large = renderer.core.max_texture_size as i32 + 1;

        for size in [
            device_size(0, 8),
            device_size(8, 0),
            device_size(-1, 8),
            device_size(too_large, 8),
        ] {
            assert!(
                renderer.render_scene(&Scene::default(), size).is_err(),
                "{size:?} should be rejected"
            );
            assert!(
                renderer
                    .render_scene_to_image(&Scene::default(), size)
                    .is_err(),
                "{size:?} should be rejected"
            );
        }

        // Rejection must leave the renderer usable.
        let image = renderer.render_scene_to_image(&Scene::default(), device_size(4, 4))?;
        assert_eq!(image.dimensions(), (4, 4));
        Ok(())
    }

    #[test]
    fn webgl_shader_is_valid_wgsl_without_storage_buffers() {
        assert!(!WEBGL_SHADERS.contains("var<storage"));
        validate_wgsl(WEBGL_SHADERS, naga::valid::Capabilities::empty());
    }

    #[test]
    fn storage_buffer_shader_is_valid_wgsl() {
        validate_wgsl(STORAGE_BUFFER_SHADERS, naga::valid::Capabilities::empty());
    }

    #[test]
    fn custom_shader_webgl_transport_is_valid_wgsl() {
        let source = format!(
            "{}\n{}\nfn shade(input: ShaderInput) -> vec4<f32> {{ return input.colors[0]; }}",
            gpui::SHADER_PRELUDE,
            CUSTOM_SHADER_WEBGL_TRANSPORT,
        );
        assert!(!source.contains("var<storage"));
        validate_wgsl(&source, naga::valid::Capabilities::empty());
        assert_eq!(std::mem::size_of::<gpui::ShaderQuad>(), 13 * 16);
    }

    #[test]
    fn subpixel_shader_is_valid_wgsl() {
        validate_wgsl(
            SUBPIXEL_SHADERS,
            naga::valid::Capabilities::DUAL_SOURCE_BLENDING,
        );
    }

    fn validate_wgsl(source: &str, capabilities: naga::valid::Capabilities) {
        let module = naga::front::wgsl::parse_str(source).expect("shader should parse");
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities)
            .validate(&module)
            .expect("shader should validate");
    }

    #[test]
    fn webgl_record_sizes_match_shader_word_strides() {
        assert_eq!(std::mem::size_of::<Quad>(), 42 * 4);
        assert_eq!(std::mem::size_of::<Shadow>(), 28 * 4);
        assert_eq!(std::mem::size_of::<PathRasterizationVertex>(), 26 * 4);
        assert_eq!(std::mem::size_of::<PathSprite>(), 4 * 4);
        assert_eq!(std::mem::size_of::<Underline>(), 16 * 4);
        assert_eq!(std::mem::size_of::<MonochromeSprite>(), 28 * 4);
        assert_eq!(std::mem::size_of::<SubpixelSprite>(), 28 * 4);
        assert_eq!(std::mem::size_of::<PolychromeSprite>(), 24 * 4);
        assert_eq!(std::mem::size_of::<MaskParams>(), 18 * 4);
        assert_eq!(std::mem::align_of::<MaskParams>(), 8);
        assert_eq!(std::mem::offset_of!(MaskParams, source_bounds), 0);
        assert_eq!(std::mem::offset_of!(MaskParams, fade_bounds), 16);
        assert_eq!(std::mem::offset_of!(MaskParams, transform_origin), 32);
        assert_eq!(std::mem::offset_of!(MaskParams, translation), 40);
        assert_eq!(std::mem::offset_of!(MaskParams, scale), 48);
        assert_eq!(std::mem::offset_of!(MaskParams, fade_top), 52);
        assert_eq!(std::mem::offset_of!(MaskParams, fade_bottom), 56);
        assert_eq!(std::mem::offset_of!(MaskParams, fade_left), 60);
        assert_eq!(std::mem::offset_of!(MaskParams, fade_right), 64);
        assert_eq!(std::mem::offset_of!(MaskParams, pad), 68);
    }

    #[test]
    fn webgl_quad_layout_matches_fixed_decoder() {
        let quad = Quad {
            order: 41,
            border_style: BorderStyle::Dashed,
            bounds: Bounds {
                origin: Point {
                    x: 1.0.into(),
                    y: 2.0.into(),
                },
                size: Size {
                    width: 3.0.into(),
                    height: 4.0.into(),
                },
            },
            content_mask: ContentMask {
                bounds: Bounds {
                    origin: Point {
                        x: 5.0.into(),
                        y: 6.0.into(),
                    },
                    size: Size {
                        width: 7.0.into(),
                        height: 8.0.into(),
                    },
                },
            },
            background: linear_gradient(
                11.0,
                linear_color_stop(
                    Hsla {
                        h: 12.0,
                        s: 13.0,
                        l: 14.0,
                        a: 15.0,
                    },
                    16.0,
                ),
                linear_color_stop(
                    Hsla {
                        h: 17.0,
                        s: 18.0,
                        l: 19.0,
                        a: 20.0,
                    },
                    21.0,
                ),
            )
            .color_space(ColorSpace::Oklab),
            border_color: Hsla {
                h: 22.0,
                s: 23.0,
                l: 24.0,
                a: 25.0,
            },
            corner_radii: Corners {
                top_left: 26.0.into(),
                top_right: 27.0.into(),
                bottom_right: 28.0.into(),
                bottom_left: 29.0.into(),
            },
            border_widths: Edges {
                top: 30.0.into(),
                right: 31.0.into(),
                bottom: 32.0.into(),
                left: 33.0.into(),
            },
            corner_smoothing: 34.0,
            pad: 0,
        };

        let bytes = unsafe { WgpuRendererCore::instance_bytes(std::slice::from_ref(&quad)) };
        let words: &[u32] = bytemuck::cast_slice(bytes);
        assert_eq!(
            words,
            &[
                41,
                1,
                1.0_f32.to_bits(),
                2.0_f32.to_bits(),
                3.0_f32.to_bits(),
                4.0_f32.to_bits(),
                5.0_f32.to_bits(),
                6.0_f32.to_bits(),
                7.0_f32.to_bits(),
                8.0_f32.to_bits(),
                1,
                1,
                0,
                0,
                0,
                0,
                11.0_f32.to_bits(),
                12.0_f32.to_bits(),
                13.0_f32.to_bits(),
                14.0_f32.to_bits(),
                15.0_f32.to_bits(),
                16.0_f32.to_bits(),
                17.0_f32.to_bits(),
                18.0_f32.to_bits(),
                19.0_f32.to_bits(),
                20.0_f32.to_bits(),
                21.0_f32.to_bits(),
                0,
                22.0_f32.to_bits(),
                23.0_f32.to_bits(),
                24.0_f32.to_bits(),
                25.0_f32.to_bits(),
                26.0_f32.to_bits(),
                27.0_f32.to_bits(),
                28.0_f32.to_bits(),
                29.0_f32.to_bits(),
                30.0_f32.to_bits(),
                31.0_f32.to_bits(),
                32.0_f32.to_bits(),
                33.0_f32.to_bits(),
                34.0_f32.to_bits(),
                0,
            ]
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "../../gpui/src/backdrop_test_fixture.rs"]
mod backdrop_fixture;

#[cfg(all(test, target_os = "linux"))]
#[test]
fn nested_backdrops_sample_ancestor_layers() -> anyhow::Result<()> {
    use gpui::PlatformHeadlessRenderer as _;
    let mut renderer = crate::WgpuHeadlessRenderer::new()?;
    for blur in [3., 12.] {
        for depth in [0, 1, 2] {
            for inside in [false, true] {
                let scene = backdrop_fixture::scene(depth, blur, inside);
                let image = renderer.render_scene_to_image(
                    &scene,
                    gpui::size(gpui::DevicePixels(64), gpui::DevicePixels(64)),
                )?;
                backdrop_fixture::check(&image);
            }
        }
    }
    Ok(())
}
