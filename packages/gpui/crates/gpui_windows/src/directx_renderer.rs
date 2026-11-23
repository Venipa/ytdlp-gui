use std::{
    slice,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use collections::FxHashMap;
use gpui_util::ResultExt;
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
        Graphics::{
            Direct3D::*,
            Direct3D11::*,
            DirectComposition::*,
            DirectWrite::*,
            Dxgi::{Common::*, *},
        },
    },
    core::{HSTRING, Interface},
};

use crate::directx_renderer::shader_resources::{RawShaderBytes, ShaderModule, ShaderTarget};
use crate::*;
use gpui::*;

pub(crate) const DISABLE_DIRECT_COMPOSITION: &str = "GPUI_DISABLE_DIRECT_COMPOSITION";
const RENDER_TARGET_FORMAT: DXGI_FORMAT = DXGI_FORMAT_B8G8R8A8_UNORM;
// This configuration is used for MSAA rendering on paths only, and it's guaranteed to be supported by DirectX 11.
const PATH_MULTISAMPLE_COUNT: u32 = 4;
/// How deeply filtered layers can nest before the innermost ones stop being filtered.
const LAYER_DEPTH: usize = 4;
/// How long an offscreen target may go unused before its memory is given back. A frame with a
/// blur, layer or path on screen touches its targets every time, so only targets that nothing
/// on screen draws through are released.
const IDLE_RELEASE: Duration = Duration::from_secs(2);
const MAX_INSTANCE_BUFFER_SIZE: usize = 256 * 1024 * 1024;

pub(crate) struct FontInfo {
    pub gamma_ratios: [f32; 4],
    pub grayscale_enhanced_contrast: f32,
    pub subpixel_enhanced_contrast: f32,
    pub is_bgr: bool,
}

pub(crate) struct DirectXRenderer {
    hwnd: HWND,
    atlas: Arc<DirectXAtlas>,
    devices: Option<DirectXRendererDevices>,
    resources: Option<DirectXResources>,
    globals: DirectXGlobalElements,
    pipelines: DirectXRenderPipelines,
    direct_composition: Option<DirectComposition>,
    font_info: &'static FontInfo,

    width: u32,
    height: u32,

    /// Whether we want to skip drwaing due to device lost events.
    ///
    /// In that case we want to discard the first frame that we draw as we got reset in the middle of a frame
    /// meaning we lost all the allocated gpu textures and scene resources.
    skip_draws: bool,

    /// When the frame being drawn started, which stamps every offscreen target it touches.
    frame_time: Instant,
}

/// Direct3D objects
#[derive(Clone)]
pub(crate) struct DirectXRendererDevices {
    pub(crate) adapter: IDXGIAdapter1,
    pub(crate) dxgi_factory: IDXGIFactory6,
    pub(crate) device: ID3D11Device,
    pub(crate) device_context: ID3D11DeviceContext,
    dxgi_device: Option<IDXGIDevice>,
    annotation: Option<ID3DUserDefinedAnnotation>,
}

struct DirectXResources {
    // Direct3D rendering objects
    swap_chain: IDXGISwapChain1,
    render_target: Option<ID3D11Texture2D>,
    render_target_view: Option<ID3D11RenderTargetView>,

    /// Path intermediate textures (with MSAA), created by the first frame that draws a path.
    paths: Option<PathTargets>,

    // Offscreen targets the frame blurs and masks through
    filters: FilterTargets,

    // Cached viewport
    viewport: D3D11_VIEWPORT,
}

/// A texture the renderer can both draw into and sample from.
#[derive(Clone)]
struct RenderTexture {
    _texture: ID3D11Texture2D,
    view: Option<ID3D11RenderTargetView>,
    source: Option<ID3D11ShaderResourceView>,
}

/// An offscreen target created the first time a frame needs it and released once it has gone
/// `IDLE_RELEASE` without use.
struct Scratch {
    texture: RenderTexture,
    used: Instant,
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
}

/// Offscreen targets a frame needs to blur and mask what it has drawn, all made for one swap
/// chain size. Each one is allocated the first time a frame draws through it, so a frame with
/// one shallow blur pays for the targets it touches rather than for every depth and level.
struct FilterTargets {
    width: u32,
    height: u32,
    frame: Option<Scratch>,
    backdrop: Option<Scratch>,
    layers: [Option<Scratch>; LAYER_DEPTH],
    levels: [Option<Scratch>; KAWASE_LEVELS + 1],
    scratch: Option<Scratch>,
}

impl FilterTargets {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            frame: None,
            backdrop: None,
            layers: std::array::from_fn(|_| None),
            levels: std::array::from_fn(|_| None),
            scratch: None,
        }
    }

    fn slot(&mut self, target: FilterTarget) -> &mut Option<Scratch> {
        match target {
            FilterTarget::Backdrop => &mut self.backdrop,
            FilterTarget::Frame => &mut self.frame,
            FilterTarget::Layer(depth) => &mut self.layers[depth.min(LAYER_DEPTH - 1)],
            FilterTarget::Level(level) => &mut self.levels[level],
            FilterTarget::Scratch => &mut self.scratch,
        }
    }

    /// A target this frame has already drawn through, which every target but the one being
    /// opened is by the time it is read.
    fn get(&self, target: FilterTarget) -> Result<&RenderTexture> {
        let slot = match target {
            FilterTarget::Backdrop => &self.backdrop,
            FilterTarget::Frame => &self.frame,
            FilterTarget::Layer(depth) => &self.layers[depth.min(LAYER_DEPTH - 1)],
            FilterTarget::Level(level) => &self.levels[level],
            FilterTarget::Scratch => &self.scratch,
        };
        slot.as_ref()
            .map(|scratch| &scratch.texture)
            .context("missing filter target")
    }

    /// The target, created at the swap chain's size (shrunk for a blur level) if no frame has
    /// drawn through it since it was last released, and marked as used by the frame at `now`.
    fn touch(
        &mut self,
        device: &ID3D11Device,
        target: FilterTarget,
        now: Instant,
    ) -> Result<RenderTexture> {
        let shrink = target.shrink();
        let (width, height) = (self.width / shrink, self.height / shrink);
        let slot = self.slot(target);
        if slot.is_none() {
            *slot = Some(Scratch {
                texture: create_render_texture(device, width, height)?,
                used: now,
            });
        }
        let scratch = slot.as_mut().context("missing filter target")?;
        scratch.used = now;
        Ok(scratch.texture.clone())
    }

    /// Drops every target no frame has drawn through within `IDLE_RELEASE` of `now`.
    fn release_idle(&mut self, now: Instant) {
        let slots = std::iter::once(&mut self.frame)
            .chain(std::iter::once(&mut self.backdrop))
            .chain(self.layers.iter_mut())
            .chain(self.levels.iter_mut())
            .chain(std::iter::once(&mut self.scratch));
        for slot in slots {
            if slot
                .as_ref()
                .is_some_and(|scratch| now.duration_since(scratch.used) >= IDLE_RELEASE)
            {
                *slot = None;
            }
        }
    }
}

/// The textures paths are rasterized through, made for the swap chain's size.
struct PathTargets {
    texture: ID3D11Texture2D,
    srv: Option<ID3D11ShaderResourceView>,
    msaa_texture: ID3D11Texture2D,
    msaa_view: Option<ID3D11RenderTargetView>,
    used: Instant,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct FilterParams {
    source_bounds: Bounds<ScaledPixels>,
    fade_bounds: Bounds<ScaledPixels>,
    direction: [f32; 2],
    transform_origin: [f32; 2],
    radius: f32,
    scale: f32,
    fade_top: f32,
    fade_bottom: f32,
    fade_left: f32,
    fade_right: f32,
    translation: [f32; 2],
}

const _: () = {
    assert!(std::mem::size_of::<FilterParams>() == 20 * std::mem::size_of::<f32>());
    assert!(std::mem::offset_of!(FilterParams, source_bounds) == 0);
    assert!(std::mem::offset_of!(FilterParams, fade_bounds) == 16);
    assert!(std::mem::offset_of!(FilterParams, direction) == 32);
    assert!(std::mem::offset_of!(FilterParams, transform_origin) == 40);
    assert!(std::mem::offset_of!(FilterParams, radius) == 48);
    assert!(std::mem::offset_of!(FilterParams, scale) == 52);
    assert!(std::mem::offset_of!(FilterParams, fade_top) == 56);
    assert!(std::mem::offset_of!(FilterParams, fade_bottom) == 60);
    assert!(std::mem::offset_of!(FilterParams, fade_left) == 64);
    assert!(std::mem::offset_of!(FilterParams, fade_right) == 68);
    assert!(std::mem::offset_of!(FilterParams, translation) == 72);
};

#[derive(Clone, Copy)]
enum Pass {
    Gaussian,
    KawaseDown,
    KawaseUp,
}

struct DirectXRenderPipelines {
    backdrop_pipeline: PipelineState<Backdrop>,
    backdrop_punch: Shading,
    gaussian_pipeline: PipelineState<FilterParams>,
    kawase_down_pipeline: PipelineState<FilterParams>,
    kawase_up_pipeline: PipelineState<FilterParams>,
    blit_pipeline: PipelineState<FilterParams>,
    composite_pipeline: PipelineState<FilterParams>,
    mask_pipeline: PipelineState<FilterParams>,
    shadow_pipeline: PipelineState<Shadow>,
    quad_pipeline: PipelineState<Quad>,
    path_rasterization_pipeline: PipelineState<PathRasterizationSprite>,
    path_sprite_pipeline: PipelineState<PathSprite>,
    underline_pipeline: PipelineState<Underline>,
    mono_sprites: PipelineState<MonochromeSprite>,
    subpixel_sprites: PipelineState<SubpixelSprite>,
    subpixel_sprites_layered: Shading,
    poly_sprites: PipelineState<PolychromeSprite>,
    custom_shaders: CustomShaders,
}

struct DirectXGlobalElements {
    global_params_buffer: Option<ID3D11Buffer>,
    batch_params_buffer: Option<ID3D11Buffer>,
    sampler: Option<ID3D11SamplerState>,
    /// Clamps to the edge, for filter and compositing passes. A blur tap past the bottom of a
    /// target would otherwise wrap to its top, and the transparent rows it reads there show the
    /// desktop through an edge the blurred element touches.
    filter_sampler: Option<ID3D11SamplerState>,
}

struct Annotation<'a>(&'a ID3DUserDefinedAnnotation);

impl<'a> Annotation<'a> {
    fn new(annotation: &'a ID3DUserDefinedAnnotation, label: HSTRING) -> Self {
        unsafe { annotation.BeginEvent(&label) };
        Self(annotation)
    }
}

impl Drop for Annotation<'_> {
    fn drop(&mut self) {
        unsafe { self.0.EndEvent() };
    }
}

struct DirectComposition {
    comp_device: IDCompositionDevice,
    comp_target: IDCompositionTarget,
    comp_visual: IDCompositionVisual,
}

impl DirectXRendererDevices {
    pub(crate) fn new(
        directx_devices: &DirectXDevices,
        disable_direct_composition: bool,
    ) -> Result<Self> {
        let DirectXDevices {
            adapter,
            dxgi_factory,
            device,
            device_context,
        } = directx_devices;
        let dxgi_device = if disable_direct_composition {
            None
        } else {
            Some(device.cast().context("Creating DXGI device")?)
        };
        let annotation = device_context.cast().ok();

        Ok(Self {
            adapter: adapter.clone(),
            dxgi_factory: dxgi_factory.clone(),
            device: device.clone(),
            device_context: device_context.clone(),
            dxgi_device,
            annotation,
        })
    }
}

impl DirectXRenderer {
    pub(crate) fn new(
        hwnd: HWND,
        directx_devices: &DirectXDevices,
        disable_direct_composition: bool,
    ) -> Result<Self> {
        if disable_direct_composition {
            log::info!("Direct Composition is disabled.");
        }

        let devices = DirectXRendererDevices::new(directx_devices, disable_direct_composition)
            .context("Creating DirectX devices")?;
        let atlas = Arc::new(DirectXAtlas::new(&devices.device, &devices.device_context));

        let resources = DirectXResources::new(&devices, 1, 1, hwnd, disable_direct_composition)
            .context("Creating DirectX resources")?;
        let globals = DirectXGlobalElements::new(&devices.device)
            .context("Creating DirectX global elements")?;
        let pipelines = DirectXRenderPipelines::new(&devices.device)
            .context("Creating DirectX render pipelines")?;

        let direct_composition = if disable_direct_composition {
            None
        } else {
            let composition = DirectComposition::new(devices.dxgi_device.as_ref().unwrap(), hwnd)
                .context("Creating DirectComposition")?;
            composition
                .set_swap_chain(&resources.swap_chain)
                .context("Setting swap chain for DirectComposition")?;
            Some(composition)
        };

        Ok(DirectXRenderer {
            hwnd,
            atlas,
            devices: Some(devices),
            resources: Some(resources),
            globals,
            pipelines,
            direct_composition,
            font_info: Self::get_font_info(),
            width: 1,
            height: 1,
            skip_draws: false,
            frame_time: Instant::now(),
        })
    }

    pub(crate) fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.atlas.clone()
    }

    fn pre_draw(&self, clear_color: &[f32; 4]) -> Result<()> {
        let resources = self.resources.as_ref().expect("resources missing");
        let device_context = &self
            .devices
            .as_ref()
            .expect("devices missing")
            .device_context;
        update_buffer(
            device_context,
            self.globals.global_params_buffer.as_ref().unwrap(),
            &[GlobalParams {
                gamma_ratios: self.font_info.gamma_ratios,
                viewport_size: [resources.viewport.Width, resources.viewport.Height],
                grayscale_enhanced_contrast: self.font_info.grayscale_enhanced_contrast,
                subpixel_enhanced_contrast: self.font_info.subpixel_enhanced_contrast,
                is_bgr: self.font_info.is_bgr as u32,
                _pad: [0; 3],
            }],
        )?;
        unsafe {
            device_context.ClearRenderTargetView(
                resources
                    .render_target_view
                    .as_ref()
                    .context("missing render target view")?,
                clear_color,
            );
            device_context
                .OMSetRenderTargets(Some(slice::from_ref(&resources.render_target_view)), None);
            device_context.RSSetViewports(Some(slice::from_ref(&resources.viewport)));
            device_context
                .VSSetConstantBuffers(0, Some(slice::from_ref(&self.globals.global_params_buffer)));
            device_context
                .VSSetConstantBuffers(1, Some(slice::from_ref(&self.globals.batch_params_buffer)));
            device_context
                .PSSetConstantBuffers(0, Some(slice::from_ref(&self.globals.global_params_buffer)));
        }
        // The rasterizer clips to the scissor rect, which starts every frame empty.
        self.reset_scissor()?;

        Ok(())
    }

    #[inline]
    /// The handles a filter pass needs, cloned so the pipelines can be borrowed alongside them.
    fn pipeline_handles(
        &self,
    ) -> Result<(
        ID3D11Device,
        ID3D11DeviceContext,
        ID3D11SamplerState,
        ID3D11Buffer,
    )> {
        let devices = self.devices.as_ref().context("devices missing")?;

        Ok((
            devices.device.clone(),
            devices.device_context.clone(),
            self.globals
                .filter_sampler
                .as_ref()
                .context("missing filter sampler")?
                .clone(),
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("missing batch params")?
                .clone(),
        ))
    }

    /// Points the pipeline at one of the offscreen targets, or at the frame when `depth` is none.
    fn open_target(&self, depth: Option<usize>) -> Result<()> {
        let resources = self.resources.as_ref().context("resources missing")?;
        let devices = self.devices.as_ref().context("devices missing")?;
        let view = match depth {
            Some(depth) => &resources.filters.get(FilterTarget::Layer(depth))?.view,
            None => &resources.filters.get(FilterTarget::Frame)?.view,
        };
        unsafe {
            devices
                .device_context
                .OMSetRenderTargets(Some(slice::from_ref(view)), None);
            devices
                .device_context
                .RSSetViewports(Some(slice::from_ref(&resources.viewport)));
        }
        self.reset_scissor()?;

        Ok(())
    }

    /// Points the pipeline back at what the batch in hand is drawing into: the innermost open
    /// layer, or the frame when none is open. Anything that binds a target of its own has to come
    /// back through here, or the primitives after it land in the window while the rest of the
    /// frame is still being assembled somewhere else.
    fn resume_target(&self, stack: &[usize], mirrored: bool) -> Result<()> {
        match stack.len().checked_sub(1) {
            Some(depth) => self.open_target(Some(depth)),
            None => self.restore_frame(mirrored),
        }
    }

    /// Clears one of the offscreen layer targets, creating it if it was released, and points the
    /// pipeline at it.
    fn open_layer(&mut self, depth: usize) -> Result<()> {
        let target = self.filter_target(FilterTarget::Layer(depth))?;
        let devices = self.devices.as_ref().context("devices missing")?;
        let view = target.view.as_ref().context("missing layer target")?;
        unsafe {
            devices
                .device_context
                .ClearRenderTargetView(view, &[0.0f32; 4]);
        }

        self.open_target(Some(depth))
    }

    /// Draws a filtered layer back into its parent, clipped to the layer's bounds.
    fn close_layer(
        &mut self,
        stack: &[usize],
        layer: LayerEffect,
        clip: Bounds<ScaledPixels>,
        mirrored: bool,
    ) -> Result<()> {
        let depth = stack.len();
        let source = {
            let resources = self.resources.as_ref().context("resources missing")?;
            resources
                .filters
                .get(FilterTarget::Layer(depth))?
                .source
                .clone()
        };
        let blurred = match layer.filter.blurs() {
            true => self.blur_source(
                source,
                Some(depth),
                layer.filter.blur,
                Some(layer.blur_bounds(clip)),
            )?,
            false => source,
        };

        match depth.checked_sub(1) {
            Some(onto) => self.open_target(Some(onto))?,
            None => self.restore_frame(mirrored)?,
        }
        let Some(within) = self.scissor(layer.destination_scissor_bounds(clip), 1)? else {
            return Ok(());
        };

        let (device, device_context, sampler, batch_params) = self.pipeline_handles()?;
        let params = [FilterParams {
            source_bounds: if layer.filter.transforms() {
                layer.source_bounds
            } else {
                clip
            },
            // Preserve the existing DirectX fade clip for unscaled layers. A scaled layer applies
            // the fade in source coordinates before the final transform.
            fade_bounds: if layer.filter.transforms() {
                layer.source_bounds
            } else {
                clip
            },
            transform_origin: [layer.transform_origin.x.0, layer.transform_origin.y.0],
            translation: [layer.filter.translate.x.0, layer.filter.translate.y.0],
            scale: layer.filter.scale,
            fade_top: layer.filter.fade_top,
            fade_bottom: layer.filter.fade_bottom,
            fade_left: layer.filter.fade_left,
            fade_right: layer.filter.fade_right,
            ..Default::default()
        }];

        self.set_scissor(within);
        let pipeline = match layer.filter.fades() {
            true => &mut self.pipelines.mask_pipeline,
            false => &mut self.pipelines.composite_pipeline,
        };
        pipeline.update_buffer(&device, &device_context, &params)?;
        pipeline.draw_range_with_texture(
            &device_context,
            None,
            slice::from_ref(&blurred),
            &batch_params,
            slice::from_ref(&Some(sampler)),
            0,
            1,
        )?;
        self.reset_scissor()?;

        Ok(())
    }

    /// Blurs a source and returns the target holding the result. A narrow blur comes back at
    /// full size, a wide one at the level [`Kawase::settled`] names, and whatever samples it
    /// stretches it back up. A source that is the layer at depth `spare` is read by nothing
    /// else, so a gaussian writes its result back into it rather than into a full-size target of
    /// its own.
    fn blur_source(
        &mut self,
        source: Option<ID3D11ShaderResourceView>,
        spare: Option<usize>,
        sigma: f32,
        clip: Option<Bounds<ScaledPixels>>,
    ) -> Result<Option<ID3D11ShaderResourceView>> {
        match BlurPasses::for_sigma(sigma) {
            BlurPasses::Gaussian(sigma) => self.gaussian_blur(source, spare, sigma, clip),
            BlurPasses::Kawase(kawase) => self.kawase_blur(source, kawase, clip),
        }
    }

    /// Blurs a source across into the scratch target and down into level zero, or back into the
    /// `spare` layer it came from, both at full size. The pass across covers the clip grown by
    /// the kernel, since the pass down reads that far above and below it.
    fn gaussian_blur(
        &mut self,
        source: Option<ID3D11ShaderResourceView>,
        spare: Option<usize>,
        sigma: f32,
        clip: Option<Bounds<ScaledPixels>>,
    ) -> Result<Option<ID3D11ShaderResourceView>> {
        let across = FilterParams {
            direction: [1., 0.],
            radius: sigma,
            ..Default::default()
        };
        let down = FilterParams {
            direction: [0., 1.],
            radius: sigma,
            ..Default::default()
        };
        let region = |renderer: &Self, margin: f32| match clip {
            Some(clip) => renderer.scissor(clip.dilate(ScaledPixels(margin)), 1),
            None => Ok(None),
        };
        let reach = (sigma * GAUSSIAN_REACH).ceil() + 1.;

        let within = region(self, reach)?;
        let scratch = self.filter_pass(
            Pass::Gaussian,
            source,
            FilterTarget::Scratch,
            Some(across),
            within,
        )?;
        let within = region(self, 1.)?;
        self.filter_pass(
            Pass::Gaussian,
            scratch,
            spare.map_or(FilterTarget::Level(0), FilterTarget::Layer),
            Some(down),
            within,
        )
    }

    /// Blurs a source with a dual Kawase blur and returns the level it settled at.
    fn kawase_blur(
        &mut self,
        source: Option<ID3D11ShaderResourceView>,
        kawase: Kawase,
        clip: Option<Bounds<ScaledPixels>>,
    ) -> Result<Option<ID3D11ShaderResourceView>> {
        let params = FilterParams {
            radius: kawase.offset,
            ..Default::default()
        };
        // Every pass covers the clip grown by what all the passes together read. The targets are
        // never cleared, so a pass reads stale texels past the edge of what the one before it
        // wrote, but they only creep inwards by what the later passes read, which this margin
        // already holds. A texel more keeps the final bilinear sample on the clip's edge clean.
        let reach = kawase.reach();
        let region = |renderer: &Self, level: usize| match clip {
            Some(clip) => {
                let shrink = 1u32 << level;
                renderer.scissor(clip.dilate(ScaledPixels(reach + shrink as f32)), shrink)
            }
            None => Ok(None),
        };

        let mut from = source;
        for level in 1..=kawase.levels {
            let within = region(self, level)?;
            from = self.filter_pass(
                Pass::KawaseDown,
                from,
                FilterTarget::Level(level),
                Some(params),
                within,
            )?;
        }
        let within = region(self, kawase.settled)?;
        self.filter_pass(
            Pass::KawaseUp,
            from,
            FilterTarget::Level(kawase.settled),
            Some(params),
            within,
        )
    }

    /// Draws one full-screen pass of a filter into a blur target, returning what it wrote. The
    /// target is not cleared first, since the pass writes every texel it covers.
    fn filter_pass(
        &mut self,
        pass: Pass,
        source: Option<ID3D11ShaderResourceView>,
        target: FilterTarget,
        params: Option<FilterParams>,
        within: Option<[u32; 4]>,
    ) -> Result<Option<ID3D11ShaderResourceView>> {
        let shrink = target.shrink();
        let target = self.filter_target(target)?;
        let (view, held, viewport) = {
            let resources = self.resources.as_ref().context("resources missing")?;
            let (width, height) = level_size(&resources.viewport, shrink);
            (
                target.view,
                target.source,
                D3D11_VIEWPORT {
                    TopLeftX: 0.,
                    TopLeftY: 0.,
                    Width: width as f32,
                    Height: height as f32,
                    MinDepth: 0.,
                    MaxDepth: 1.,
                },
            )
        };
        let (device, device_context, sampler, batch_params) = self.pipeline_handles()?;

        unsafe {
            device_context.OMSetRenderTargets(Some(slice::from_ref(&view)), None);
            device_context.RSSetViewports(Some(slice::from_ref(&viewport)));
        }
        match within {
            Some(within) => self.set_scissor(within),
            None => self.set_scissor([0, 0, viewport.Width as u32, viewport.Height as u32]),
        }

        let pipeline = match pass {
            Pass::Gaussian => &mut self.pipelines.gaussian_pipeline,
            Pass::KawaseDown => &mut self.pipelines.kawase_down_pipeline,
            Pass::KawaseUp => &mut self.pipelines.kawase_up_pipeline,
        };
        pipeline.update_buffer(&device, &device_context, &[params.unwrap_or_default()])?;
        pipeline.draw_range_with_texture(
            &device_context,
            None,
            slice::from_ref(&source),
            &batch_params,
            slice::from_ref(&Some(sampler)),
            0,
            1,
        )?;

        Ok(held)
    }

    /// Flattens open layers in source coordinates without changing their eventual composites.
    fn backdrop_source(
        &mut self,
        depth: usize,
        clip: Option<Bounds<ScaledPixels>>,
    ) -> Result<Option<ID3D11ShaderResourceView>> {
        let frame = self.filter_target(FilterTarget::Frame)?;
        if depth == 0 {
            return Ok(frame.source);
        }
        let target = self.filter_target(FilterTarget::Backdrop)?;
        let mut sources = vec![frame.source];
        for level in 0..depth {
            sources.push(self.filter_target(FilterTarget::Layer(level))?.source);
        }
        let (device, context, sampler, batch_params) = self.pipeline_handles()?;
        let viewport = self
            .resources
            .as_ref()
            .context("resources missing")?
            .viewport;
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(viewport.Width), ScaledPixels(viewport.Height)),
        );
        let params = [FilterParams {
            source_bounds: bounds,
            fade_bounds: bounds,
            scale: 1.,
            ..Default::default()
        }];
        unsafe {
            context.OMSetRenderTargets(Some(slice::from_ref(&target.view)), None);
            context.ClearRenderTargetView(
                target.view.as_ref().context("missing backdrop target")?,
                &[0.; 4],
            );
            context.RSSetViewports(Some(slice::from_ref(&viewport)));
        }
        match clip
            .map(|clip| self.scissor(clip, 1))
            .transpose()?
            .flatten()
        {
            Some(within) => self.set_scissor(within),
            None => self.reset_scissor()?,
        }
        self.pipelines
            .composite_pipeline
            .update_buffer(&device, &context, &params)?;
        for source in sources {
            self.pipelines.composite_pipeline.draw_range_with_texture(
                &context,
                None,
                slice::from_ref(&source),
                &batch_params,
                slice::from_ref(&Some(sampler.clone())),
                0,
                1,
            )?;
        }
        Ok(target.source)
    }

    /// Blurs the frame within `clip`, the union of the batch's backdrops grown by the kernel
    /// inside `blur_source`, and draws the batch over it. A backdrop samples only within its
    /// own bounds, so nothing outside the union is worth blurring.
    fn draw_backdrops(
        &mut self,
        first: usize,
        count: usize,
        sigma: f32,
        clip: Option<Bounds<ScaledPixels>>,
        mirrored: bool,
        stack: &[usize],
    ) -> Result<()> {
        let frame = self.backdrop_source(
            stack.len(),
            clip.map(|clip| clip.dilate(BlurPasses::for_sigma(sigma).source_margin())),
        )?;
        let blurred = self.blur_source(frame, None, sigma, clip)?;
        self.resume_target(stack, mirrored)?;

        let (_, device_context, sampler, batch_params) = self.pipeline_handles()?;

        // The hole first, the blurred copy into it second: together they are
        // `dst = mix(dst, blurred, k)`, which leaves a see-through window as clear as it was.
        self.pipelines.backdrop_pipeline.draw_range_with_texture(
            &device_context,
            Some(&self.pipelines.backdrop_punch),
            slice::from_ref(&blurred),
            &batch_params,
            slice::from_ref(&Some(sampler.clone())),
            first as u32,
            count as u32,
        )?;
        self.pipelines.backdrop_pipeline.draw_range_with_texture(
            &device_context,
            None,
            slice::from_ref(&blurred),
            &batch_params,
            slice::from_ref(&Some(sampler)),
            first as u32,
            count as u32,
        )
    }

    /// Points the pipeline back at what the frame is drawn into, offscreen or not.
    fn restore_frame(&self, mirrored: bool) -> Result<()> {
        if mirrored {
            return self.open_target(None);
        }

        let resources = self.resources.as_ref().context("resources missing")?;
        let devices = self.devices.as_ref().context("devices missing")?;
        unsafe {
            devices
                .device_context
                .OMSetRenderTargets(Some(slice::from_ref(&resources.render_target_view)), None);
            devices
                .device_context
                .RSSetViewports(Some(slice::from_ref(&resources.viewport)));
        }
        self.reset_scissor()?;

        Ok(())
    }

    /// Copies the offscreen frame onto the swap chain.
    fn blit_frame(&mut self) -> Result<()> {
        let source = {
            let resources = self.resources.as_ref().context("resources missing")?;
            resources.filters.get(FilterTarget::Frame)?.source.clone()
        };
        self.restore_frame(false)?;

        let (device, device_context, sampler, batch_params) = self.pipeline_handles()?;

        self.pipelines.blit_pipeline.update_buffer(
            &device,
            &device_context,
            &[FilterParams::default()],
        )?;
        self.pipelines.blit_pipeline.draw_range_with_texture(
            &device_context,
            None,
            slice::from_ref(&source),
            &batch_params,
            slice::from_ref(&Some(sampler)),
            0,
            1,
        )
    }

    /// Clamps a region to the target it is drawn into, shrunk to the blur step it runs at. Every
    /// texel the region touches is covered, so a blur level is written out to the frame's edge
    /// even where the region starts part way into a texel.
    fn scissor(&self, bounds: Bounds<ScaledPixels>, shrink: u32) -> Result<Option<[u32; 4]>> {
        let resources = self.resources.as_ref().context("resources missing")?;
        let (width, height) = level_size(&resources.viewport, shrink);
        let shrink = shrink as f32;
        let edge = |at: f32, round: fn(f32) -> f32, limit: u32| {
            (round(at / shrink).max(0.) as u32).min(limit)
        };
        let left = edge(bounds.origin.x.0, f32::floor, width);
        let top = edge(bounds.origin.y.0, f32::floor, height);
        let right = edge(bounds.origin.x.0 + bounds.size.width.0, f32::ceil, width);
        let bottom = edge(bounds.origin.y.0 + bounds.size.height.0, f32::ceil, height);
        if right <= left || bottom <= top {
            return Ok(None);
        }

        Ok(Some([left, top, right - left, bottom - top]))
    }

    fn set_scissor(&self, within: [u32; 4]) {
        let Some(devices) = self.devices.as_ref() else {
            return;
        };
        let [left, top, width, height] = within;
        unsafe {
            devices.device_context.RSSetScissorRects(Some(&[RECT {
                left: left as i32,
                top: top as i32,
                right: (left + width) as i32,
                bottom: (top + height) as i32,
            }]));
        }
    }

    fn reset_scissor(&self) -> Result<()> {
        let resources = self.resources.as_ref().context("resources missing")?;
        self.set_scissor([
            0,
            0,
            resources.viewport.Width as u32,
            resources.viewport.Height as u32,
        ]);

        Ok(())
    }

    fn present(&mut self) -> Result<()> {
        let result = unsafe {
            self.resources
                .as_ref()
                .expect("resources missing")
                .swap_chain
                .Present(0, DXGI_PRESENT(0))
        };
        result.ok().context("Presenting swap chain failed")
    }

    pub(crate) fn handle_device_lost(&mut self, directx_devices: &DirectXDevices) -> Result<()> {
        try_to_recover_from_device_lost(|| {
            self.handle_device_lost_impl(directx_devices)
                .context("DirectXRenderer handling device lost")
        })
    }

    fn handle_device_lost_impl(&mut self, directx_devices: &DirectXDevices) -> Result<()> {
        let disable_direct_composition = self.direct_composition.is_none();

        unsafe {
            #[cfg(debug_assertions)]
            if let Some(devices) = &self.devices {
                report_live_objects(&devices.device)
                    .context("Failed to report live objects after device lost")
                    .log_err();
            }

            self.resources.take();
            if let Some(devices) = &self.devices {
                devices.device_context.OMSetRenderTargets(None, None);
                devices.device_context.ClearState();
                devices.device_context.Flush();
                #[cfg(debug_assertions)]
                report_live_objects(&devices.device)
                    .context("Failed to report live objects after device lost")
                    .log_err();
            }

            self.direct_composition.take();
            self.devices.take();
        }

        let devices = DirectXRendererDevices::new(directx_devices, disable_direct_composition)
            .context("Recreating DirectX devices")?;
        let resources = DirectXResources::new(
            &devices,
            self.width,
            self.height,
            self.hwnd,
            disable_direct_composition,
        )
        .context("Creating DirectX resources")?;
        let globals = DirectXGlobalElements::new(&devices.device)
            .context("Creating DirectXGlobalElements")?;
        let pipelines = DirectXRenderPipelines::new(&devices.device)
            .context("Creating DirectXRenderPipelines")?;

        let direct_composition = if disable_direct_composition {
            None
        } else {
            let composition =
                DirectComposition::new(devices.dxgi_device.as_ref().unwrap(), self.hwnd)?;
            composition.set_swap_chain(&resources.swap_chain)?;
            Some(composition)
        };

        self.atlas
            .handle_device_lost(&devices.device, &devices.device_context);

        unsafe {
            devices
                .device_context
                .OMSetRenderTargets(Some(slice::from_ref(&resources.render_target_view)), None);
        }
        self.devices = Some(devices);
        self.resources = Some(resources);
        self.globals = globals;
        self.pipelines = pipelines;
        self.direct_composition = direct_composition;
        self.skip_draws = true;
        Ok(())
    }

    pub(crate) fn draw(
        &mut self,
        scene: &Scene,
        background_appearance: WindowBackgroundAppearance,
    ) -> Result<()> {
        if self.skip_draws {
            // skip drawing this frame, we just recovered from a device lost event
            // and so likely do not have the textures anymore that are required for drawing
            return Ok(());
        }
        self.render(scene, background_appearance)?;
        let presented = self.present();
        self.release_targets_idle_at(self.frame_time);
        presented
    }

    /// Clear the render target for `background_appearance` and encode every
    /// primitive batch of `scene` into it, without presenting. Shared by
    /// [`draw`](Self::draw) (which then presents) and
    /// [`render_to_image`](Self::render_to_image) (which reads the target back
    /// instead), so the two cannot drift.
    fn render(
        &mut self,
        scene: &Scene,
        background_appearance: WindowBackgroundAppearance,
    ) -> Result<()> {
        self.frame_time = Instant::now();
        self.pre_draw(&match background_appearance {
            WindowBackgroundAppearance::Opaque => [1.0f32; 4],
            _ => [0.0f32; 4],
        })?;
        self.upload_scene_buffers(scene)?;

        // Backdrops read what the frame has drawn so far, which the swap chain cannot be sampled
        // from, so a frame carrying them is drawn offscreen and blitted back at the end.
        let mirrored = !scene.backdrops.is_empty();
        if mirrored {
            let cleared = self.filter_target(FilterTarget::Frame)?.view;
            let devices = self.devices.as_ref().context("devices missing")?;
            if let Some(cleared) = cleared.as_ref() {
                unsafe {
                    devices
                        .device_context
                        .ClearRenderTargetView(cleared, &[0.0f32; 4])
                };
            }
            self.open_target(None)?;
        }

        let mut stack: Vec<usize> = Vec::new();
        // What each open layer will be composited through: neighbours that ask for the same filter
        // share one target, so a list of separately blurred rows costs one pass, not one per row.
        let mut spans: Vec<Bounds<ScaledPixels>> = Vec::new();

        let annotation = self
            .devices
            .as_ref()
            .and_then(|devices| devices.annotation.clone())
            .filter(|annotation| unsafe { annotation.GetStatus().as_bool() });
        for batch in scene.batches() {
            let _annotation = annotation
                .as_ref()
                .map(|annotation| Annotation::new(annotation, HSTRING::from(batch.label())));

            let wanted = scene
                .filtered(scene.batch_order(&batch))
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
                    let layer = scene.effects[*open];
                    if layer.can_merge_with(&next) {
                        *open = wanted[shared];
                        *span = span.union(&next.destination_clip());
                        merged = true;
                        break;
                    }
                }

                let index = stack.pop().expect("the stack is not empty");
                let layer = scene.effects[index];
                let clip = spans.pop().unwrap_or_else(|| layer.destination_clip());
                self.close_layer(&stack, layer, clip, mirrored)?;
            }

            for index in wanted.iter().skip(shared + usize::from(merged)) {
                if stack.len() >= LAYER_DEPTH {
                    break;
                }
                let layer = scene.effects[*index];
                self.open_layer(stack.len())?;
                stack.push(*index);
                spans.push(layer.destination_clip());
            }

            match batch {
                PrimitiveBatch::Shadows(range) => self.draw_shadows(range.start, range.len()),
                PrimitiveBatch::Quads(range) => self.draw_quads(range.start, range.len()),
                PrimitiveBatch::Shaders { shader, range } => {
                    self.draw_custom_shaders(shader, range.start, range.len())
                }
                PrimitiveBatch::Paths(range) => {
                    let paths = &scene.paths[range];
                    self.draw_paths_to_intermediate(paths)?;
                    self.resume_target(&stack, mirrored)?;
                    self.draw_paths_from_intermediate(paths)
                }
                PrimitiveBatch::Underlines(range) => self.draw_underlines(range.start, range.len()),
                PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                    self.draw_monochrome_sprites(texture_id, range.start, range.len())
                }
                PrimitiveBatch::SubpixelSprites { texture_id, range } => {
                    let into_layer = !stack.is_empty();
                    self.draw_subpixel_sprites(texture_id, range.start, range.len(), into_layer)
                }
                PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                    self.draw_polychrome_sprites(texture_id, range.start, range.len())
                }
                PrimitiveBatch::Backdrops(range) => {
                    {
                        let backdrops = &scene.backdrops[range.clone()];
                        let sigma = backdrops
                            .iter()
                            .fold(0., |widest: f32, backdrop| widest.max(backdrop.blur));
                        let clip = backdrops
                            .iter()
                            .map(|backdrop| backdrop.bounds)
                            .reduce(|union, bounds| union.union(&bounds));
                        self.draw_backdrops(range.start, range.len(), sigma, clip, mirrored, &stack)?;
                    }
                    Ok(())
                }
                PrimitiveBatch::Surfaces(range) => self.draw_surfaces(&scene.surfaces[range]),
            }
            .with_context(|| {
                format!(
                    "scene too large:\
                    {} paths, {} shadows, {} quads, {} underlines, {} mono, {} subpixel, {} poly, {} surfaces",
                    scene.paths.len(),
                    scene.shadows.len(),
                    scene.quads.len(),
                    scene.underlines.len(),
                    scene.monochrome_sprites.len(),
                    scene.subpixel_sprites.len(),
                    scene.polychrome_sprites.len(),
                    scene.surfaces.len(),
                )
            })?;
        }

        while let Some(index) = stack.pop() {
            let layer = scene.effects[index];
            let clip = spans.pop().unwrap_or_else(|| layer.destination_clip());
            self.close_layer(&stack, layer, clip, mirrored)?;
        }

        if mirrored {
            self.blit_frame()?;
        }

        Ok(())
    }

    /// Frees offscreen targets that no frame has drawn through for `IDLE_RELEASE`. Every frame
    /// checks this itself, so a caller only needs it for a window that has stopped drawing.
    pub(crate) fn release_idle_targets(&mut self) {
        self.release_targets_idle_at(Instant::now());
    }

    /// Drops every offscreen target that no frame has drawn through within `IDLE_RELEASE` of
    /// `now`. The device context keeps a texture alive until the work that uses it is done, so
    /// this is safe while earlier frames are still in flight.
    fn release_targets_idle_at(&mut self, now: Instant) {
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        resources.filters.release_idle(now);
        if resources
            .paths
            .as_ref()
            .is_some_and(|paths| now.duration_since(paths.used) >= IDLE_RELEASE)
        {
            resources.paths = None;
        }
    }

    /// One filter target, created if no frame has drawn through it since it was last released.
    fn filter_target(&mut self, target: FilterTarget) -> Result<RenderTexture> {
        let now = self.frame_time;
        let device = &self.devices.as_ref().context("devices missing")?.device;
        self.resources
            .as_mut()
            .context("resources missing")?
            .filters
            .touch(device, target, now)
    }

    /// The path textures, created if no frame has drawn a path since they were last released.
    fn path_targets(&mut self) -> Result<&PathTargets> {
        let now = self.frame_time;
        let (width, height) = (self.width, self.height);
        let device = &self.devices.as_ref().context("devices missing")?.device;
        let resources = self.resources.as_mut().context("resources missing")?;
        if resources.paths.is_none() {
            let (texture, srv) = create_path_intermediate_texture(device, width, height)?;
            let (msaa_texture, msaa_view) =
                create_path_intermediate_msaa_texture_and_view(device, width, height)?;
            resources.paths = Some(PathTargets {
                texture,
                srv,
                msaa_texture,
                msaa_view,
                used: now,
            });
        }
        let paths = resources.paths.as_mut().context("missing path targets")?;
        paths.used = now;
        Ok(paths)
    }

    /// Render `scene` to an offscreen CPU image **without presenting** so
    /// the window need never be shown or visible (the macOS headless path
    /// goes through MetalRenderer; this is the Windows analogue). Draws into
    /// the existing render target, copies it into a `D3D11_USAGE_STAGING`
    /// texture, maps it, and converts BGRA to RGBA.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn render_to_image(
        &mut self,
        scene: &Scene,
        background_appearance: WindowBackgroundAppearance,
    ) -> Result<image::RgbaImage> {
        // A pending device-lost recovery (`skip_draws`) leaves the atlas holding
        // tile references from the previous device; drawing before the forced
        // re-render rebuilds them panics in `DirectXAtlasTextures::texture`.
        anyhow::ensure!(
            !self.skip_draws,
            "render_to_image unavailable while recovering from a lost device"
        );
        self.render(scene, background_appearance)?;

        let devices = self.devices.as_ref().context("devices missing")?;
        let device = &devices.device;
        let context = &devices.device_context;
        let resources = self.resources.as_ref().context("resources missing")?;
        let render_target = resources
            .render_target
            .as_ref()
            .context("render target missing")?;

        // A CPU-readable copy of the render target.
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { render_target.GetDesc(&mut desc) };
        let width = desc.Width;
        let height = desc.Height;
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            MipLevels: 1,
            ArraySize: 1,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            ..desc
        };
        let mut staging: Option<ID3D11Texture2D> = None;
        unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging))? };
        let staging = staging.context("creating staging texture")?;
        unsafe { context.CopyResource(&staging, render_target) };

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))? };
        let row_bytes = (width as usize) * 4;
        let mut pixels = vec![0u8; row_bytes * height as usize];
        // SAFETY: `Map` succeeded, so `pData` points at `RowPitch * height`
        // readable bytes for as long as the mapping is held, and `RowPitch >=
        // row_bytes` (it only ever adds trailing padding). `pixels` is sized
        // `row_bytes * height`, so every copy stays in bounds on both sides,
        // and the regions cannot overlap (`pixels` is a fresh allocation).
        unsafe {
            let src = mapped.pData as *const u8;
            for row in 0..height as usize {
                let s = src.add(row * mapped.RowPitch as usize);
                let d = pixels.as_mut_ptr().add(row * row_bytes);
                std::ptr::copy_nonoverlapping(s, d, row_bytes);
            }
            context.Unmap(&staging, 0);
        }
        // The render target is BGRA; image::RgbaImage expects RGBA.
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        image::RgbaImage::from_raw(width, height, pixels)
            .context("Failed to build RgbaImage from staging readback")
    }

    pub(crate) fn resize(&mut self, new_size: Size<DevicePixels>) -> Result<()> {
        let width = new_size.width.0.max(1) as u32;
        let height = new_size.height.0.max(1) as u32;
        if self.width == width && self.height == height {
            return Ok(());
        }
        self.width = width;
        self.height = height;

        // Clear the render target before resizing
        let devices = self.devices.as_ref().context("devices missing")?;
        unsafe { devices.device_context.OMSetRenderTargets(None, None) };
        let resources = self.resources.as_mut().context("resources missing")?;
        resources.render_target.take();
        resources.render_target_view.take();

        // Resizing the swap chain requires a call to the underlying DXGI adapter, which can return the device removed error.
        // The app might have moved to a monitor that's attached to a different graphics device.
        // When a graphics device is removed or reset, the desktop resolution often changes, resulting in a window size change.
        // But here we just return the error, because we are handling device lost scenarios elsewhere.
        unsafe {
            resources
                .swap_chain
                .ResizeBuffers(
                    BUFFER_COUNT as u32,
                    width,
                    height,
                    RENDER_TARGET_FORMAT,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )
                .context("Failed to resize swap chain")?;
        }

        resources.recreate_resources(devices, width, height)?;

        unsafe {
            devices
                .device_context
                .OMSetRenderTargets(Some(slice::from_ref(&resources.render_target_view)), None);
        }

        Ok(())
    }

    fn upload_scene_buffers(&mut self, scene: &Scene) -> Result<()> {
        let devices = self.devices.as_ref().context("devices missing")?;

        if !scene.shadows.is_empty() {
            self.pipelines.shadow_pipeline.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.shadows,
            )?;
        }

        if !scene.backdrops.is_empty() {
            self.pipelines.backdrop_pipeline.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.backdrops,
            )?;
        }

        if !scene.quads.is_empty() {
            self.pipelines.quad_pipeline.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.quads,
            )?;
        }

        if !scene.shaders.is_empty() {
            self.pipelines.custom_shaders.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.shaders,
            )?;
        }

        if !scene.underlines.is_empty() {
            self.pipelines.underline_pipeline.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.underlines,
            )?;
        }

        if !scene.monochrome_sprites.is_empty() {
            self.pipelines.mono_sprites.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.monochrome_sprites,
            )?;
        }

        if !scene.subpixel_sprites.is_empty() {
            self.pipelines.subpixel_sprites.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.subpixel_sprites,
            )?;
        }

        if !scene.polychrome_sprites.is_empty() {
            self.pipelines.poly_sprites.update_buffer(
                &devices.device,
                &devices.device_context,
                &scene.polychrome_sprites,
            )?;
        }

        Ok(())
    }

    fn draw_shadows(&mut self, start: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        self.pipelines.shadow_pipeline.draw_range(
            &devices.device_context,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            start as u32,
            len as u32,
        )
    }

    fn draw_quads(&mut self, start: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        self.pipelines.quad_pipeline.draw_range(
            &devices.device_context,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            start as u32,
            len as u32,
        )
    }

    fn draw_custom_shaders(&mut self, shader: ShaderId, start: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let viewport = self
            .resources
            .as_ref()
            .context("resources missing")?
            .viewport;
        self.pipelines.custom_shaders.draw(
            &devices.device,
            &devices.device_context,
            shader,
            [viewport.Width, viewport.Height],
            start as u32,
            len as u32,
        )
    }

    fn draw_paths_to_intermediate(&mut self, paths: &[Path<ScaledPixels>]) -> Result<()> {
        if paths.is_empty() {
            return Ok(());
        }

        let (texture, msaa_texture, msaa_view) = {
            let targets = self.path_targets()?;
            (
                targets.texture.clone(),
                targets.msaa_texture.clone(),
                targets.msaa_view.clone(),
            )
        };
        let devices = self.devices.as_ref().context("devices missing")?;
        // Clear intermediate MSAA texture
        unsafe {
            devices.device_context.ClearRenderTargetView(
                msaa_view.as_ref().context("missing path target")?,
                &[0.0; 4],
            );
            // Set intermediate MSAA texture as render target
            devices
                .device_context
                .OMSetRenderTargets(Some(slice::from_ref(&msaa_view)), None);
        }

        // Collect all vertices and sprites for a single draw call
        let mut vertices = Vec::new();

        for path in paths {
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationSprite {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds: path.clipped_bounds(),
            }));
        }

        self.pipelines.path_rasterization_pipeline.update_buffer(
            &devices.device,
            &devices.device_context,
            &vertices,
        )?;

        self.pipelines.path_rasterization_pipeline.draw(
            &devices.device_context,
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
            vertices.len() as u32,
            1,
        )?;

        // Resolve MSAA to non-MSAA intermediate texture
        unsafe {
            devices.device_context.ResolveSubresource(
                &texture,
                0,
                &msaa_texture,
                0,
                RENDER_TARGET_FORMAT,
            );
        }

        Ok(())
    }

    fn draw_paths_from_intermediate(&mut self, paths: &[Path<ScaledPixels>]) -> Result<()> {
        let Some(first_path) = paths.first() else {
            return Ok(());
        };

        // When copying paths from the intermediate texture to the drawable,
        // each pixel must only be copied once, in case of transparent paths.
        //
        // If all paths have the same draw order, then their bounds are all
        // disjoint, so we can copy each path's bounds individually. If this
        // batch combines different draw orders, we perform a single copy
        // for a minimal spanning rect.
        let sprites = if paths.last().unwrap().order == first_path.order {
            paths
                .iter()
                .map(|path| PathSprite {
                    bounds: path.clipped_bounds(),
                })
                .collect::<Vec<_>>()
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            vec![PathSprite { bounds }]
        };

        let devices = self.devices.as_ref().context("devices missing")?;
        let resources = self.resources.as_ref().context("resources missing")?;
        self.pipelines.path_sprite_pipeline.update_buffer(
            &devices.device,
            &devices.device_context,
            &sprites,
        )?;

        // Draw the sprites with the path texture
        self.pipelines.path_sprite_pipeline.draw_with_texture(
            &devices.device_context,
            slice::from_ref(
                &resources
                    .paths
                    .as_ref()
                    .context("missing path targets")?
                    .srv,
            ),
            slice::from_ref(&self.globals.sampler),
            sprites.len() as u32,
        )
    }

    fn draw_underlines(&mut self, start: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        self.pipelines.underline_pipeline.draw_range(
            &devices.device_context,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            start as u32,
            len as u32,
        )
    }

    fn draw_monochrome_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        start: usize,
        len: usize,
    ) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let Some(texture_view) = self.atlas.get_texture_view(texture_id) else {
            return Ok(());
        };
        self.pipelines.mono_sprites.draw_range_with_texture(
            &devices.device_context,
            None,
            &texture_view,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            slice::from_ref(&self.globals.sampler),
            start as u32,
            len as u32,
        )
    }

    fn draw_subpixel_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        start: usize,
        len: usize,
        into_layer: bool,
    ) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let Some(texture_view) = self.atlas.get_texture_view(texture_id) else {
            return Ok(());
        };
        let batch_params = self
            .globals
            .batch_params_buffer
            .as_ref()
            .context("batch params buffer missing")?;
        // See `subpixel_sprite_layered_fragment` for why a layer needs a different blend.
        let shading = match into_layer {
            true => Some(&self.pipelines.subpixel_sprites_layered),
            false => None,
        };
        self.pipelines.subpixel_sprites.draw_range_with_texture(
            &devices.device_context,
            shading,
            &texture_view,
            batch_params,
            slice::from_ref(&self.globals.sampler),
            start as u32,
            len as u32,
        )
    }

    fn draw_polychrome_sprites(
        &mut self,
        texture_id: AtlasTextureId,
        start: usize,
        len: usize,
    ) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let Some(texture_view) = self.atlas.get_texture_view(texture_id) else {
            return Ok(());
        };
        self.pipelines.poly_sprites.draw_range_with_texture(
            &devices.device_context,
            None,
            &texture_view,
            self.globals
                .batch_params_buffer
                .as_ref()
                .context("batch params buffer missing")?,
            slice::from_ref(&self.globals.sampler),
            start as u32,
            len as u32,
        )
    }

    fn draw_surfaces(&mut self, surfaces: &[PaintSurface]) -> Result<()> {
        if surfaces.is_empty() {
            return Ok(());
        }
        Ok(())
    }

    pub(crate) fn gpu_specs(&self) -> Result<GpuSpecs> {
        let devices = self.devices.as_ref().context("devices missing")?;
        let desc = unsafe { devices.adapter.GetDesc1() }?;
        let is_software_emulated = (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0;
        let device_name = String::from_utf16_lossy(&desc.Description)
            .trim_matches(char::from(0))
            .to_string();
        let driver_name = match desc.VendorId {
            0x10DE => "NVIDIA Corporation".to_string(),
            0x1002 => "AMD Corporation".to_string(),
            0x8086 => "Intel Corporation".to_string(),
            id => format!("Unknown Vendor (ID: {:#X})", id),
        };
        let driver_version = match desc.VendorId {
            0x10DE => nvidia::get_driver_version(),
            0x1002 => amd::get_driver_version(),
            // For Intel and other vendors, we use the DXGI API to get the driver version.
            _ => dxgi::get_driver_version(&devices.adapter),
        }
        .context("Failed to get gpu driver info")
        .log_err()
        .unwrap_or("Unknown Driver".to_string());
        Ok(GpuSpecs {
            is_software_emulated,
            device_name,
            driver_name,
            driver_info: driver_version,
        })
    }

    pub(crate) fn get_font_info() -> &'static FontInfo {
        static CACHED_FONT_INFO: OnceLock<FontInfo> = OnceLock::new();
        CACHED_FONT_INFO.get_or_init(|| unsafe {
            let factory: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).unwrap();
            let render_params: IDWriteRenderingParams1 =
                factory.CreateRenderingParams().unwrap().cast().unwrap();
            FontInfo {
                gamma_ratios: gpui::get_gamma_correction_ratios(render_params.GetGamma()),
                grayscale_enhanced_contrast: render_params.GetGrayscaleEnhancedContrast(),
                subpixel_enhanced_contrast: render_params.GetEnhancedContrast(),
                is_bgr: render_params.GetPixelGeometry() == DWRITE_PIXEL_GEOMETRY_BGR,
            }
        })
    }

    pub(crate) fn mark_drawable(&mut self) {
        self.skip_draws = false;
    }
}

impl DirectXResources {
    pub fn new(
        devices: &DirectXRendererDevices,
        width: u32,
        height: u32,
        hwnd: HWND,
        disable_direct_composition: bool,
    ) -> Result<Self> {
        let swap_chain = if disable_direct_composition {
            create_swap_chain(&devices.dxgi_factory, &devices.device, hwnd, width, height)?
        } else {
            create_swap_chain_for_composition(
                &devices.dxgi_factory,
                &devices.device,
                width,
                height,
            )?
        };

        let (render_target, render_target_view, viewport) =
            create_resources(devices, &swap_chain, width, height)?;
        let filters = FilterTargets::new(width, height);
        set_rasterizer_state(&devices.device, &devices.device_context)?;

        Ok(Self {
            swap_chain,
            filters,
            render_target: Some(render_target),
            render_target_view,
            paths: None,
            viewport,
        })
    }

    #[inline]
    fn recreate_resources(
        &mut self,
        devices: &DirectXRendererDevices,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let (render_target, render_target_view, viewport) =
            create_resources(devices, &self.swap_chain, width, height)?;
        // The offscreen targets are rebuilt at the new size by the first frame that needs them.
        self.filters = FilterTargets::new(width, height);
        self.paths = None;
        self.render_target = Some(render_target);
        self.render_target_view = render_target_view;
        self.viewport = viewport;
        Ok(())
    }
}

impl DirectXRenderPipelines {
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        // A backdrop mixes the blurred copy into what it covers instead of laying it over:
        // the punch scales the destination by `1 - k`, the additive draw after it adds
        // `blurred * k`. Laying it over cost a see-through window its transparency.
        let backdrop_pipeline = PipelineState::new(
            device,
            "backdrop_pipeline",
            ShaderModule::Backdrop,
            16,
            create_blend_state_additive(device)?,
        )?;
        let backdrop_punch = Shading::new(
            device,
            ShaderModule::BackdropPunch,
            create_blend_state_punch(device)?,
        )?;
        let gaussian_pipeline = PipelineState::new(
            device,
            "gaussian_pipeline",
            ShaderModule::Gaussian,
            1,
            create_blend_state_opaque(device)?,
        )?;
        let kawase_down_pipeline = PipelineState::new(
            device,
            "kawase_down_pipeline",
            ShaderModule::KawaseDown,
            1,
            create_blend_state_opaque(device)?,
        )?;
        let kawase_up_pipeline = PipelineState::new(
            device,
            "kawase_up_pipeline",
            ShaderModule::KawaseUp,
            1,
            create_blend_state_opaque(device)?,
        )?;
        let blit_pipeline = PipelineState::new(
            device,
            "blit_pipeline",
            ShaderModule::Blit,
            1,
            create_blend_state_opaque(device)?,
        )?;
        let composite_pipeline = PipelineState::new(
            device,
            "composite_pipeline",
            ShaderModule::Mask,
            1,
            create_blend_state_premultiplied(device)?,
        )?;
        let mask_pipeline = PipelineState::new(
            device,
            "mask_pipeline",
            ShaderModule::Mask,
            1,
            create_blend_state_premultiplied(device)?,
        )?;
        let shadow_pipeline = PipelineState::new(
            device,
            "shadow_pipeline",
            ShaderModule::Shadow,
            4,
            create_blend_state(device)?,
        )?;
        let quad_pipeline = PipelineState::new(
            device,
            "quad_pipeline",
            ShaderModule::Quad,
            64,
            create_blend_state(device)?,
        )?;
        let path_rasterization_pipeline = PipelineState::new(
            device,
            "path_rasterization_pipeline",
            ShaderModule::PathRasterization,
            32,
            create_blend_state_for_path_rasterization(device)?,
        )?;
        let path_sprite_pipeline = PipelineState::new(
            device,
            "path_sprite_pipeline",
            ShaderModule::PathSprite,
            4,
            create_blend_state_for_path_sprite(device)?,
        )?;
        let underline_pipeline = PipelineState::new(
            device,
            "underline_pipeline",
            ShaderModule::Underline,
            4,
            create_blend_state(device)?,
        )?;
        let mono_sprites = PipelineState::new(
            device,
            "monochrome_sprite_pipeline",
            ShaderModule::MonochromeSprite,
            512,
            create_blend_state(device)?,
        )?;
        let subpixel_sprites = PipelineState::new(
            device,
            "subpixel_sprite_pipeline",
            ShaderModule::SubpixelSprite,
            512,
            create_blend_state_for_subpixel_rendering(device)?,
        )?;
        // The same glyphs, for when they land in an offscreen layer instead of the frame;
        // see `subpixel_sprite_layered_fragment`.
        let subpixel_sprites_layered = Shading::new(
            device,
            ShaderModule::SubpixelSpriteLayered,
            create_blend_state_premultiplied(device)?,
        )?;
        let poly_sprites = PipelineState::new(
            device,
            "polychrome_sprite_pipeline",
            ShaderModule::PolychromeSprite,
            16,
            create_blend_state(device)?,
        )?;

        Ok(Self {
            backdrop_pipeline,
            backdrop_punch,
            gaussian_pipeline,
            kawase_down_pipeline,
            kawase_up_pipeline,
            blit_pipeline,
            composite_pipeline,
            mask_pipeline,
            shadow_pipeline,
            quad_pipeline,
            path_rasterization_pipeline,
            path_sprite_pipeline,
            underline_pipeline,
            mono_sprites,
            subpixel_sprites,
            subpixel_sprites_layered,
            poly_sprites,
            custom_shaders: CustomShaders::new(device)?,
        })
    }
}

impl DirectComposition {
    pub fn new(dxgi_device: &IDXGIDevice, hwnd: HWND) -> Result<Self> {
        let comp_device = get_comp_device(dxgi_device)?;
        let comp_target = unsafe { comp_device.CreateTargetForHwnd(hwnd, true) }?;
        let comp_visual = unsafe { comp_device.CreateVisual() }?;

        Ok(Self {
            comp_device,
            comp_target,
            comp_visual,
        })
    }

    pub fn set_swap_chain(&self, swap_chain: &IDXGISwapChain1) -> Result<()> {
        unsafe {
            self.comp_visual.SetContent(swap_chain)?;
            self.comp_target.SetRoot(&self.comp_visual)?;
            self.comp_device.Commit()?;
        }
        Ok(())
    }
}

impl DirectXGlobalElements {
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        let global_params_buffer = create_constant_buffer::<GlobalParams>(device)?;
        let batch_params_buffer = create_constant_buffer::<BatchParams>(device)?;

        let sampler = create_sampler(device, D3D11_TEXTURE_ADDRESS_WRAP)?;
        let filter_sampler = create_sampler(device, D3D11_TEXTURE_ADDRESS_CLAMP)?;

        Ok(Self {
            global_params_buffer,
            batch_params_buffer,
            sampler,
            filter_sampler,
        })
    }
}

#[derive(Debug, Default)]
#[repr(C)]
struct GlobalParams {
    gamma_ratios: [f32; 4],
    viewport_size: [f32; 2],
    grayscale_enhanced_contrast: f32,
    subpixel_enhanced_contrast: f32,
    is_bgr: u32,
    _pad: [u32; 3],
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C, align(16))]
struct BatchParams {
    start_index: u32,
    _padding: [u32; 3],
}

const _: () = assert!(std::mem::size_of::<BatchParams>() == 16);

struct PipelineState<T> {
    label: &'static str,
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    buffer: ID3D11Buffer,
    buffer_size: usize,
    view: Option<ID3D11ShaderResourceView>,
    blend_state: ID3D11BlendState,
    _marker: std::marker::PhantomData<T>,
}

/// A second way to shade a pipeline's instances: its own fragment stage and blend over the
/// vertex stage and the instance buffer of the pipeline it is handed to.
struct Shading {
    fragment: ID3D11PixelShader,
    blend_state: ID3D11BlendState,
}

impl Shading {
    /// Takes the fragment stage of `shader_module`. The module needs no vertex entry point of
    /// its own, since the draw keeps the pipeline's.
    fn new(
        device: &ID3D11Device,
        shader_module: ShaderModule,
        blend_state: ID3D11BlendState,
    ) -> Result<Self> {
        let raw_shader = RawShaderBytes::new(shader_module, ShaderTarget::Fragment)?;
        let fragment = create_fragment_shader(device, raw_shader.as_bytes())?;
        Ok(Shading {
            fragment,
            blend_state,
        })
    }
}

impl<T> PipelineState<T> {
    fn new(
        device: &ID3D11Device,
        label: &'static str,
        shader_module: ShaderModule,
        buffer_size: usize,
        blend_state: ID3D11BlendState,
    ) -> Result<Self> {
        let vertex = {
            let raw_shader = RawShaderBytes::new(shader_module, ShaderTarget::Vertex)?;
            create_vertex_shader(device, raw_shader.as_bytes())?
        };
        let fragment = {
            let raw_shader = RawShaderBytes::new(shader_module, ShaderTarget::Fragment)?;
            create_fragment_shader(device, raw_shader.as_bytes())?
        };
        let buffer = create_buffer(device, std::mem::size_of::<T>(), buffer_size)?;
        let view = create_buffer_view(device, &buffer)?;

        Ok(PipelineState {
            label,
            vertex,
            fragment,
            buffer,
            buffer_size,
            view,
            blend_state,
            _marker: std::marker::PhantomData,
        })
    }

    fn update_buffer(
        &mut self,
        device: &ID3D11Device,
        device_context: &ID3D11DeviceContext,
        data: &[T],
    ) -> Result<()> {
        if self.buffer_size < data.len() {
            let element_size = std::mem::size_of::<T>();
            let required_size = std::mem::size_of_val(data);
            anyhow::ensure!(
                required_size <= MAX_INSTANCE_BUFFER_SIZE,
                "{} buffer needs {required_size} bytes, above the maximum of {MAX_INSTANCE_BUFFER_SIZE}",
                self.label
            );
            let new_buffer_size = data
                .len()
                .next_power_of_two()
                .min(MAX_INSTANCE_BUFFER_SIZE / element_size);
            log::debug!(
                "Updating {} buffer size from {} to {}",
                self.label,
                self.buffer_size,
                new_buffer_size
            );
            let buffer = create_buffer(device, std::mem::size_of::<T>(), new_buffer_size)?;
            let view = create_buffer_view(device, &buffer)?;
            self.buffer = buffer;
            self.view = view;
            self.buffer_size = new_buffer_size;
        }
        update_buffer(device_context, &self.buffer, data)
    }

    fn draw(
        &self,
        device_context: &ID3D11DeviceContext,
        topology: D3D_PRIMITIVE_TOPOLOGY,
        vertex_count: u32,
        instance_count: u32,
    ) -> Result<()> {
        set_pipeline_state(
            device_context,
            slice::from_ref(&self.view),
            topology,
            &self.vertex,
            &self.fragment,
            &self.blend_state,
        );
        unsafe {
            device_context.DrawInstanced(vertex_count, instance_count, 0, 0);
        }
        Ok(())
    }

    fn draw_with_texture(
        &self,
        device_context: &ID3D11DeviceContext,
        texture: &[Option<ID3D11ShaderResourceView>],
        sampler: &[Option<ID3D11SamplerState>],
        instance_count: u32,
    ) -> Result<()> {
        set_pipeline_state(
            device_context,
            slice::from_ref(&self.view),
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            &self.vertex,
            &self.fragment,
            &self.blend_state,
        );
        unsafe {
            device_context.PSSetSamplers(0, Some(sampler));
            device_context.VSSetShaderResources(0, Some(texture));
            device_context.PSSetShaderResources(0, Some(texture));

            device_context.DrawInstanced(4, instance_count, 0, 0);
        }
        Ok(())
    }

    fn draw_range(
        &self,
        device_context: &ID3D11DeviceContext,
        batch_params_buffer: &ID3D11Buffer,
        first_instance: u32,
        instance_count: u32,
    ) -> Result<()> {
        anyhow::ensure!(
            first_instance as usize + instance_count as usize <= self.buffer_size,
            "DirectX instance range exceeds the {} buffer",
            self.label
        );
        update_batch_start(device_context, batch_params_buffer, first_instance)?;
        set_pipeline_state(
            device_context,
            slice::from_ref(&self.view),
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            &self.vertex,
            &self.fragment,
            &self.blend_state,
        );
        unsafe {
            device_context.DrawInstanced(4, instance_count, 0, 0);
        }
        Ok(())
    }

    /// Draws a range of this pipeline's instances. `shading` replaces the fragment stage and
    /// the blend for this draw alone, which is how one batch of instances is drawn two ways
    /// without a second copy of it on the GPU.
    fn draw_range_with_texture(
        &self,
        device_context: &ID3D11DeviceContext,
        shading: Option<&Shading>,
        texture: &[Option<ID3D11ShaderResourceView>],
        batch_params_buffer: &ID3D11Buffer,
        sampler: &[Option<ID3D11SamplerState>],
        first_instance: u32,
        instance_count: u32,
    ) -> Result<()> {
        anyhow::ensure!(
            first_instance as usize + instance_count as usize <= self.buffer_size,
            "DirectX instance range exceeds the {} buffer",
            self.label
        );
        let (fragment, blend_state) = match shading {
            Some(shading) => (&shading.fragment, &shading.blend_state),
            None => (&self.fragment, &self.blend_state),
        };
        update_batch_start(device_context, batch_params_buffer, first_instance)?;
        set_pipeline_state(
            device_context,
            slice::from_ref(&self.view),
            D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
            &self.vertex,
            fragment,
            blend_state,
        );
        unsafe {
            device_context.PSSetSamplers(0, Some(sampler));
            device_context.VSSetShaderResources(0, Some(texture));
            device_context.PSSetShaderResources(0, Some(texture));
            device_context.DrawInstanced(4, instance_count, 0, 0);
        }
        Ok(())
    }
}

/// The globals the custom shader prelude reads, laid out as its `GpuiShaderGlobals`.
#[derive(Clone, Copy)]
#[repr(C)]
struct CustomShaderGlobals {
    viewport_size: [f32; 2],
    premultiplied_alpha: u32,
    pad: u32,
}

/// The constants naga adds to `SV_VertexID` and `SV_InstanceID`, which Direct3D 11 counts from
/// zero in every draw, so the shader indexes the instances of its batch.
#[derive(Clone, Copy)]
#[repr(C)]
struct NagaConstants {
    first_vertex: i32,
    first_instance: i32,
    other: u32,
    pad: u32,
}

/// The stages of one custom shader, compiled from its HLSL translation.
struct CustomPipeline {
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
}

/// Everything custom shaders draw with: a raw buffer of the frame's instances, since naga reads
/// storage buffers as `ByteAddressBuffer`, the two constant buffers the prelude reads, and a
/// pipeline per shader, compiled the first time a frame draws it.
struct CustomShaders {
    buffer: Option<ID3D11Buffer>,
    view: Option<ID3D11ShaderResourceView>,
    /// How many instances the buffer holds.
    capacity: usize,
    globals: Option<ID3D11Buffer>,
    constants: Option<ID3D11Buffer>,
    blend_state: ID3D11BlendState,
    /// A shader that failed to compile is kept as `None`, so it is not compiled again every
    /// frame.
    pipelines: FxHashMap<ShaderId, Option<CustomPipeline>>,
}

impl CustomShaders {
    fn new(device: &ID3D11Device) -> Result<Self> {
        Ok(Self {
            buffer: None,
            view: None,
            capacity: 0,
            globals: create_constant_buffer::<CustomShaderGlobals>(device)?,
            constants: create_constant_buffer::<NagaConstants>(device)?,
            blend_state: create_blend_state(device)?,
            pipelines: FxHashMap::default(),
        })
    }

    fn update_buffer(
        &mut self,
        device: &ID3D11Device,
        device_context: &ID3D11DeviceContext,
        data: &[ShaderQuad],
    ) -> Result<()> {
        if self.capacity < data.len() || self.buffer.is_none() {
            let element_size = std::mem::size_of::<ShaderQuad>();
            let required_size = std::mem::size_of_val(data);
            anyhow::ensure!(
                required_size <= MAX_INSTANCE_BUFFER_SIZE,
                "custom shader buffer needs {required_size} bytes, above the maximum of {MAX_INSTANCE_BUFFER_SIZE}"
            );
            let capacity = data
                .len()
                .next_power_of_two()
                .max(16)
                .min(MAX_INSTANCE_BUFFER_SIZE / element_size);
            let buffer = create_raw_buffer(device, element_size * capacity)?;
            self.view = create_raw_buffer_view(device, &buffer, element_size * capacity)?;
            self.buffer = Some(buffer);
            self.capacity = capacity;
        }
        let buffer = self
            .buffer
            .as_ref()
            .context("custom shader buffer missing")?;
        update_buffer(device_context, buffer, data)
    }

    /// Draws `count` instances of `shader` from `first` on, or nothing when the shader does not
    /// compile on this device.
    fn draw(
        &mut self,
        device: &ID3D11Device,
        device_context: &ID3D11DeviceContext,
        shader: ShaderId,
        viewport_size: [f32; 2],
        first: u32,
        count: u32,
    ) -> Result<()> {
        anyhow::ensure!(
            (first + count) as usize <= self.capacity,
            "DirectX instance range exceeds the custom shader buffer"
        );
        let pipeline = self.pipelines.entry(shader).or_insert_with(|| {
            compile_custom_pipeline(device, shader)
                .inspect_err(|error| log::error!("{error:#}"))
                .ok()
        });
        let Some(pipeline) = pipeline else {
            return Ok(());
        };

        // Quads blend straight alpha here, so the prelude leaves its colour unpremultiplied.
        let globals = self
            .globals
            .as_ref()
            .context("custom shader globals missing")?;
        update_buffer(
            device_context,
            globals,
            &[CustomShaderGlobals {
                viewport_size,
                premultiplied_alpha: 0,
                pad: 0,
            }],
        )?;
        let constants = self
            .constants
            .as_ref()
            .context("custom shader constants missing")?;
        update_buffer(
            device_context,
            constants,
            &[NagaConstants {
                first_vertex: 0,
                first_instance: first as i32,
                other: 0,
                pad: 0,
            }],
        )?;

        let globals_slot = gpui::HlslShader::GLOBALS_REGISTER;
        let constants_slot = gpui::HlslShader::CONSTANTS_REGISTER;
        let instances_slot = gpui::HlslShader::INSTANCES_REGISTER;
        unsafe {
            device_context.VSSetConstantBuffers(globals_slot, Some(slice::from_ref(&self.globals)));
            device_context.PSSetConstantBuffers(globals_slot, Some(slice::from_ref(&self.globals)));
            device_context
                .VSSetConstantBuffers(constants_slot, Some(slice::from_ref(&self.constants)));
            device_context
                .PSSetConstantBuffers(constants_slot, Some(slice::from_ref(&self.constants)));
            device_context.VSSetShaderResources(instances_slot, Some(slice::from_ref(&self.view)));
            device_context.PSSetShaderResources(instances_slot, Some(slice::from_ref(&self.view)));
            device_context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            device_context.VSSetShader(&pipeline.vertex, None);
            device_context.PSSetShader(&pipeline.fragment, None);
            device_context.OMSetBlendState(&self.blend_state, None, 0xFFFFFFFF);
            device_context.DrawInstanced(4, count, 0, 0);
        }
        Ok(())
    }
}

/// Translates a custom shader to HLSL and compiles both of its stages for shader model 5.0,
/// which needs a device of feature level 11 or above.
fn compile_custom_pipeline(device: &ID3D11Device, shader: ShaderId) -> Result<CustomPipeline> {
    let program = gpui::shader_program(shader)
        .with_context(|| format!("custom shader {} was never registered", shader.0))?;
    anyhow::ensure!(
        unsafe { device.GetFeatureLevel() }.0 >= D3D_FEATURE_LEVEL_11_0.0,
        "custom shader `{}` needs Direct3D feature level 11",
        program.label
    );
    let hlsl = program.to_hlsl()?;
    let vertex = compile_hlsl(&hlsl.source, &hlsl.vertex_entry, "vs_5_0", &program.label)?;
    let fragment = compile_hlsl(&hlsl.source, &hlsl.fragment_entry, "ps_5_0", &program.label)?;
    Ok(CustomPipeline {
        vertex: create_vertex_shader(device, blob_bytes(&vertex))?,
        fragment: create_fragment_shader(device, blob_bytes(&fragment))?,
    })
}

/// Compiles one entry point of HLSL source with the system's `d3dcompiler_47.dll`.
fn compile_hlsl(
    source: &str,
    entry: &str,
    target: &str,
    label: &str,
) -> Result<windows::Win32::Graphics::Direct3D::ID3DBlob> {
    use windows::Win32::Graphics::Direct3D::{
        Fxc::{D3DCOMPILE_OPTIMIZATION_LEVEL3, D3DCompile},
        ID3DInclude,
    };
    use windows::core::PCSTR;

    let entry = std::ffi::CString::new(entry)?;
    let target = std::ffi::CString::new(target)?;
    let name = std::ffi::CString::new(label)?;
    let mut code = None;
    let mut errors = None;
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            PCSTR::from_raw(name.as_ptr().cast()),
            None,
            None::<&ID3DInclude>,
            PCSTR::from_raw(entry.as_ptr().cast()),
            PCSTR::from_raw(target.as_ptr().cast()),
            D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(error) = result {
        let message = errors
            .as_ref()
            .map(|errors| String::from_utf8_lossy(blob_bytes(errors)).into_owned())
            .unwrap_or_else(|| error.to_string());
        anyhow::bail!("custom shader `{label}` failed to compile: {message}");
    }
    code.context("the shader compiler returned no bytecode")
}

fn blob_bytes(blob: &windows::Win32::Graphics::Direct3D::ID3DBlob) -> &[u8] {
    unsafe { slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize()) }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct PathRasterizationSprite {
    xy_position: Point<ScaledPixels>,
    st_position: Point<f32>,
    color: Background,
    bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct PathSprite {
    bounds: Bounds<ScaledPixels>,
}

impl Drop for DirectXRenderer {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        if let Some(devices) = &self.devices {
            report_live_objects(&devices.device).ok();
        }
    }
}

#[inline]
fn get_comp_device(dxgi_device: &IDXGIDevice) -> Result<IDCompositionDevice> {
    Ok(unsafe { DCompositionCreateDevice(dxgi_device)? })
}

fn create_swap_chain_for_composition(
    dxgi_factory: &IDXGIFactory6,
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<IDXGISwapChain1> {
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: RENDER_TARGET_FORMAT,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: BUFFER_COUNT as u32,
        // Composition SwapChains only support the DXGI_SCALING_STRETCH Scaling.
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Flags: 0,
    };
    Ok(unsafe { dxgi_factory.CreateSwapChainForComposition(device, &desc, None)? })
}

fn create_swap_chain(
    dxgi_factory: &IDXGIFactory6,
    device: &ID3D11Device,
    hwnd: HWND,
    width: u32,
    height: u32,
) -> Result<IDXGISwapChain1> {
    use windows::Win32::Graphics::Dxgi::DXGI_MWA_NO_ALT_ENTER;

    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: width,
        Height: height,
        Format: RENDER_TARGET_FORMAT,
        Stereo: false.into(),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: BUFFER_COUNT as u32,
        Scaling: DXGI_SCALING_NONE,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: 0,
    };
    let swap_chain =
        unsafe { dxgi_factory.CreateSwapChainForHwnd(device, hwnd, &desc, None, None) }?;
    unsafe { dxgi_factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER) }?;
    Ok(swap_chain)
}

#[inline]
fn create_resources(
    devices: &DirectXRendererDevices,
    swap_chain: &IDXGISwapChain1,
    width: u32,
    height: u32,
) -> Result<(
    ID3D11Texture2D,
    Option<ID3D11RenderTargetView>,
    D3D11_VIEWPORT,
)> {
    let (render_target, render_target_view) =
        create_render_target_and_its_view(swap_chain, &devices.device)?;
    let viewport = D3D11_VIEWPORT {
        TopLeftX: 0.0,
        TopLeftY: 0.0,
        Width: width as f32,
        Height: height as f32,
        MinDepth: 0.0,
        MaxDepth: 1.0,
    };
    Ok((render_target, render_target_view, viewport))
}

#[inline]
fn create_render_target_and_its_view(
    swap_chain: &IDXGISwapChain1,
    device: &ID3D11Device,
) -> Result<(ID3D11Texture2D, Option<ID3D11RenderTargetView>)> {
    let render_target: ID3D11Texture2D = unsafe { swap_chain.GetBuffer(0) }?;
    let mut render_target_view = None;
    unsafe { device.CreateRenderTargetView(&render_target, None, Some(&mut render_target_view))? };
    Ok((render_target, render_target_view))
}

#[inline]
fn create_path_intermediate_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(ID3D11Texture2D, Option<ID3D11ShaderResourceView>)> {
    let texture = unsafe {
        let mut output = None;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: RENDER_TARGET_FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        device.CreateTexture2D(&desc, None, Some(&mut output))?;
        output.unwrap()
    };

    let mut shader_resource_view = None;
    unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut shader_resource_view))? };

    Ok((texture, Some(shader_resource_view.unwrap())))
}

#[inline]
fn create_path_intermediate_msaa_texture_and_view(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(ID3D11Texture2D, Option<ID3D11RenderTargetView>)> {
    let msaa_texture = unsafe {
        let mut output = None;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: RENDER_TARGET_FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: PATH_MULTISAMPLE_COUNT,
                Quality: D3D11_STANDARD_MULTISAMPLE_PATTERN.0 as u32,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        device.CreateTexture2D(&desc, None, Some(&mut output))?;
        output.unwrap()
    };
    let mut msaa_view = None;
    unsafe { device.CreateRenderTargetView(&msaa_texture, None, Some(&mut msaa_view))? };
    Ok((msaa_texture, Some(msaa_view.unwrap())))
}

#[inline]
fn set_rasterizer_state(device: &ID3D11Device, device_context: &ID3D11DeviceContext) -> Result<()> {
    let desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        FrontCounterClockwise: false.into(),
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: true.into(),
        ScissorEnable: true.into(),
        MultisampleEnable: true.into(),
        AntialiasedLineEnable: false.into(),
    };
    let rasterizer_state = unsafe {
        let mut state = None;
        device.CreateRasterizerState(&desc, Some(&mut state))?;
        state.unwrap()
    };
    unsafe { device_context.RSSetState(&rasterizer_state) };
    Ok(())
}

// https://learn.microsoft.com/en-us/windows/win32/api/d3d11/ns-d3d11-d3d11_blend_desc
#[inline]
fn create_blend_state(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC_ALPHA;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

/// Layers arrive already multiplied by their own alpha, so their colour must not be scaled again.
#[inline]
fn create_blend_state_premultiplied(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

/// The first half of a backdrop's mix: the source contributes nothing of its own and the
/// destination keeps `1 - src.a` of itself, colour and alpha together, so the blurred copy
/// drawn after it lands in a hole rather than on top of what it replaces.
#[inline]
fn create_blend_state_punch(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ZERO;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ZERO;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

/// The second half: the blurred copy is added into the hole the punch left.
#[inline]
fn create_blend_state_additive(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

/// Blur and blit passes own every pixel they touch.
#[inline]
fn create_blend_state_opaque(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = false.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_ZERO;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ZERO;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

#[inline]
fn create_render_texture(device: &ID3D11Device, width: u32, height: u32) -> Result<RenderTexture> {
    let texture = unsafe {
        let mut output = None;
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width.max(1),
            Height: height.max(1),
            MipLevels: 1,
            ArraySize: 1,
            Format: RENDER_TARGET_FORMAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        device.CreateTexture2D(&desc, None, Some(&mut output))?;
        output.unwrap()
    };
    let mut view = None;
    let mut source = None;
    unsafe {
        device.CreateRenderTargetView(&texture, None, Some(&mut view))?;
        device.CreateShaderResourceView(&texture, None, Some(&mut source))?;
    }

    Ok(RenderTexture {
        _texture: texture,
        view,
        source,
    })
}

#[inline]
fn create_blend_state_for_subpixel_rendering(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_SRC1_COLOR;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC1_COLOR;
    // It does not make sense to draw transparent subpixel-rendered text, since it cannot be meaningfully alpha-blended onto anything else.
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_ZERO;
    desc.RenderTarget[0].RenderTargetWriteMask =
        D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8 & !D3D11_COLOR_WRITE_ENABLE_ALPHA.0 as u8;

    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

#[inline]
fn create_blend_state_for_path_rasterization(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    // If the feature level is set to greater than D3D_FEATURE_LEVEL_9_3, the display
    // device performs the blend in linear space, which is ideal.
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

#[inline]
fn create_blend_state_for_path_sprite(device: &ID3D11Device) -> Result<ID3D11BlendState> {
    // If the feature level is set to greater than D3D_FEATURE_LEVEL_9_3, the display
    // device performs the blend in linear space, which is ideal.
    let mut desc = D3D11_BLEND_DESC::default();
    desc.RenderTarget[0].BlendEnable = true.into();
    desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
    desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
    desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
    desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
    desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    unsafe {
        let mut state = None;
        device.CreateBlendState(&desc, Some(&mut state))?;
        Ok(state.unwrap())
    }
}

#[inline]
fn create_vertex_shader(device: &ID3D11Device, bytes: &[u8]) -> Result<ID3D11VertexShader> {
    unsafe {
        let mut shader = None;
        device.CreateVertexShader(bytes, None, Some(&mut shader))?;
        Ok(shader.unwrap())
    }
}

#[inline]
fn create_fragment_shader(device: &ID3D11Device, bytes: &[u8]) -> Result<ID3D11PixelShader> {
    unsafe {
        let mut shader = None;
        device.CreatePixelShader(bytes, None, Some(&mut shader))?;
        Ok(shader.unwrap())
    }
}

#[inline]
fn create_constant_buffer<T>(device: &ID3D11Device) -> Result<Option<ID3D11Buffer>> {
    const { assert!(std::mem::size_of::<T>() != 0 && std::mem::size_of::<T>().is_multiple_of(16)) };
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: std::mem::size_of::<T>() as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }?;
    Ok(buffer)
}

#[inline]
fn create_buffer(
    device: &ID3D11Device,
    element_size: usize,
    buffer_size: usize,
) -> Result<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: (element_size * buffer_size) as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
        StructureByteStride: element_size as u32,
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }?;
    Ok(buffer.unwrap())
}

/// A buffer shaders read as a `ByteAddressBuffer`, written from the CPU every frame.
fn create_raw_buffer(device: &ID3D11Device, size: usize) -> Result<ID3D11Buffer> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: size as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32,
        StructureByteStride: 0,
    };
    let mut buffer = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buffer)) }?;
    buffer.context("the device returned no buffer")
}

fn create_raw_buffer_view(
    device: &ID3D11Device,
    buffer: &ID3D11Buffer,
    size: usize,
) -> Result<Option<ID3D11ShaderResourceView>> {
    let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32_TYPELESS,
        ViewDimension: D3D11_SRV_DIMENSION_BUFFEREX,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            BufferEx: D3D11_BUFFEREX_SRV {
                FirstElement: 0,
                NumElements: (size / 4) as u32,
                Flags: D3D11_BUFFEREX_SRV_FLAG_RAW.0 as u32,
            },
        },
    };
    let mut view = None;
    unsafe { device.CreateShaderResourceView(buffer, Some(&desc), Some(&mut view)) }?;
    Ok(view)
}

#[inline]
fn create_buffer_view(
    device: &ID3D11Device,
    buffer: &ID3D11Buffer,
) -> Result<Option<ID3D11ShaderResourceView>> {
    let mut view = None;
    unsafe { device.CreateShaderResourceView(buffer, None, Some(&mut view)) }?;
    Ok(view)
}

#[inline]
fn update_buffer<T>(
    device_context: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    data: &[T],
) -> Result<()> {
    unsafe {
        let mut dest = std::mem::zeroed();
        device_context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut dest))?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), dest.pData as _, data.len());
        device_context.Unmap(buffer, 0);
    }
    Ok(())
}

#[inline]
fn update_batch_start(
    device_context: &ID3D11DeviceContext,
    buffer: &ID3D11Buffer,
    first_instance: u32,
) -> Result<()> {
    update_buffer(
        device_context,
        buffer,
        &[BatchParams {
            start_index: first_instance,
            _padding: [0; 3],
        }],
    )
}

#[inline]
fn set_pipeline_state(
    device_context: &ID3D11DeviceContext,
    buffer_view: &[Option<ID3D11ShaderResourceView>],
    topology: D3D_PRIMITIVE_TOPOLOGY,
    vertex_shader: &ID3D11VertexShader,
    fragment_shader: &ID3D11PixelShader,
    blend_state: &ID3D11BlendState,
) {
    unsafe {
        device_context.VSSetShaderResources(1, Some(buffer_view));
        device_context.PSSetShaderResources(1, Some(buffer_view));
        device_context.IASetPrimitiveTopology(topology);
        device_context.VSSetShader(vertex_shader, None);
        device_context.PSSetShader(fragment_shader, None);
        device_context.OMSetBlendState(blend_state, None, 0xFFFFFFFF);
    }
}

#[cfg(debug_assertions)]
fn report_live_objects(device: &ID3D11Device) -> Result<()> {
    let debug_device: ID3D11Debug = device.cast()?;
    unsafe {
        debug_device.ReportLiveDeviceObjects(D3D11_RLDO_DETAIL)?;
    }
    Ok(())
}

const BUFFER_COUNT: usize = 3;

pub(crate) mod shader_resources {
    use anyhow::Result;

    #[cfg(debug_assertions)]
    use windows::{
        Win32::Graphics::Direct3D::{
            Fxc::{D3DCOMPILE_DEBUG, D3DCOMPILE_SKIP_OPTIMIZATION, D3DCompileFromFile},
            ID3DBlob,
        },
        core::{HSTRING, PCSTR},
    };

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum ShaderModule {
        Backdrop,
        /// Fragment only. `vertex_source` sends it to `Backdrop` for a vertex stage.
        BackdropPunch,
        Gaussian,
        KawaseDown,
        /// Fragment only. `vertex_source` sends it to `KawaseDown` for a vertex stage.
        KawaseUp,
        Blit,
        Mask,
        Quad,
        Shadow,
        Underline,
        PathRasterization,
        PathSprite,
        MonochromeSprite,
        SubpixelSprite,
        /// Fragment only. `vertex_source` sends it to `SubpixelSprite` for a vertex stage.
        SubpixelSpriteLayered,
        PolychromeSprite,
        EmojiRasterization,
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    pub(crate) enum ShaderTarget {
        Vertex,
        Fragment,
    }

    pub(crate) struct RawShaderBytes<'t> {
        inner: &'t [u8],

        #[cfg(debug_assertions)]
        _blob: ID3DBlob,
    }

    impl ShaderModule {
        /// Where the vertex stage of this module comes from. A fragment-only module borrows
        /// another's, so asking it for a vertex shader answers with that one.
        fn vertex_source(self) -> ShaderModule {
            match self {
                ShaderModule::BackdropPunch => ShaderModule::Backdrop,
                ShaderModule::KawaseUp => ShaderModule::KawaseDown,
                ShaderModule::SubpixelSpriteLayered => ShaderModule::SubpixelSprite,
                module => module,
            }
        }
    }

    impl<'t> RawShaderBytes<'t> {
        pub(crate) fn new(module: ShaderModule, target: ShaderTarget) -> Result<Self> {
            let module = match target {
                ShaderTarget::Vertex => module.vertex_source(),
                ShaderTarget::Fragment => module,
            };
            #[cfg(not(debug_assertions))]
            {
                Ok(Self::from_bytes(module, target))
            }
            #[cfg(debug_assertions)]
            {
                let blob = build_shader_blob(module, target)?;
                let inner = unsafe {
                    std::slice::from_raw_parts(
                        blob.GetBufferPointer() as *const u8,
                        blob.GetBufferSize(),
                    )
                };
                Ok(Self { inner, _blob: blob })
            }
        }

        pub(crate) fn as_bytes(&'t self) -> &'t [u8] {
            self.inner
        }

        #[cfg(not(debug_assertions))]
        fn from_bytes(module: ShaderModule, target: ShaderTarget) -> Self {
            let bytes = match module {
                ShaderModule::Backdrop => match target {
                    ShaderTarget::Vertex => BACKDROP_VERTEX_BYTES,
                    ShaderTarget::Fragment => BACKDROP_FRAGMENT_BYTES,
                },
                ShaderModule::BackdropPunch => match target {
                    ShaderTarget::Vertex => BACKDROP_VERTEX_BYTES,
                    ShaderTarget::Fragment => BACKDROP_PUNCH_FRAGMENT_BYTES,
                },
                ShaderModule::Gaussian => match target {
                    ShaderTarget::Vertex => GAUSSIAN_VERTEX_BYTES,
                    ShaderTarget::Fragment => GAUSSIAN_FRAGMENT_BYTES,
                },
                ShaderModule::KawaseDown => match target {
                    ShaderTarget::Vertex => KAWASE_DOWN_VERTEX_BYTES,
                    ShaderTarget::Fragment => KAWASE_DOWN_FRAGMENT_BYTES,
                },
                ShaderModule::KawaseUp => match target {
                    ShaderTarget::Vertex => KAWASE_DOWN_VERTEX_BYTES,
                    ShaderTarget::Fragment => KAWASE_UP_FRAGMENT_BYTES,
                },
                ShaderModule::Blit => match target {
                    ShaderTarget::Vertex => BLIT_VERTEX_BYTES,
                    ShaderTarget::Fragment => BLIT_FRAGMENT_BYTES,
                },
                ShaderModule::Mask => match target {
                    ShaderTarget::Vertex => MASK_VERTEX_BYTES,
                    ShaderTarget::Fragment => MASK_FRAGMENT_BYTES,
                },
                ShaderModule::Quad => match target {
                    ShaderTarget::Vertex => QUAD_VERTEX_BYTES,
                    ShaderTarget::Fragment => QUAD_FRAGMENT_BYTES,
                },
                ShaderModule::Shadow => match target {
                    ShaderTarget::Vertex => SHADOW_VERTEX_BYTES,
                    ShaderTarget::Fragment => SHADOW_FRAGMENT_BYTES,
                },
                ShaderModule::Underline => match target {
                    ShaderTarget::Vertex => UNDERLINE_VERTEX_BYTES,
                    ShaderTarget::Fragment => UNDERLINE_FRAGMENT_BYTES,
                },
                ShaderModule::PathRasterization => match target {
                    ShaderTarget::Vertex => PATH_RASTERIZATION_VERTEX_BYTES,
                    ShaderTarget::Fragment => PATH_RASTERIZATION_FRAGMENT_BYTES,
                },
                ShaderModule::PathSprite => match target {
                    ShaderTarget::Vertex => PATH_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => PATH_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::MonochromeSprite => match target {
                    ShaderTarget::Vertex => MONOCHROME_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => MONOCHROME_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::SubpixelSprite => match target {
                    ShaderTarget::Vertex => SUBPIXEL_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => SUBPIXEL_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::SubpixelSpriteLayered => match target {
                    ShaderTarget::Vertex => SUBPIXEL_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => SUBPIXEL_SPRITE_LAYERED_FRAGMENT_BYTES,
                },
                ShaderModule::PolychromeSprite => match target {
                    ShaderTarget::Vertex => POLYCHROME_SPRITE_VERTEX_BYTES,
                    ShaderTarget::Fragment => POLYCHROME_SPRITE_FRAGMENT_BYTES,
                },
                ShaderModule::EmojiRasterization => match target {
                    ShaderTarget::Vertex => EMOJI_RASTERIZATION_VERTEX_BYTES,
                    ShaderTarget::Fragment => EMOJI_RASTERIZATION_FRAGMENT_BYTES,
                },
            };
            Self { inner: bytes }
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn build_shader_blob(entry: ShaderModule, target: ShaderTarget) -> Result<ID3DBlob> {
        unsafe {
            use windows::Win32::Graphics::{
                Direct3D::ID3DInclude, Hlsl::D3D_COMPILE_STANDARD_FILE_INCLUDE,
            };

            let shader_name = if matches!(entry, ShaderModule::EmojiRasterization) {
                "color_text_raster.hlsl"
            } else {
                "shaders.hlsl"
            };

            let entry = format!(
                "{}_{}\0",
                entry.as_str(),
                match target {
                    ShaderTarget::Vertex => "vertex",
                    ShaderTarget::Fragment => "fragment",
                }
            );
            let target = match target {
                ShaderTarget::Vertex => "vs_4_1\0",
                ShaderTarget::Fragment => "ps_4_1\0",
            };

            let mut compile_blob = None;
            let mut error_blob = None;
            let shader_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(&format!("src/{}", shader_name))
                .canonicalize()?;

            let entry_point = PCSTR::from_raw(entry.as_ptr());
            let target_cstr = PCSTR::from_raw(target.as_ptr());

            // really dirty trick because winapi bindings are unhappy otherwise
            let include_handler = &std::mem::transmute::<usize, ID3DInclude>(
                D3D_COMPILE_STANDARD_FILE_INCLUDE as usize,
            );

            let ret = D3DCompileFromFile(
                &HSTRING::from(shader_path.to_str().unwrap()),
                None,
                include_handler,
                entry_point,
                target_cstr,
                D3DCOMPILE_DEBUG | D3DCOMPILE_SKIP_OPTIMIZATION,
                0,
                &mut compile_blob,
                Some(&mut error_blob),
            );
            if ret.is_err() {
                let Some(error_blob) = error_blob else {
                    return Err(anyhow::anyhow!("{ret:?}"));
                };

                let error_string =
                    std::ffi::CStr::from_ptr(error_blob.GetBufferPointer() as *const i8)
                        .to_string_lossy();
                log::error!("Shader compile error: {}", error_string);
                return Err(anyhow::anyhow!("Compile error: {}", error_string));
            }
            Ok(compile_blob.unwrap())
        }
    }

    #[cfg(not(debug_assertions))]
    include!(concat!(env!("OUT_DIR"), "/shaders_bytes.rs"));

    #[cfg(debug_assertions)]
    impl ShaderModule {
        pub fn as_str(self) -> &'static str {
            match self {
                ShaderModule::Backdrop => "backdrop",
                ShaderModule::BackdropPunch => "backdrop_punch",
                ShaderModule::Gaussian => "gaussian",
                ShaderModule::KawaseDown => "kawase_down",
                ShaderModule::KawaseUp => "kawase_up",
                ShaderModule::Blit => "blit",
                ShaderModule::Mask => "mask",
                ShaderModule::Quad => "quad",
                ShaderModule::Shadow => "shadow",
                ShaderModule::Underline => "underline",
                ShaderModule::PathRasterization => "path_rasterization",
                ShaderModule::PathSprite => "path_sprite",
                ShaderModule::MonochromeSprite => "monochrome_sprite",
                ShaderModule::SubpixelSprite => "subpixel_sprite",
                ShaderModule::SubpixelSpriteLayered => "subpixel_sprite_layered",
                ShaderModule::PolychromeSprite => "polychrome_sprite",
                ShaderModule::EmojiRasterization => "emoji_rasterization",
            }
        }
    }
}

mod nvidia {
    use std::{
        ffi::CStr,
        os::raw::{c_char, c_int, c_uint},
    };

    use anyhow::Result;
    use windows::{Win32::System::LibraryLoader::GetProcAddress, core::s};

    use crate::with_dll_library;

    // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_lite_common.h#L180
    const NVAPI_SHORT_STRING_MAX: usize = 64;

    // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_lite_common.h#L235
    #[allow(non_camel_case_types)]
    type NvAPI_ShortString = [c_char; NVAPI_SHORT_STRING_MAX];

    // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_lite_common.h#L447
    #[allow(non_camel_case_types)]
    type NvAPI_SYS_GetDriverAndBranchVersion_t = unsafe extern "C" fn(
        driver_version: *mut c_uint,
        build_branch_string: *mut NvAPI_ShortString,
    ) -> c_int;

    pub(super) fn get_driver_version() -> Result<String> {
        #[cfg(target_pointer_width = "64")]
        let nvidia_dll_name = s!("nvapi64.dll");
        #[cfg(target_pointer_width = "32")]
        let nvidia_dll_name = s!("nvapi.dll");

        with_dll_library(nvidia_dll_name, |nvidia_dll| unsafe {
            let nvapi_query_addr = GetProcAddress(nvidia_dll, s!("nvapi_QueryInterface"))
                .ok_or_else(|| anyhow::anyhow!("Failed to get nvapi_QueryInterface address"))?;
            let nvapi_query: extern "C" fn(u32) -> *mut () = std::mem::transmute(nvapi_query_addr);

            // https://github.com/NVIDIA/nvapi/blob/7cb76fce2f52de818b3da497af646af1ec16ce27/nvapi_interface.h#L41
            let nvapi_get_driver_version_ptr = nvapi_query(0x2926aaad);
            if nvapi_get_driver_version_ptr.is_null() {
                anyhow::bail!("Failed to get NVIDIA driver version function pointer");
            }
            let nvapi_get_driver_version: NvAPI_SYS_GetDriverAndBranchVersion_t =
                std::mem::transmute(nvapi_get_driver_version_ptr);

            let mut driver_version: c_uint = 0;
            let mut build_branch_string: NvAPI_ShortString = [0; NVAPI_SHORT_STRING_MAX];
            let result = nvapi_get_driver_version(
                &mut driver_version as *mut c_uint,
                &mut build_branch_string as *mut NvAPI_ShortString,
            );

            if result != 0 {
                anyhow::bail!(
                    "Failed to get NVIDIA driver version, error code: {}",
                    result
                );
            }
            let major = driver_version / 100;
            let minor = driver_version % 100;
            let branch_string = CStr::from_ptr(build_branch_string.as_ptr());
            Ok(format!(
                "{}.{} {}",
                major,
                minor,
                branch_string.to_string_lossy()
            ))
        })
    }
}

mod amd {
    use std::os::raw::{c_char, c_int, c_void};

    use anyhow::Result;
    use windows::{Win32::System::LibraryLoader::GetProcAddress, core::s};

    use crate::with_dll_library;

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L145
    const AGS_CURRENT_VERSION: i32 = (6 << 22) | (3 << 12);

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L204
    // This is an opaque type, using struct to represent it properly for FFI
    #[repr(C)]
    struct AGSContext {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub struct AGSGPUInfo {
        pub driver_version: *const c_char,
        pub radeon_software_version: *const c_char,
        pub num_devices: c_int,
        pub devices: *mut c_void,
    }

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L429
    #[allow(non_camel_case_types)]
    type agsInitialize_t = unsafe extern "C" fn(
        version: c_int,
        config: *const c_void,
        context: *mut *mut AGSContext,
        gpu_info: *mut AGSGPUInfo,
    ) -> c_int;

    // https://github.com/GPUOpen-LibrariesAndSDKs/AGS_SDK/blob/5d8812d703d0335741b6f7ffc37838eeb8b967f7/ags_lib/inc/amd_ags.h#L436
    #[allow(non_camel_case_types)]
    type agsDeInitialize_t = unsafe extern "C" fn(context: *mut AGSContext) -> c_int;

    pub(super) fn get_driver_version() -> Result<String> {
        #[cfg(target_pointer_width = "64")]
        let amd_dll_name = s!("amd_ags_x64.dll");
        #[cfg(target_pointer_width = "32")]
        let amd_dll_name = s!("amd_ags_x86.dll");

        with_dll_library(amd_dll_name, |amd_dll| unsafe {
            let ags_initialize_addr = GetProcAddress(amd_dll, s!("agsInitialize"))
                .ok_or_else(|| anyhow::anyhow!("Failed to get agsInitialize address"))?;
            let ags_deinitialize_addr = GetProcAddress(amd_dll, s!("agsDeInitialize"))
                .ok_or_else(|| anyhow::anyhow!("Failed to get agsDeInitialize address"))?;

            let ags_initialize: agsInitialize_t = std::mem::transmute(ags_initialize_addr);
            let ags_deinitialize: agsDeInitialize_t = std::mem::transmute(ags_deinitialize_addr);

            let mut context: *mut AGSContext = std::ptr::null_mut();
            let mut gpu_info: AGSGPUInfo = AGSGPUInfo {
                driver_version: std::ptr::null(),
                radeon_software_version: std::ptr::null(),
                num_devices: 0,
                devices: std::ptr::null_mut(),
            };

            let result = ags_initialize(
                AGS_CURRENT_VERSION,
                std::ptr::null(),
                &mut context,
                &mut gpu_info,
            );
            if result != 0 {
                anyhow::bail!("Failed to initialize AMD AGS, error code: {}", result);
            }

            // Vulkan actually returns this as the driver version
            let software_version = if !gpu_info.radeon_software_version.is_null() {
                std::ffi::CStr::from_ptr(gpu_info.radeon_software_version)
                    .to_string_lossy()
                    .into_owned()
            } else {
                "Unknown Radeon Software Version".to_string()
            };

            let driver_version = if !gpu_info.driver_version.is_null() {
                std::ffi::CStr::from_ptr(gpu_info.driver_version)
                    .to_string_lossy()
                    .into_owned()
            } else {
                "Unknown Radeon Driver Version".to_string()
            };

            ags_deinitialize(context);
            Ok(format!("{} ({})", software_version, driver_version))
        })
    }
}

mod dxgi {
    use windows::{
        Win32::Graphics::Dxgi::{IDXGIAdapter1, IDXGIDevice},
        core::Interface,
    };

    pub(super) fn get_driver_version(adapter: &IDXGIAdapter1) -> anyhow::Result<String> {
        let number = unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID as _) }?;
        Ok(format!(
            "{}.{}.{}.{}",
            number >> 48,
            (number >> 32) & 0xFFFF,
            (number >> 16) & 0xFFFF,
            number & 0xFFFF
        ))
    }
}

/// A linear sampler that treats coordinates outside a texture with `address`.
fn create_sampler(
    device: &ID3D11Device,
    address: D3D11_TEXTURE_ADDRESS_MODE,
) -> Result<Option<ID3D11SamplerState>> {
    let desc = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: address,
        AddressV: address,
        AddressW: address,
        MipLODBias: 0.0,
        MaxAnisotropy: 1,
        ComparisonFunc: D3D11_COMPARISON_ALWAYS,
        BorderColor: [0.0; 4],
        MinLOD: 0.0,
        MaxLOD: D3D11_FLOAT32_MAX,
    };
    let mut output = None;
    unsafe { device.CreateSamplerState(&desc, Some(&mut output))? };
    Ok(output)
}

/// The size in texels of a target `shrink` times smaller than the frame. Blur levels are made at
/// exactly this size, so every pass draws over the whole texture and no row or column is left
/// unwritten at the frame's edge.
fn level_size(viewport: &D3D11_VIEWPORT, shrink: u32) -> (u32, u32) {
    (
        viewport.Width as u32 / shrink,
        viewport.Height as u32 / shrink,
    )
}

#[cfg(test)]
#[path = "../../gpui/src/backdrop_test_fixture.rs"]
mod backdrop_fixture;

#[cfg(test)]
#[::core::prelude::v1::test]
fn nested_backdrops_sample_ancestor_layers() -> anyhow::Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows::core::w;
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("backdrop regression"),
            WS_POPUP,
            0,
            0,
            64,
            64,
            None,
            None,
            None,
            None,
        )?
    };
    let result = (|| {
        let devices = DirectXDevices::new()?;
        let mut renderer = DirectXRenderer::new(hwnd, &devices, true)?;
        renderer.resize(size(DevicePixels(64), DevicePixels(64)))?;
        for blur in [3., 12.] {
            for depth in [0, 1, 2] {
                for inside in [false, true] {
                    let scene = backdrop_fixture::scene(depth, blur, inside);
                    let image =
                        renderer.render_to_image(&scene, WindowBackgroundAppearance::Opaque)?;
                    backdrop_fixture::check(&image);
                }
            }
        }
        Ok(())
    })();
    unsafe {
        DestroyWindow(hwnd)?;
    }
    result
}
