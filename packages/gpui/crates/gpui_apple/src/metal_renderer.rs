use crate::metal_atlas::MetalAtlas;
use anyhow::{Context as _, Result};
use block2::RcBlock;
use collections::FxHashMap;
use core_graphics::geometry::CGSize;
use gpui::{
    AtlasTextureId, Background, BlurPasses, Bounds, ContentMask, DevicePixels, GAUSSIAN_REACH,
    KAWASE_LEVELS, Kawase, LayerEffect, PaintSurface, Path, Point, PrimitiveBatch, ScaledPixels,
    Scene, ShaderId, Size, point, size,
};
#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
use image::RgbaImage;
use objc2::runtime::AnyObject;

use core_graphics::color_space::{CGColorSpace, kCGColorSpaceDisplayP3};
use foreign_types::{ForeignType, ForeignTypeRef};
use metal::{
    CAMetalLayer, CommandQueue, MTLGPUFamily, MTLPixelFormat, MTLResourceOptions, NSRange,
    NSUInteger,
};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBufferGetHeight,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferGetWidthOfPlane, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, kCVReturnSuccess,
};
use parking_lot::Mutex;

use std::{
    cell::Cell,
    ffi::c_void,
    mem,
    mem::MaybeUninit,
    ops::Range,
    ptr, slice,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

// Exported to metal
pub(crate) type PointF = gpui::Point<f32>;

#[cfg(not(feature = "runtime_shaders"))]
const SHADERS_METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shaders.metallib"));
#[cfg(feature = "runtime_shaders")]
const SHADERS_SOURCE_FILE: &str = include_str!(concat!(env!("OUT_DIR"), "/stitched_shaders.metal"));
// Use 4x MSAA, all devices support it.
// https://developer.apple.com/documentation/metal/mtldevice/1433355-supportstexturesamplecount
const PATH_SAMPLE_COUNT: u32 = 4;
/// Metal requires the offset a buffer is bound at to be 256-byte aligned.
const INSTANCE_BUFFER_ALIGNMENT: usize = 256;
const MAX_INSTANCE_BUFFER_SIZE: usize = 256 * 1024 * 1024;
const INITIAL_INSTANCE_BUFFER_SIZE: usize = 2 * 1024 * 1024;
/// How long the instance buffer pool watches its peak use before deciding whether to shrink.
const INSTANCE_BUFFER_SETTLE: Duration = Duration::from_secs(5);
/// How long an offscreen target may go unused before its memory is given back. A frame with a
/// blur, layer or path on screen touches its targets every time, so only targets that nothing
/// on screen draws through are released.
const IDLE_RELEASE: Duration = Duration::from_secs(2);
/// How deeply filtered layers can nest before the innermost ones stop being filtered.
const LAYER_DEPTH: usize = 4;
/// How long a frame may stay on the GPU before the window draws again without waiting for it, so a
/// completion that never arrives cannot freeze the window.
const FRAME_STALL: Duration = Duration::from_millis(250);

pub type Context = Arc<Mutex<InstanceBufferPool>>;
pub type Renderer = MetalRenderer;

pub unsafe fn new_renderer(
    context: self::Context,
    _native_window: *mut c_void,
    _native_view: *mut c_void,
    _bounds: gpui::Size<f32>,
    transparent: bool,
    use_display_p3: bool,
) -> Renderer {
    MetalRenderer::new(context, transparent, use_display_p3)
}

pub struct InstanceBufferPool {
    buffer_size: usize,
    buffers: Vec<metal::Buffer>,
    /// The most any frame has written since `window` began.
    peak: usize,
    window: Instant,
}

impl Default for InstanceBufferPool {
    fn default() -> Self {
        Self {
            buffer_size: INITIAL_INSTANCE_BUFFER_SIZE,
            buffers: Vec::new(),
            peak: 0,
            window: Instant::now(),
        }
    }
}

pub(crate) struct InstanceBuffer {
    metal_buffer: metal::Buffer,
    size: usize,
}

impl InstanceBufferPool {
    pub(crate) fn reset(&mut self, buffer_size: usize) {
        self.buffer_size = buffer_size;
        self.buffers.clear();
    }

    pub(crate) fn acquire(
        &mut self,
        device: &metal::Device,
        unified_memory: bool,
    ) -> InstanceBuffer {
        let buffer = self.buffers.pop().unwrap_or_else(|| {
            let options = if unified_memory {
                MTLResourceOptions::StorageModeShared
                    // Buffers are write only which can benefit from the combined cache
                    // https://developer.apple.com/documentation/metal/mtlresourceoptions/cpucachemodewritecombined
                    | MTLResourceOptions::CPUCacheModeWriteCombined
            } else {
                MTLResourceOptions::StorageModeManaged
            };

            device.new_buffer(self.buffer_size as u64, options)
        });
        InstanceBuffer {
            metal_buffer: buffer,
            size: self.buffer_size,
        }
    }

    pub(crate) fn release(&mut self, buffer: InstanceBuffer) {
        if buffer.size == self.buffer_size {
            self.buffers.push(buffer.metal_buffer)
        }
    }

    /// Shrinks the buffers once every frame over the last `INSTANCE_BUFFER_SETTLE` has needed
    /// under a quarter of one. `used` is what the frame just encoded wrote, and the new size keeps
    /// twice the peak so the next busy frame does not grow it straight back. Buffers still in
    /// flight are dropped when they come back, since their size no longer matches.
    pub(crate) fn settle(&mut self, used: usize) {
        self.peak = self.peak.max(used);
        let now = Instant::now();
        if now.duration_since(self.window) < INSTANCE_BUFFER_SETTLE {
            return;
        }
        let peak = mem::take(&mut self.peak);
        self.window = now;

        let buffer_size = (peak * 2)
            .next_power_of_two()
            .clamp(INITIAL_INSTANCE_BUFFER_SIZE, MAX_INSTANCE_BUFFER_SIZE);
        if peak * 4 > self.buffer_size || buffer_size >= self.buffer_size {
            return;
        }
        log::info!("decreased instance buffer size to {buffer_size}");
        self.reset(buffer_size);
    }
}

pub struct MetalRenderer {
    device: metal::Device,
    layer: Option<metal::MetalLayer>,
    is_apple_gpu: bool,
    is_unified_memory: bool,
    presents_with_transaction: bool,
    /// For headless rendering, tracks whether output should be opaque
    opaque: bool,
    command_queue: CommandQueue,
    paths_rasterization_pipeline_state: metal::RenderPipelineState,
    path_sprites_pipeline_state: metal::RenderPipelineState,
    shadows_pipeline_state: metal::RenderPipelineState,
    quads_pipeline_state: metal::RenderPipelineState,
    underlines_pipeline_state: metal::RenderPipelineState,
    monochrome_sprites_pipeline_state: metal::RenderPipelineState,
    polychrome_sprites_pipeline_state: metal::RenderPipelineState,
    surfaces_pipeline_state: metal::RenderPipelineState,
    backdrops_pipeline_state: metal::RenderPipelineState,
    backdrop_punch_pipeline_state: metal::RenderPipelineState,
    gaussian_pipeline_state: metal::RenderPipelineState,
    kawase_down_pipeline_state: metal::RenderPipelineState,
    kawase_up_pipeline_state: metal::RenderPipelineState,
    blit_pipeline_state: metal::RenderPipelineState,
    /// Writes transparent black under a scissor, for wiping the part of a layer target in use.
    clear_pipeline_state: metal::RenderPipelineState,
    composite_pipeline_state: metal::RenderPipelineState,
    masked_pipeline_state: metal::RenderPipelineState,
    /// A pipeline per custom shader, built from its MSL translation the first time a frame draws
    /// it. A shader that failed to build is kept as `None`, so it is not built again every frame.
    custom_pipeline_states: FxHashMap<ShaderId, Option<metal::RenderPipelineState>>,
    unit_vertices: metal::Buffer,
    #[allow(clippy::arc_with_non_send_sync)]
    instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    sprite_atlas: Arc<MetalAtlas>,
    core_video_texture_cache: CFRetained<CVMetalTextureCache>,
    path_targets: Option<PathTargets>,
    path_sample_count: u32,
    filter_targets: FilterTargets,
    /// Frames `draw` has committed that the GPU has not finished yet, and what wakes the window
    /// once they are done.
    frame_pacing: Arc<FramePacing>,
    /// When `draw` last committed a frame.
    committed_at: Instant,
    /// When the frame being encoded started, which stamps every offscreen target it touches.
    frame_time: Instant,
    /// Offscreen render target reused across `render_scene` calls when
    /// rendering headlessly without reading pixels back.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    headless_render_target: Option<metal::Texture>,
}

/// Frames on the GPU and the window's request for one more. The window skips a display tick
/// while a frame is still running, and the frame that finishes asks for the one it held back.
#[derive(Default)]
struct FramePacing {
    in_flight: AtomicUsize,
    waiting: AtomicBool,
    wake: Mutex<Option<FrameWaker>>,
}

/// Asks the window for a frame from a Metal completion thread.
type FrameWaker = Box<dyn Fn() + Send + Sync>;

impl FramePacing {
    /// Counts a frame off the GPU and asks for the frame a skipped display tick held back.
    fn frame_done(&self) {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        if self.waiting.swap(false, Ordering::SeqCst)
            && let Some(wake) = self.wake.lock().as_ref()
        {
            wake();
        }
    }
}

#[repr(C)]
pub struct PathRasterizationVertex {
    pub xy_position: Point<ScaledPixels>,
    pub st_position: Point<f32>,
    pub color: Background,
    pub bounds: Bounds<ScaledPixels>,
}

impl MetalRenderer {
    /// Creates a new MetalRenderer with a CAMetalLayer for window-based rendering.
    pub fn new(
        instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
        transparent: bool,
        use_display_p3: bool,
    ) -> Self {
        let device = Self::create_device();
        let layer = metal::MetalLayer::new();
        Self::configure_layer(&layer, &device, transparent, use_display_p3);
        Self::new_internal(device, Some(layer), !transparent, instance_buffer_pool)
    }

    /// Creates a renderer for a CAMetalLayer owned by a platform view, such as
    /// the backing layer UIKit creates for a view whose `layerClass` is
    /// `CAMetalLayer`. The renderer retains the layer.
    pub fn from_layer(
        instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
        layer: &objc2_quartz_core::CAMetalLayer,
        transparent: bool,
    ) -> Self {
        let device = Self::create_device();
        // Both types bind the same Objective-C class, so this only changes which
        // Rust wrapper views the live layer. `to_owned` retains it.
        let layer = unsafe {
            metal::MetalLayerRef::from_ptr(ptr::from_ref(layer).cast_mut().cast::<CAMetalLayer>())
        }
        .to_owned();
        Self::configure_layer(&layer, &device, transparent, false);
        Self::new_internal(device, Some(layer), !transparent, instance_buffer_pool)
    }

    fn configure_layer(
        layer: &metal::MetalLayerRef,
        device: &metal::DeviceRef,
        transparent: bool,
        use_display_p3: bool,
    ) {
        layer.set_device(device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        // metal-rs doesn't bind these setters, so view the same object through
        // objc2's typed CAMetalLayer binding.
        let objc2_layer: &objc2_quartz_core::CAMetalLayer = unsafe { &*layer.as_ptr().cast() };
        if use_display_p3 {
            // SAFETY: The constant is extern, from Core Graphics
            let display_p3_name = unsafe { kCGColorSpaceDisplayP3 };
            if let Some(display_p3_color_space) = CGColorSpace::create_with_name(display_p3_name) {
                // SAFETY: `layer` is a CAMetalLayer and the color space remains valid for the call.
                // 'metal-rs' does not expose CAMetalLayer's typed `colorspace` setter.
                // Could be replaced with a native binding via objc2-* in future
                unsafe {
                    let _: () = objc2::msg_send![
                        objc2_layer,
                        setColorspace: display_p3_color_space.as_ptr().cast::<c_void>()
                    ];
                }
            } else {
                log::error!("unable to create the Display P3 color space");
            }
        }
        // Support direct-to-display rendering if the window is not transparent
        // https://developer.apple.com/documentation/metal/managing-your-game-window-for-metal-in-macos
        layer.set_opaque(!transparent);
        layer.set_maximum_drawable_count(3);
        // Allow texture reading for visual tests and UI automation screenshots
        // (captures without ScreenCaptureKit). Only debug builds pay the
        // presentation cost, even when `test-support` is compiled in.
        #[cfg(all(feature = "test-support", debug_assertions))]
        layer.set_framebuffer_only(false);
        objc2_layer.setAllowsNextDrawableTimeout(false);
        objc2_layer.setNeedsDisplayOnBoundsChange(true);
        // UIKit sizes a view's backing layer itself; only AppKit-hosted
        // layers need to track their superlayer's bounds.
        #[cfg(target_os = "macos")]
        objc2_layer.setAutoresizingMask(
            objc2_quartz_core::CAAutoresizingMask::LayerWidthSizable
                | objc2_quartz_core::CAAutoresizingMask::LayerHeightSizable,
        );
    }

    /// Creates a new headless MetalRenderer for offscreen rendering without a window.
    ///
    /// This renderer can render scenes to images without requiring a CAMetalLayer,
    /// window, or AppKit. Use `render_scene_to_image()` to render scenes.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn new_headless(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>) -> Self {
        let device = Self::create_device();
        Self::new_internal(device, None, true, instance_buffer_pool)
    }

    #[cfg(target_os = "macos")]
    fn create_device() -> metal::Device {
        // Prefer low‐power integrated GPUs on Intel Mac. On Apple
        // Silicon, there is only ever one GPU, so this is equivalent to
        // `metal::Device::system_default()`.
        if let Some(d) = metal::Device::all()
            .into_iter()
            .min_by_key(|d| (d.is_removable(), !d.is_low_power()))
        {
            d
        } else {
            // For some reason `all()` can return an empty list, see https://github.com/zed-industries/zed/issues/37689
            // In that case, we fall back to the system default device.
            log::error!(
                "Unable to enumerate Metal devices; attempting to use system default device"
            );
            Self::system_default_device()
        }
    }

    #[cfg(target_os = "ios")]
    fn create_device() -> metal::Device {
        Self::system_default_device()
    }

    fn system_default_device() -> metal::Device {
        metal::Device::system_default().unwrap_or_else(|| {
            log::error!("unable to access a compatible graphics device");
            std::process::exit(1);
        })
    }

    fn new_internal(
        device: metal::Device,
        layer: Option<metal::MetalLayer>,
        opaque: bool,
        instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    ) -> Self {
        #[cfg(feature = "runtime_shaders")]
        let library = device
            .new_library_with_source(&SHADERS_SOURCE_FILE, &metal::CompileOptions::new())
            .expect("error building metal library");
        #[cfg(not(feature = "runtime_shaders"))]
        let library = device
            .new_library_with_data(SHADERS_METALLIB)
            .expect("error building metal library");

        fn to_float2_bits(point: PointF) -> u64 {
            let mut output = point.y.to_bits() as u64;
            output <<= 32;
            output |= point.x.to_bits() as u64;
            output
        }

        // Shared memory can be used only if CPU and GPU share the same memory space.
        // https://developer.apple.com/documentation/metal/setting-resource-storage-modes
        // iOS does not support managed resources. Its simulator may report a
        // non-unified host GPU even though resources must still use shared storage.
        let is_unified_memory = cfg!(target_os = "ios") || device.has_unified_memory();
        // Apple GPU families support memoryless textures, which can significantly reduce
        // memory usage by keeping render targets in on-chip tile memory instead of
        // allocating backing store in system memory.
        // https://developer.apple.com/documentation/metal/mtlgpufamily
        let is_apple_gpu = device.supports_family(MTLGPUFamily::Apple1);

        let unit_vertices = [
            to_float2_bits(point(0., 0.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(1., 1.)),
        ];
        let unit_vertices = device.new_buffer_with_data(
            unit_vertices.as_ptr() as *const c_void,
            mem::size_of_val(&unit_vertices) as u64,
            if is_unified_memory {
                MTLResourceOptions::StorageModeShared
                    | MTLResourceOptions::CPUCacheModeWriteCombined
            } else {
                MTLResourceOptions::StorageModeManaged
            },
        );

        let paths_rasterization_pipeline_state = build_path_rasterization_pipeline_state(
            &device,
            &library,
            "paths_rasterization",
            "path_rasterization_vertex",
            "path_rasterization_fragment",
            MTLPixelFormat::BGRA8Unorm,
            PATH_SAMPLE_COUNT,
        );
        let path_sprites_pipeline_state = build_path_sprite_pipeline_state(
            &device,
            &library,
            "path_sprites",
            "path_sprite_vertex",
            "path_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let shadows_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "shadows",
            "shadow_vertex",
            "shadow_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let quads_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "quads",
            "quad_vertex",
            "quad_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let underlines_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "underlines",
            "underline_vertex",
            "underline_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let monochrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "monochrome_sprites",
            "monochrome_sprite_vertex",
            "monochrome_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let polychrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "polychrome_sprites",
            "polychrome_sprite_vertex",
            "polychrome_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let surfaces_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "surfaces",
            "surface_vertex",
            "surface_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );

        // A backdrop mixes the blurred copy into what it covers rather than laying it over:
        // the punch scales the destination by `1 - k`, the draw after it adds `blurred * k`.
        // Laying it over cost a see-through window its transparency.
        let backdrops_pipeline_state = build_filter_pipeline_state(
            &device,
            &library,
            "backdrops",
            "backdrop_vertex",
            "backdrop_fragment",
            MTLPixelFormat::BGRA8Unorm,
            FilterBlend::Add,
        );
        let backdrop_punch_pipeline_state = build_filter_pipeline_state(
            &device,
            &library,
            "backdrop_punch",
            "backdrop_vertex",
            "backdrop_punch_fragment",
            MTLPixelFormat::BGRA8Unorm,
            FilterBlend::Punch,
        );
        let gaussian_pipeline_state = build_filter_pipeline_state(
            &device,
            &library,
            "gaussian",
            "filter_vertex",
            "gaussian_fragment",
            MTLPixelFormat::BGRA8Unorm,
            FilterBlend::Replace,
        );
        let kawase_down_pipeline_state = build_filter_pipeline_state(
            &device,
            &library,
            "kawase_down",
            "filter_vertex",
            "kawase_down_fragment",
            MTLPixelFormat::BGRA8Unorm,
            FilterBlend::Replace,
        );
        let kawase_up_pipeline_state = build_filter_pipeline_state(
            &device,
            &library,
            "kawase_up",
            "filter_vertex",
            "kawase_up_fragment",
            MTLPixelFormat::BGRA8Unorm,
            FilterBlend::Replace,
        );
        let blit_pipeline_state = build_filter_pipeline_state(
            &device,
            &library,
            "blit",
            "filter_vertex",
            "blit_fragment",
            MTLPixelFormat::BGRA8Unorm,
            FilterBlend::Replace,
        );
        let clear_pipeline_state = build_filter_pipeline_state(
            &device,
            &library,
            "clear",
            "filter_vertex",
            "clear_fragment",
            MTLPixelFormat::BGRA8Unorm,
            FilterBlend::Replace,
        );
        let composite_pipeline_state = build_filter_pipeline_state(
            &device,
            &library,
            "composite",
            "filter_vertex",
            "mask_fragment",
            MTLPixelFormat::BGRA8Unorm,
            FilterBlend::Over,
        );
        let masked_pipeline_state = build_filter_pipeline_state(
            &device,
            &library,
            "masked",
            "filter_vertex",
            "mask_fragment",
            MTLPixelFormat::BGRA8Unorm,
            FilterBlend::Over,
        );

        let command_queue = device.new_command_queue();
        let supports_shared_storage = cfg!(target_os = "ios") || is_apple_gpu;
        let sprite_atlas = Arc::new(MetalAtlas::new(device.clone(), supports_shared_storage));
        // metal and objc2-metal expose the same Objective-C device; the borrowed
        // protocol object remains valid while `device` owns it.
        let core_video_texture_cache = unsafe {
            let mut cache = ptr::null_mut();
            let status = CVMetalTextureCache::create(
                None,
                None,
                &*device
                    .as_ptr()
                    .cast::<objc2::runtime::ProtocolObject<dyn objc2_metal::MTLDevice>>(),
                None,
                ptr::NonNull::from(&mut cache),
            );
            assert_eq!(
                status, kCVReturnSuccess,
                "failed to create CoreVideo Metal texture cache"
            );
            CFRetained::from_raw(ptr::NonNull::new(cache).expect("CoreVideo returned a null cache"))
        };

        Self {
            device,
            layer,
            presents_with_transaction: false,
            is_apple_gpu,
            is_unified_memory,
            opaque,
            command_queue,
            paths_rasterization_pipeline_state,
            path_sprites_pipeline_state,
            shadows_pipeline_state,
            quads_pipeline_state,
            underlines_pipeline_state,
            monochrome_sprites_pipeline_state,
            polychrome_sprites_pipeline_state,
            surfaces_pipeline_state,
            backdrops_pipeline_state,
            backdrop_punch_pipeline_state,
            gaussian_pipeline_state,
            kawase_down_pipeline_state,
            kawase_up_pipeline_state,
            blit_pipeline_state,
            clear_pipeline_state,
            composite_pipeline_state,
            masked_pipeline_state,
            custom_pipeline_states: FxHashMap::default(),
            unit_vertices,
            instance_buffer_pool,
            sprite_atlas,
            core_video_texture_cache,
            path_targets: None,
            path_sample_count: PATH_SAMPLE_COUNT,
            filter_targets: FilterTargets::default(),
            frame_pacing: Arc::default(),
            committed_at: Instant::now(),
            frame_time: Instant::now(),
            #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
            headless_render_target: None,
        }
    }

    pub fn layer(&self) -> Option<&metal::MetalLayerRef> {
        self.layer.as_ref().map(|l| l.as_ref())
    }

    pub fn layer_ptr(&self) -> *mut CAMetalLayer {
        self.layer
            .as_ref()
            .map(|l| l.as_ptr())
            .unwrap_or(ptr::null_mut())
    }

    pub fn sprite_atlas(&self) -> &Arc<MetalAtlas> {
        &self.sprite_atlas
    }

    pub fn set_presents_with_transaction(&mut self, presents_with_transaction: bool) {
        self.presents_with_transaction = presents_with_transaction;
        if let Some(layer) = &self.layer {
            layer.set_presents_with_transaction(presents_with_transaction);
        }
    }

    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        if let Some(layer) = &self.layer {
            layer.set_drawable_size(CGSize::new(size.width.0 as f64, size.height.0 as f64));
        }
        // Targets made for the old size are freed now rather than held until the next frame
        // that draws a path or a filter rebuilds them.
        if self
            .path_targets
            .as_ref()
            .is_some_and(|targets| targets.size != size)
        {
            self.path_targets = None;
        }
        if self.filter_targets.size != size {
            self.filter_targets = FilterTargets::default();
        }
    }

    /// The textures paths are rasterized through at `size`, created if no frame has drawn a
    /// path at that size since they were last released. None for an empty drawable, which
    /// happens before the first layout and cannot back a texture.
    fn path_targets(
        &mut self,
        size: Size<DevicePixels>,
    ) -> Option<(metal::Texture, Option<metal::Texture>)> {
        // We are uncertain when this happens, but sometimes size can be 0 here. Most likely before
        // the layout pass on window creation. Zero-sized texture creation causes SIGABRT.
        // https://github.com/zed-industries/zed/issues/36229
        if size.width.0 <= 0 || size.height.0 <= 0 {
            return None;
        }

        if self
            .path_targets
            .as_ref()
            .is_none_or(|targets| targets.size != size)
        {
            let texture_descriptor = metal::TextureDescriptor::new();
            texture_descriptor.set_width(size.width.0 as u64);
            texture_descriptor.set_height(size.height.0 as u64);
            texture_descriptor.set_pixel_format(metal::MTLPixelFormat::BGRA8Unorm);
            texture_descriptor.set_storage_mode(metal::MTLStorageMode::Private);
            texture_descriptor.set_usage(
                metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
            );
            let intermediate = self.device.new_texture(&texture_descriptor);

            let msaa = (self.path_sample_count > 1).then(|| {
                // https://developer.apple.com/documentation/metal/choosing-a-resource-storage-mode-for-apple-gpus
                // Rendering MSAA textures are done in a single pass, so we can use memory-less storage on Apple Silicon
                let storage_mode = if self.is_apple_gpu {
                    metal::MTLStorageMode::Memoryless
                } else {
                    metal::MTLStorageMode::Private
                };

                let msaa_descriptor = texture_descriptor;
                msaa_descriptor.set_texture_type(metal::MTLTextureType::D2Multisample);
                msaa_descriptor.set_storage_mode(storage_mode);
                msaa_descriptor.set_sample_count(self.path_sample_count as _);
                self.device.new_texture(&msaa_descriptor)
            });

            self.path_targets = Some(PathTargets {
                size,
                intermediate,
                msaa,
                used: self.frame_time,
            });
        }

        let targets = self.path_targets.as_mut()?;
        targets.used = self.frame_time;
        Some((targets.intermediate.clone(), targets.msaa.clone()))
    }

    /// One filter target for a drawable of `viewport_size`, created if no frame has drawn
    /// through it at that size since it was last released.
    fn filter_texture(
        &mut self,
        target: FilterTarget,
        viewport_size: Size<DevicePixels>,
    ) -> metal::Texture {
        if self.filter_targets.size != viewport_size {
            self.filter_targets = FilterTargets {
                size: viewport_size,
                ..FilterTargets::default()
            };
        }
        let shrink = target.shrink() as u64;
        let width = (i32::from(viewport_size.width).max(1) as u64 / shrink).max(1);
        let height = (i32::from(viewport_size.height).max(1) as u64 / shrink).max(1);
        let now = self.frame_time;
        let device = &self.device;
        let scratch = self.filter_targets.slot(target).get_or_insert_with(|| {
            let descriptor = metal::TextureDescriptor::new();
            descriptor.set_texture_type(metal::MTLTextureType::D2);
            descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
            descriptor.set_width(width);
            descriptor.set_height(height);
            descriptor.set_storage_mode(metal::MTLStorageMode::Private);
            descriptor.set_usage(
                metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
            );
            Scratch {
                texture: device.new_texture(&descriptor),
                used: now,
            }
        });
        scratch.used = now;
        scratch.texture.clone()
    }

    /// Frees offscreen targets that no frame has drawn through for `IDLE_RELEASE`. Every frame
    /// checks this itself, so a caller only needs it for a window that has stopped drawing.
    pub fn release_idle_targets(&mut self) {
        self.release_targets_idle_at(Instant::now());
    }

    /// Drops every offscreen target that no frame has drawn through within `IDLE_RELEASE` of
    /// `now`. A command buffer retains what it encodes, so a target an earlier frame still uses
    /// on the GPU stays alive until that frame completes.
    fn release_targets_idle_at(&mut self, now: Instant) {
        let idle = |used: Instant| now.duration_since(used) >= IDLE_RELEASE;
        if self
            .path_targets
            .as_ref()
            .is_some_and(|targets| idle(targets.used))
        {
            self.path_targets = None;
        }
        for slot in self.filter_targets.slots() {
            if slot.as_ref().is_some_and(|scratch| idle(scratch.used)) {
                *slot = None;
            }
        }
    }

    pub fn update_transparency(&mut self, transparent: bool) {
        self.opaque = !transparent;
        if let Some(layer) = &self.layer {
            layer.set_opaque(!transparent);
        }
    }

    pub fn destroy(&self) {
        // nothing to do
    }

    pub fn draw(&mut self, scene: &Scene) {
        let layer = match &self.layer {
            Some(l) => l.clone(),
            None => {
                log::error!(
                    "draw() called on headless renderer - use render_scene_to_image() instead"
                );
                return;
            }
        };
        let viewport_size = layer.drawable_size();
        let viewport_size: Size<DevicePixels> = size(
            (viewport_size.width.ceil() as i32).into(),
            (viewport_size.height.ceil() as i32).into(),
        );
        let drawable = if let Some(drawable) = layer.next_drawable() {
            drawable
        } else {
            log::error!(
                "failed to retrieve next drawable, drawable size: {:?}",
                viewport_size
            );
            return;
        };

        let command_buffer = match self.render_frame(scene, drawable.texture(), viewport_size) {
            Ok(command_buffer) => command_buffer,
            Err(error) => {
                log::error!("failed to render: {error:#}");
                return;
            }
        };

        self.frame_pacing.in_flight.fetch_add(1, Ordering::SeqCst);
        let pacing = self.frame_pacing.clone();
        let block = RcBlock::new(move |_: ptr::NonNull<AnyObject>| pacing.frame_done());
        // SAFETY: Both pointee types are opaque views of the same Objective-C block pointer ABI.
        unsafe {
            command_buffer.add_completed_handler(&*RcBlock::as_ptr(&block).cast());
        }
        self.committed_at = Instant::now();

        if self.presents_with_transaction {
            command_buffer.commit();
            command_buffer.wait_until_scheduled();
            drawable.present();
        } else {
            command_buffer.present_drawable(drawable);
            command_buffer.commit();
        }
    }

    /// Whether the GPU is still running the last frame `draw` committed. Drawing another one now
    /// would queue it behind that frame and block in `next_drawable` once the drawables run out,
    /// which holds input back by every frame in the queue. A `true` arms the waker set with
    /// `set_frame_waker`, so the window draws as soon as the GPU is done rather than a whole
    /// display tick later.
    pub fn gpu_behind(&self) -> bool {
        if self.committed_at.elapsed() >= FRAME_STALL {
            return false;
        }
        let pacing = &self.frame_pacing;
        pacing.waiting.store(true, Ordering::SeqCst);
        if pacing.in_flight.load(Ordering::SeqCst) == 0 {
            pacing.waiting.store(false, Ordering::SeqCst);
            return false;
        }
        true
    }

    /// Sets what asks the window for a frame once a frame `gpu_behind` held back has finished on
    /// the GPU. It runs on a Metal completion thread.
    pub fn set_frame_waker(&self, wake: impl Fn() + Send + Sync + 'static) {
        *self.frame_pacing.wake.lock() = Some(Box::new(wake));
    }

    fn render_frame(
        &mut self,
        scene: &Scene,
        texture: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
    ) -> Result<metal::CommandBuffer> {
        let mut writer = InstanceBufferWriter::new(
            &self.device,
            &self.instance_buffer_pool,
            self.is_unified_memory,
        );
        let instance_bindings = write_instances(scene, &mut writer).with_context(|| {
            format!(
                "scene too large: {} paths, {} shadows, {} quads, {} underlines, {} mono, {} poly, {} surfaces",
                scene.paths.len(),
                scene.shadows.len(),
                scene.quads.len(),
                scene.underlines.len(),
                scene.monochrome_sprites.len(),
                scene.polychrome_sprites.len(),
                scene.surfaces.len(),
            )
        })?;
        let command_buffer = self.draw_primitives_to_texture(
            scene,
            &instance_bindings,
            &mut writer,
            texture,
            viewport_size,
        )?;

        let instance_buffer_pool = self.instance_buffer_pool.clone();
        let used = writer.offset;
        let instance_buffer = Cell::new(Some(writer.finish()));
        instance_buffer_pool.lock().settle(used);
        let block = RcBlock::new(move |_: ptr::NonNull<AnyObject>| {
            if let Some(instance_buffer) = instance_buffer.take() {
                instance_buffer_pool.lock().release(instance_buffer);
            }
        });
        // SAFETY: Both pointee types are opaque views of the same Objective-C block pointer ABI.
        unsafe {
            command_buffer.add_completed_handler(&*RcBlock::as_ptr(&block).cast());
        }

        Ok(command_buffer)
    }

    /// Renders the scene to a texture and returns the pixel data as an RGBA image.
    /// This does not present the frame to screen - useful for visual testing
    /// where we want to capture what would be rendered without displaying it.
    ///
    /// Note: This requires a layer-backed renderer. For headless rendering,
    /// use `render_scene_to_image()` instead.
    #[cfg(any(test, feature = "test-support"))]
    pub fn render_to_image(&mut self, scene: &Scene) -> Result<RgbaImage> {
        let layer = self
            .layer
            .clone()
            .ok_or_else(|| anyhow::anyhow!("render_to_image requires a layer-backed renderer"))?;
        let viewport_size = layer.drawable_size();
        let viewport_size: Size<DevicePixels> = size(
            (viewport_size.width.ceil() as i32).into(),
            (viewport_size.height.ceil() as i32).into(),
        );
        let drawable = layer
            .next_drawable()
            .ok_or_else(|| anyhow::anyhow!("Failed to get drawable for render_to_image"))?;

        let command_buffer = self.render_frame(scene, drawable.texture(), viewport_size)?;

        // Commit and wait for completion without presenting
        command_buffer.commit();
        command_buffer.wait_until_completed();

        read_texture_to_image(drawable.texture())
    }

    /// Renders a scene to an image without requiring a window or CAMetalLayer.
    ///
    /// This is the primary method for headless rendering. It creates an offscreen
    /// texture, renders the scene to it, and returns the pixel data as an RGBA image.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> Result<RgbaImage> {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            anyhow::bail!("Invalid size for render_scene_to_image: {:?}", size);
        }

        // Headless callers do not have a Cocoa event-loop pool to release
        // autoreleased command buffers and render-pass descriptors.
        objc2::rc::autoreleasepool(|_| {
            // Update path intermediate textures for this size
            // Create an offscreen texture as render target
            let texture_descriptor = metal::TextureDescriptor::new();
            texture_descriptor.set_width(size.width.0 as u64);
            texture_descriptor.set_height(size.height.0 as u64);
            texture_descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
            texture_descriptor.set_usage(
                metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
            );
            // Like the atlas, only Apple GPUs can create shared textures on macOS;
            // Intel Macs cannot, even with unified memory. iOS has no managed storage.
            let uses_shared_storage = cfg!(target_os = "ios") || self.is_apple_gpu;
            texture_descriptor.set_storage_mode(if uses_shared_storage {
                metal::MTLStorageMode::Shared
            } else {
                metal::MTLStorageMode::Managed
            });
            let target_texture = self.device.new_texture(&texture_descriptor);

            let command_buffer = self.render_frame(scene, &target_texture, size)?;

            // Managed textures require an explicit blit synchronize before the CPU
            // can read back the rendered data. Without this, get_bytes returns
            // stale zeros.
            if !uses_shared_storage {
                let blit = command_buffer.new_blit_command_encoder();
                blit.synchronize_resource(&target_texture);
                blit.end_encoding();
            }

            // Commit and wait for completion
            command_buffer.commit();
            command_buffer.wait_until_completed();

            read_texture_to_image(&target_texture)
        })
    }

    /// Renders a scene to a reused offscreen texture without reading pixels
    /// back or blocking on GPU completion.
    ///
    /// This mirrors the CPU cost of presenting a frame to a window (scene
    /// encoding, instance buffer writes, command submission) and is used by
    /// headless benchmark rendering, where the produced pixels are never
    /// inspected.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> Result<()> {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            anyhow::bail!("Invalid size for render_scene: {:?}", size);
        }

        objc2::rc::autoreleasepool(|_| {
            let needs_new_target = self.headless_render_target.as_ref().is_none_or(|texture| {
                texture.width() != size.width.0 as u64 || texture.height() != size.height.0 as u64
            });
            if needs_new_target {
                let texture_descriptor = metal::TextureDescriptor::new();
                texture_descriptor.set_width(size.width.0 as u64);
                texture_descriptor.set_height(size.height.0 as u64);
                texture_descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
                texture_descriptor.set_usage(
                    metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
                );
                texture_descriptor.set_storage_mode(metal::MTLStorageMode::Private);
                self.headless_render_target = Some(self.device.new_texture(&texture_descriptor));
            }
            let target_texture = self
                .headless_render_target
                .clone()
                .expect("just ensured the render target exists");

            let command_buffer = self.render_frame(scene, &target_texture, size)?;

            // Commit without waiting, mirroring presentation to a real window where
            // the CPU doesn't block on the GPU.
            command_buffer.commit();
            Ok(())
        })
    }

    fn draw_primitives_to_texture(
        &mut self,
        scene: &Scene,
        instance_bindings: &InstanceBindings,
        writer: &mut InstanceBufferWriter,
        texture: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
    ) -> Result<metal::CommandBuffer> {
        self.frame_time = Instant::now();
        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue.new_command_buffer();
        let alpha = if self.opaque { 1. } else { 0. };
        let filtered =
            filters_enabled() && (!scene.effects.is_empty() || !scene.backdrops.is_empty());
        // Backdrops read what the frame has drawn so far, which a drawable cannot be sampled from,
        // so a frame carrying them is drawn offscreen and blitted back at the end.
        let mirrored = filtered && !scene.backdrops.is_empty();
        let main_texture = match mirrored {
            true => self.filter_texture(FilterTarget::Frame, viewport_size),
            false => texture.to_owned(),
        };

        let mut command_encoder = new_command_encoder_for_texture(
            command_buffer,
            &main_texture,
            viewport_size,
            Some(metal::MTLClearColor::new(0., 0., 0., alpha)),
        );

        let mut stack: Vec<usize> = Vec::new();
        // What each open layer will be composited through: neighbours that ask for the same filter
        // share one target, so a list of separately blurred rows costs one pass, not one per row.
        let mut spans: Vec<Bounds<ScaledPixels>> = Vec::new();
        // The part of each open layer's target that was wiped when it opened. A neighbour may only
        // share the target when everything it draws lands inside it.
        let mut wiped: Vec<Bounds<ScaledPixels>> = Vec::new();

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
                    let layer = scene.effects[*open];
                    if layer.can_merge_with(&next)
                        && wiped
                            .last()
                            .is_some_and(|wiped| covers(*wiped, layer_wipe(next)))
                    {
                        *open = wanted[shared];
                        *span = span.union(&next.destination_clip());
                        merged = true;
                        break;
                    }
                }

                let index = stack.pop().expect("the stack is not empty");
                let layer = scene.effects[index];
                let clip = spans.pop().unwrap_or_else(|| layer.destination_clip());
                wiped.pop();

                command_encoder.end_encoding();
                command_encoder = self.close_layer(
                    command_buffer,
                    layer,
                    clip,
                    stack.len(),
                    &main_texture,
                    viewport_size,
                );
            }

            for index in wanted.iter().skip(shared + usize::from(merged)) {
                if stack.len() >= LAYER_DEPTH {
                    break;
                }
                let layer = scene.effects[*index];
                let depth = stack.len();

                command_encoder.end_encoding();
                let target = self.filter_texture(FilterTarget::Layer(depth), viewport_size);
                // Clearing the whole target writes every tile of a window-sized texture, which
                // on a tiled GPU costs more than most layers draw. Only the source area, and the
                // margin a blur reads past it, is ever read back, so only that is wiped.
                command_encoder =
                    new_discarding_command_encoder(command_buffer, &target, viewport_size);
                self.clear_within(command_encoder, layer_wipe(layer), viewport_size);
                stack.push(*index);
                spans.push(layer.destination_clip());
                wiped.push(layer_wipe(layer));
            }

            match batch {
                PrimitiveBatch::Shadows(range) => {
                    self.draw_shadows(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::Quads(range) => {
                    self.draw_quads(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::Shaders { shader, range } => {
                    if let Some(pipeline) = self.custom_pipeline_state(shader) {
                        self.draw_custom_shaders(
                            &pipeline,
                            range,
                            instance_bindings,
                            viewport_size,
                            command_encoder,
                        );
                    }
                }
                PrimitiveBatch::Paths(range) => {
                    let paths = &scene.paths[range];
                    // The texture the frame is drawing into right now: the innermost open
                    // layer, or the frame itself.
                    let onto = match stack.len().checked_sub(1) {
                        Some(depth) => self.filter_texture(
                            FilterTarget::Layer(depth.min(LAYER_DEPTH - 1)),
                            viewport_size,
                        ),
                        None => main_texture.clone(),
                    };
                    command_encoder.end_encoding();

                    let targets = match paths.is_empty() {
                        true => None,
                        false => self.path_targets(viewport_size),
                    };
                    let did_draw = match &targets {
                        Some((intermediate, msaa)) => self.draw_paths_to_intermediate(
                            paths,
                            writer,
                            viewport_size,
                            command_buffer,
                            intermediate,
                            msaa.as_deref(),
                        )?,
                        None => false,
                    };

                    command_encoder =
                        new_command_encoder_for_texture(command_buffer, &onto, viewport_size, None);

                    if let Some((intermediate, _)) = targets.filter(|_| did_draw)
                        && let Err(error) = self.draw_paths_from_intermediate(
                            paths,
                            writer,
                            viewport_size,
                            command_encoder,
                            &intermediate,
                        )
                    {
                        command_encoder.end_encoding();
                        return Err(error);
                    }
                }
                PrimitiveBatch::Underlines(range) => {
                    self.draw_underlines(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::MonochromeSprites { texture_id, range } => self
                    .draw_monochrome_sprites(
                        texture_id,
                        range,
                        instance_bindings,
                        viewport_size,
                        command_encoder,
                    ),
                PrimitiveBatch::PolychromeSprites { texture_id, range } => self
                    .draw_polychrome_sprites(
                        texture_id,
                        range,
                        instance_bindings,
                        viewport_size,
                        command_encoder,
                    ),
                PrimitiveBatch::Backdrops(range) => {
                    if !filtered {
                        continue;
                    }
                    let backdrops = &scene.backdrops[range.clone()];
                    let sigma = backdrops
                        .iter()
                        .fold(0., |widest: f32, backdrop| widest.max(backdrop.blur));
                    // A backdrop samples the blur only within its own bounds, so the passes
                    // are scissored to the union of the batch's, grown by the kernel inside
                    // `blur_source`, rather than run over the whole frame.
                    let clip = backdrops
                        .iter()
                        .map(|backdrop| backdrop.bounds)
                        .reduce(|union, bounds| union.union(&bounds));

                    command_encoder.end_encoding();
                    let source = self.backdrop_source(
                        command_buffer,
                        &main_texture,
                        &wiped,
                        viewport_size,
                        clip.map(|clip| clip.dilate(BlurPasses::for_sigma(sigma).source_margin())),
                    );
                    let blurred = self.blur_source(
                        command_buffer,
                        &source,
                        false,
                        sigma,
                        clip,
                        viewport_size,
                    );
                    let target = match stack.len().checked_sub(1) {
                        Some(depth) => {
                            self.filter_texture(FilterTarget::Layer(depth), viewport_size)
                        }
                        None => main_texture.clone(),
                    };
                    command_encoder = new_command_encoder_for_texture(
                        command_buffer,
                        &target,
                        viewport_size,
                        None,
                    );
                    self.draw_backdrops(
                        range,
                        instance_bindings,
                        &blurred,
                        viewport_size,
                        command_encoder,
                    );
                }
                PrimitiveBatch::Surfaces(range) => self.draw_surfaces(
                    &scene.surfaces[range.clone()],
                    range.start,
                    instance_bindings,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::SubpixelSprites { .. } => unreachable!(),
            }
        }

        while let Some(index) = stack.pop() {
            let layer = scene.effects[index];
            let clip = spans.pop().unwrap_or_else(|| layer.destination_clip());
            wiped.pop();

            command_encoder.end_encoding();
            command_encoder = self.close_layer(
                command_buffer,
                layer,
                clip,
                stack.len(),
                &main_texture,
                viewport_size,
            );
        }

        command_encoder.end_encoding();

        if mirrored {
            self.filter_pass(
                command_buffer,
                &self.blit_pipeline_state,
                &main_texture,
                texture,
                None,
                None,
            );
        }

        self.release_targets_idle_at(self.frame_time);

        Ok(command_buffer.to_owned())
    }

    /// Blurs a filtered layer that has just closed, when its filter asks for a blur, and
    /// composites it as the first draw of a pass that resumes its parent. The caller carries on
    /// drawing through the encoder it returns. `depth` is the closed layer's own nesting depth.
    fn close_layer<'a>(
        &mut self,
        command_buffer: &'a metal::CommandBufferRef,
        layer: LayerEffect,
        clip: Bounds<ScaledPixels>,
        depth: usize,
        main_texture: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
    ) -> &'a metal::RenderCommandEncoderRef {
        let source = self.filter_texture(
            FilterTarget::Layer(depth.min(LAYER_DEPTH - 1)),
            viewport_size,
        );
        let onto = match depth.checked_sub(1) {
            Some(parent) => self.filter_texture(
                FilterTarget::Layer(parent.min(LAYER_DEPTH - 1)),
                viewport_size,
            ),
            None => main_texture.to_owned(),
        };
        let blurred = match layer.filter.blurs() {
            true => self.blur_source(
                command_buffer,
                &source,
                true,
                layer.filter.blur,
                Some(layer.blur_bounds(clip)),
                viewport_size,
            ),
            false => source,
        };
        let encoder = new_command_encoder_for_texture(command_buffer, &onto, viewport_size, None);
        self.composite_layer(encoder, &blurred, layer, clip, viewport_size);
        encoder
    }

    /// Writes transparent black over `bounds` of the encoder's target and lifts the scissor
    /// again, so the draws after it see the whole target.
    fn clear_within(
        &self,
        encoder: &metal::RenderCommandEncoderRef,
        bounds: Bounds<ScaledPixels>,
        viewport_size: Size<DevicePixels>,
    ) {
        let Some(within) = Self::scissor(bounds, viewport_size, 1) else {
            return;
        };
        encoder.set_render_pipeline_state(&self.clear_pipeline_state);
        encoder.set_scissor_rect(within);
        encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, 0, 4);
        encoder.set_scissor_rect(whole_scissor(viewport_size));
    }

    /// Clamps a region to the target it is drawn into, shrunk to the blur step it runs at.
    fn scissor(
        bounds: Bounds<ScaledPixels>,
        viewport_size: Size<DevicePixels>,
        shrink: u32,
    ) -> Option<metal::MTLScissorRect> {
        let shrink = shrink as f32;
        let width = (i32::from(viewport_size.width) as f32 / shrink).floor();
        let height = (i32::from(viewport_size.height) as f32 / shrink).floor();
        let left = (bounds.origin.x.0 / shrink).floor().clamp(0., width);
        let top = (bounds.origin.y.0 / shrink).floor().clamp(0., height);
        let right = ((bounds.origin.x.0 + bounds.size.width.0) / shrink)
            .ceil()
            .clamp(left, width);
        let bottom = ((bounds.origin.y.0 + bounds.size.height.0) / shrink)
            .ceil()
            .clamp(top, height);
        if right <= left || bottom <= top {
            return None;
        }

        Some(metal::MTLScissorRect {
            x: left as u64,
            y: top as u64,
            width: (right - left) as u64,
            height: (bottom - top) as u64,
        })
    }

    /// Draws one full-screen pass of a filter from `source` into `target`.
    fn filter_pass(
        &self,
        command_buffer: &metal::CommandBufferRef,
        pipeline: &metal::RenderPipelineState,
        source: &metal::TextureRef,
        target: &metal::TextureRef,
        params: Option<&BlurParams>,
        within: Option<metal::MTLScissorRect>,
    ) {
        let size = size(
            DevicePixels(target.width() as i32),
            DevicePixels(target.height() as i32),
        );
        // A scissored pass leaves the rest of its target alone, and the passes after it read only
        // what the passes before them wrote. Clearing the whole target first is a full write of a
        // window-sized texture, which on a tiled GPU costs more than a narrow blur itself.
        let encoder = match within {
            Some(_) => new_discarding_command_encoder(command_buffer, target, size),
            None => new_command_encoder_for_texture(
                command_buffer,
                target,
                size,
                Some(metal::MTLClearColor::new(0., 0., 0., 0.)),
            ),
        };
        encoder.set_render_pipeline_state(pipeline);
        encoder.set_fragment_texture(FilterInputIndex::Source as u64, Some(source));
        if let Some(params) = params {
            encoder.set_fragment_bytes(
                FilterInputIndex::Params as u64,
                mem::size_of::<BlurParams>() as u64,
                params as *const BlurParams as *const _,
            );
        }
        if let Some(within) = within {
            encoder.set_scissor_rect(within);
        }
        encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, 0, 4);
        encoder.end_encoding();
    }

    /// Blurs a source and returns the target holding the result. A narrow blur comes back at
    /// full size, a wide one at the level [`Kawase::settled`] names, and whatever samples it
    /// stretches it back up. A `spare` source is a layer nothing reads again, so a gaussian
    /// writes its result back into it rather than into a full-size target of its own.
    fn blur_source(
        &mut self,
        command_buffer: &metal::CommandBufferRef,
        source: &metal::TextureRef,
        spare: bool,
        sigma: f32,
        clip: Option<Bounds<ScaledPixels>>,
        viewport_size: Size<DevicePixels>,
    ) -> metal::Texture {
        match BlurPasses::for_sigma(sigma) {
            BlurPasses::Gaussian(sigma) => {
                self.gaussian_blur(command_buffer, source, spare, sigma, clip, viewport_size)
            }
            BlurPasses::Kawase(kawase) => {
                self.kawase_blur(command_buffer, source, kawase, clip, viewport_size)
            }
        }
    }

    /// Blurs a source across into the scratch target and down into level zero, or back into a
    /// `spare` source, both at full size. The pass across covers the clip grown by the kernel,
    /// since the pass down reads that far above and below it.
    fn gaussian_blur(
        &mut self,
        command_buffer: &metal::CommandBufferRef,
        source: &metal::TextureRef,
        spare: bool,
        sigma: f32,
        clip: Option<Bounds<ScaledPixels>>,
        viewport_size: Size<DevicePixels>,
    ) -> metal::Texture {
        let across = BlurParams {
            direction: [1., 0.],
            radius: sigma,
            pad: 0.,
        };
        let down = BlurParams {
            direction: [0., 1.],
            radius: sigma,
            pad: 0.,
        };
        let scratch = self.filter_texture(FilterTarget::Scratch, viewport_size);
        let settled = match spare {
            true => source.to_owned(),
            false => self.filter_texture(FilterTarget::Level(0), viewport_size),
        };
        let within = |margin: f32| {
            clip.and_then(|clip| Self::scissor(clip.dilate(ScaledPixels(margin)), viewport_size, 1))
        };
        let reach = (sigma * GAUSSIAN_REACH).ceil() + 1.;

        self.filter_pass(
            command_buffer,
            &self.gaussian_pipeline_state,
            source,
            &scratch,
            Some(&across),
            within(reach),
        );
        self.filter_pass(
            command_buffer,
            &self.gaussian_pipeline_state,
            &scratch,
            &settled,
            Some(&down),
            within(1.),
        );
        settled
    }

    /// Blurs a source with a dual Kawase blur and returns the level it settled at.
    fn kawase_blur(
        &mut self,
        command_buffer: &metal::CommandBufferRef,
        source: &metal::TextureRef,
        kawase: Kawase,
        clip: Option<Bounds<ScaledPixels>>,
        viewport_size: Size<DevicePixels>,
    ) -> metal::Texture {
        let params = BlurParams {
            direction: [0., 0.],
            radius: kawase.offset,
            pad: 0.,
        };
        let levels: Vec<metal::Texture> = (1..=kawase.levels)
            .map(|level| self.filter_texture(FilterTarget::Level(level), viewport_size))
            .collect();
        // Every pass covers the clip grown by what all the passes together read, which is at
        // least what the ones after it still need. A texel more keeps the bilinear taps on the
        // clip's own edge off texels nothing has written.
        let reach = kawase.reach();
        let within = |level: usize| {
            let shrink = 1u32 << level;
            let grown = ScaledPixels(reach + shrink as f32);
            clip.and_then(|clip| Self::scissor(clip.dilate(grown), viewport_size, shrink))
        };

        let mut from: &metal::TextureRef = source;
        for (level, to) in (1..).zip(&levels) {
            self.filter_pass(
                command_buffer,
                &self.kawase_down_pipeline_state,
                from,
                to,
                Some(&params),
                within(level),
            );
            from = to;
        }
        let settled = &levels[kawase.settled - 1];
        self.filter_pass(
            command_buffer,
            &self.kawase_up_pipeline_state,
            from,
            settled,
            Some(&params),
            within(kawase.settled),
        );
        settled.clone()
    }

    /// Draws a filtered layer back into its parent through `encoder`, clipped to the layer's bounds,
    /// and leaves the encoder open with the scissor lifted for the parent's own draws.
    fn composite_layer(
        &self,
        encoder: &metal::RenderCommandEncoderRef,
        source: &metal::TextureRef,
        layer: LayerEffect,
        clip: Bounds<ScaledPixels>,
        viewport_size: Size<DevicePixels>,
    ) {
        let Some(within) = Self::scissor(layer.destination_scissor_bounds(clip), viewport_size, 1)
        else {
            return;
        };
        let masked = layer.filter.fades();
        encoder.set_render_pipeline_state(match masked {
            true => &self.masked_pipeline_state,
            false => &self.composite_pipeline_state,
        });
        encoder.set_fragment_texture(FilterInputIndex::Source as u64, Some(source));
        encoder.set_fragment_bytes(
            FilterInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        let params = mask_params(layer, clip);
        encoder.set_fragment_bytes(
            FilterInputIndex::Params as u64,
            mem::size_of::<MaskParams>() as u64,
            &params as *const MaskParams as *const _,
        );
        encoder.set_scissor_rect(within);
        encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, 0, 4);
        encoder.set_scissor_rect(whole_scissor(viewport_size));
    }

    /// Flattens open layers in source coordinates without changing their eventual composites.
    fn backdrop_source(
        &mut self,
        command_buffer: &metal::CommandBufferRef,
        frame: &metal::TextureRef,
        wiped: &[Bounds<ScaledPixels>],
        viewport_size: Size<DevicePixels>,
        clip: Option<Bounds<ScaledPixels>>,
    ) -> metal::Texture {
        let depth = wiped.len();
        if depth == 0 {
            return frame.to_owned();
        }
        let target = self.filter_texture(FilterTarget::Backdrop, viewport_size);
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(
                ScaledPixels(viewport_size.width.0 as f32),
                ScaledPixels(viewport_size.height.0 as f32),
            ),
        );
        let params = MaskParams {
            source_bounds: bounds,
            fade_bounds: bounds,
            scale: 1.,
            ..Default::default()
        };
        // Each open layer is read only where it was wiped when it opened, since the rest of its
        // target still holds whatever an earlier layer left there.
        let mut sources = vec![(frame.to_owned(), clip)];
        sources.extend((0..depth).map(|level| {
            let within = match clip {
                Some(clip) => clip.intersect(&wiped[level]),
                None => wiped[level],
            };
            (
                self.filter_texture(FilterTarget::Layer(level), viewport_size),
                Some(within),
            )
        }));
        let encoder = new_command_encoder_for_texture(
            command_buffer,
            &target,
            viewport_size,
            Some(metal::MTLClearColor::new(0., 0., 0., 0.)),
        );
        encoder.set_render_pipeline_state(&self.composite_pipeline_state);
        encoder.set_fragment_bytes(
            FilterInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const _ as *const _,
        );
        encoder.set_fragment_bytes(
            FilterInputIndex::Params as u64,
            mem::size_of_val(&params) as u64,
            &params as *const _ as *const _,
        );
        for (source, within) in sources {
            let scissor = match within {
                Some(within) => match Self::scissor(within, viewport_size, 1) {
                    Some(scissor) => scissor,
                    None => continue,
                },
                None => whole_scissor(viewport_size),
            };
            encoder.set_scissor_rect(scissor);
            encoder.set_fragment_texture(FilterInputIndex::Source as u64, Some(&source));
            encoder.draw_primitives(metal::MTLPrimitiveType::TriangleStrip, 0, 4);
        }
        encoder.end_encoding();
        target
    }

    fn draw_backdrops(
        &self,
        backdrops: Range<usize>,
        instance_bindings: &InstanceBindings,
        source: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if backdrops.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.backdrop_punch_pipeline_state);
        command_encoder.set_vertex_buffer(
            FilterInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            FilterInputIndex::Backdrops as u64,
            Some(&instance_bindings.backdrops.buffer),
            instance_bindings.backdrops.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            FilterInputIndex::Backdrops as u64,
            Some(&instance_bindings.backdrops.buffer),
            instance_bindings.backdrops.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            FilterInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_bytes(
            FilterInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_texture(FilterInputIndex::Source as u64, Some(source));
        // The hole first, the blurred copy into it second: together they are
        // `dst = mix(dst, blurred, k)`, which leaves a see-through window as clear as it was.
        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            backdrops.len() as u64,
            backdrops.start as u64,
        );
        command_encoder.set_render_pipeline_state(&self.backdrops_pipeline_state);
        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            backdrops.len() as u64,
            backdrops.start as u64,
        );
    }

    fn draw_paths_to_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        writer: &mut InstanceBufferWriter,
        viewport_size: Size<DevicePixels>,
        command_buffer: &metal::CommandBufferRef,
        intermediate_texture: &metal::TextureRef,
        msaa_texture: Option<&metal::TextureRef>,
    ) -> Result<bool> {
        if paths.is_empty() {
            return Ok(false);
        }

        let mut vertices = Vec::new();
        for path in paths {
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationVertex {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds: path.bounds.intersect(&path.content_mask.bounds),
            }));
        }
        let vertex_instance_bindings = writer.write(&vertices)?;

        let render_pass_descriptor = metal::RenderPassDescriptor::new();
        let color_attachment = render_pass_descriptor
            .color_attachments()
            .object_at(0)
            .unwrap();
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(metal::MTLClearColor::new(0., 0., 0., 0.));

        if let Some(msaa_texture) = msaa_texture {
            color_attachment.set_texture(Some(msaa_texture));
            color_attachment.set_resolve_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::MultisampleResolve);
        } else {
            color_attachment.set_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::Store);
        }

        let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
        command_encoder.set_render_pipeline_state(&self.paths_rasterization_pipeline_state);
        command_encoder.set_vertex_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&vertex_instance_bindings.buffer),
            vertex_instance_bindings.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            PathRasterizationInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&vertex_instance_bindings.buffer),
            vertex_instance_bindings.offset as u64,
        );
        command_encoder.draw_primitives(
            metal::MTLPrimitiveType::Triangle,
            0,
            vertices.len() as u64,
        );

        command_encoder.end_encoding();
        Ok(true)
    }

    fn draw_shadows(
        &self,
        shadows: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if shadows.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.shadows_pipeline_state);
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_bindings.shadows.buffer),
            instance_bindings.shadows.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_bindings.shadows.buffer),
            instance_bindings.shadows.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            ShadowInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            shadows.len() as u64,
            shadows.start as u64,
        );
    }

    fn draw_quads(
        &self,
        quads: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if quads.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.quads_pipeline_state);
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_bindings.quads.buffer),
            instance_bindings.quads.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_bindings.quads.buffer),
            instance_bindings.quads.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            QuadInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            quads.len() as u64,
            quads.start as u64,
        );
    }

    /// The pipeline of a custom shader, built the first time a frame draws it. `None` means it
    /// failed to build, which is logged once and leaves its instances undrawn.
    fn custom_pipeline_state(&mut self, shader: ShaderId) -> Option<metal::RenderPipelineState> {
        if let Some(pipeline) = self.custom_pipeline_states.get(&shader) {
            return pipeline.clone();
        }
        let pipeline =
            build_custom_pipeline_state(&self.device, shader, MTLPixelFormat::BGRA8Unorm)
                .inspect_err(|error| log::error!("{error:#}"))
                .ok();
        self.custom_pipeline_states.insert(shader, pipeline.clone());
        pipeline
    }

    fn draw_custom_shaders(
        &self,
        pipeline: &metal::RenderPipelineStateRef,
        shaders: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if shaders.is_empty() {
            return;
        }

        // Quads blend straight alpha on Metal, so the prelude leaves its colour unpremultiplied.
        let globals = CustomShaderGlobals {
            viewport_size: [
                i32::from(viewport_size.width) as f32,
                i32::from(viewport_size.height) as f32,
            ],
            premultiplied_alpha: 0,
            pad: 0,
        };
        let buffer = &instance_bindings.shaders;
        // naga reads the length of the instance buffer from here, though nothing it emits for
        // the prelude bounds checks against it.
        let length = (buffer.buffer.length() as usize).saturating_sub(buffer.offset) as u32;

        command_encoder.set_render_pipeline_state(pipeline);
        let globals_size = mem::size_of::<CustomShaderGlobals>() as u64;
        let globals_bytes = &globals as *const CustomShaderGlobals as *const c_void;
        let length_size = mem::size_of::<u32>() as u64;
        let length_bytes = &length as *const u32 as *const c_void;
        command_encoder.set_vertex_bytes(
            gpui::MslShader::GLOBALS_BUFFER,
            globals_size,
            globals_bytes,
        );
        command_encoder.set_fragment_bytes(
            gpui::MslShader::GLOBALS_BUFFER,
            globals_size,
            globals_bytes,
        );
        command_encoder.set_vertex_bytes(gpui::MslShader::SIZES_BUFFER, length_size, length_bytes);
        command_encoder.set_fragment_bytes(
            gpui::MslShader::SIZES_BUFFER,
            length_size,
            length_bytes,
        );
        command_encoder.set_vertex_buffer(
            gpui::MslShader::INSTANCES_BUFFER,
            Some(&buffer.buffer),
            buffer.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            gpui::MslShader::INSTANCES_BUFFER,
            Some(&buffer.buffer),
            buffer.offset as u64,
        );
        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::TriangleStrip,
            0,
            4,
            shaders.len() as u64,
            shaders.start as u64,
        );
    }

    fn draw_paths_from_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        writer: &mut InstanceBufferWriter,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
        intermediate_texture: &metal::TextureRef,
    ) -> Result<()> {
        let Some(first_path) = paths.first() else {
            return Ok(());
        };

        command_encoder.set_render_pipeline_state(&self.path_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.set_fragment_texture(
            SpriteInputIndex::AtlasTexture as u64,
            Some(intermediate_texture),
        );

        // When copying paths from the intermediate texture to the drawable,
        // each pixel must only be copied once, in case of transparent paths.
        //
        // If all paths have the same draw order, then their bounds are all
        // disjoint, so we can copy each path's bounds individually. If this
        // batch combines different draw orders, we perform a single copy
        // for a minimal spanning rect.
        let sprites;
        if paths.last().unwrap().order == first_path.order {
            sprites = paths
                .iter()
                .map(|path| PathSprite {
                    bounds: path.clipped_bounds(),
                })
                .collect();
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            sprites = vec![PathSprite { bounds }];
        }

        let sprite_instance_bindings = writer.write(&sprites)?;
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&sprite_instance_bindings.buffer),
            sprite_instance_bindings.offset as u64,
        );

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        Ok(())
    }

    fn draw_underlines(
        &self,
        underlines: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if underlines.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.underlines_pipeline_state);
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_bindings.underlines.buffer),
            instance_bindings.underlines.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_bindings.underlines.buffer),
            instance_bindings.underlines.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            UnderlineInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            underlines.len() as u64,
            underlines.start as u64,
        );
    }

    fn draw_monochrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if sprites.is_empty() {
            return;
        }

        let Some(texture) = self.sprite_atlas.metal_texture(texture_id) else {
            return;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.monochrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.monochrome_sprites.buffer),
            instance_bindings.monochrome_sprites.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.monochrome_sprites.buffer),
            instance_bindings.monochrome_sprites.offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
            sprites.start as u64,
        );
    }

    fn draw_polychrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if sprites.is_empty() {
            return;
        }

        let Some(texture) = self.sprite_atlas.metal_texture(texture_id) else {
            return;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.polychrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.polychrome_sprites.buffer),
            instance_bindings.polychrome_sprites.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.polychrome_sprites.buffer),
            instance_bindings.polychrome_sprites.offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
            sprites.start as u64,
        );
    }

    fn draw_surfaces(
        &mut self,
        surfaces: &[PaintSurface],
        first_surface: usize,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if surfaces.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.surfaces_pipeline_state);
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Surfaces as u64,
            Some(&instance_bindings.surfaces.buffer),
            instance_bindings.surfaces.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SurfaceInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        for (index, surface) in surfaces.iter().enumerate() {
            let image_buffer = &surface.image_buffer;
            let texture_size = size(
                DevicePixels::from(CVPixelBufferGetWidth(image_buffer) as i32),
                DevicePixels::from(CVPixelBufferGetHeight(image_buffer) as i32),
            );

            assert_eq!(
                CVPixelBufferGetPixelFormatType(image_buffer),
                kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
            );

            let plane_texture = |plane, format| unsafe {
                let mut texture = ptr::null_mut();
                let status = CVMetalTextureCache::create_texture_from_image(
                    None,
                    &self.core_video_texture_cache,
                    image_buffer,
                    None,
                    format,
                    CVPixelBufferGetWidthOfPlane(image_buffer, plane),
                    CVPixelBufferGetHeightOfPlane(image_buffer, plane),
                    plane,
                    ptr::NonNull::from(&mut texture),
                );
                assert_eq!(
                    status, kCVReturnSuccess,
                    "failed to create CoreVideo Metal texture"
                );
                CFRetained::<CVMetalTexture>::from_raw(
                    ptr::NonNull::new(texture).expect("CoreVideo returned a null texture"),
                )
            };
            let y_texture = plane_texture(0, objc2_metal::MTLPixelFormat::R8Unorm);
            let cb_cr_texture = plane_texture(1, objc2_metal::MTLPixelFormat::RG8Unorm);

            command_encoder.set_vertex_bytes(
                SurfaceInputIndex::TextureSize as u64,
                mem::size_of_val(&texture_size) as u64,
                &texture_size as *const Size<DevicePixels> as *const _,
            );
            command_encoder.set_fragment_texture(SurfaceInputIndex::YTexture as u64, unsafe {
                let texture = CVMetalTextureGetTexture(&y_texture)
                    .expect("CoreVideo returned no Metal texture");
                Some(metal::TextureRef::from_ptr(
                    objc2::rc::Retained::as_ptr(&texture) as *mut _,
                ))
            });
            command_encoder.set_fragment_texture(SurfaceInputIndex::CbCrTexture as u64, unsafe {
                let texture = CVMetalTextureGetTexture(&cb_cr_texture)
                    .expect("CoreVideo returned no Metal texture");
                Some(metal::TextureRef::from_ptr(
                    objc2::rc::Retained::as_ptr(&texture) as *mut _,
                ))
            });

            command_encoder.draw_primitives_instanced_base_instance(
                metal::MTLPrimitiveType::Triangle,
                0,
                6,
                1,
                (first_surface + index) as u64,
            );
        }
    }
}

/// Layer filters can be switched off from the environment to tell a rendering problem from a
/// compositing one.
fn filters_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("GPUI_FILTERS").as_deref() != Ok("0"))
}

/// Whether `outer` holds all of `inner`.
fn covers(outer: Bounds<ScaledPixels>, inner: Bounds<ScaledPixels>) -> bool {
    outer.origin.x <= inner.origin.x
        && outer.origin.y <= inner.origin.y
        && outer.bottom_right().x >= inner.bottom_right().x
        && outer.bottom_right().y >= inner.bottom_right().y
}

/// The part of a layer's target that has to start transparent: its source area and, for a blurred
/// layer, as far again as the blur reads past it, plus a pixel for the rounding of the scissor.
fn layer_wipe(layer: LayerEffect) -> Bounds<ScaledPixels> {
    let margin = match layer.filter.blurs() {
        true => BlurPasses::for_sigma(layer.filter.blur).source_margin(),
        false => ScaledPixels(0.),
    };
    layer.source_bounds.dilate(margin + ScaledPixels(1.))
}

/// A scissor over the whole target, for lifting one that was set for a single draw.
fn whole_scissor(viewport_size: Size<DevicePixels>) -> metal::MTLScissorRect {
    metal::MTLScissorRect {
        x: 0,
        y: 0,
        width: i32::from(viewport_size.width).max(0) as u64,
        height: i32::from(viewport_size.height).max(0) as u64,
    }
}

/// Starts a render pass into `texture`. The pass descriptor retains the texture, so it may be a
/// temporary the caller drops before the encoder ends.
/// An encoder on `texture` that keeps nothing it held, for a pass that writes everything it later
/// reads.
fn new_discarding_command_encoder<'a>(
    command_buffer: &'a metal::CommandBufferRef,
    texture: &metal::TextureRef,
    viewport_size: Size<DevicePixels>,
) -> &'a metal::RenderCommandEncoderRef {
    let render_pass_descriptor = metal::RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .unwrap();
    color_attachment.set_texture(Some(texture));
    color_attachment.set_load_action(metal::MTLLoadAction::DontCare);
    color_attachment.set_store_action(metal::MTLStoreAction::Store);

    let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    command_encoder.set_viewport(metal::MTLViewport {
        originX: 0.0,
        originY: 0.0,
        width: i32::from(viewport_size.width) as f64,
        height: i32::from(viewport_size.height) as f64,
        znear: 0.0,
        zfar: 1.0,
    });
    command_encoder
}

fn new_command_encoder_for_texture<'a>(
    command_buffer: &'a metal::CommandBufferRef,
    texture: &metal::TextureRef,
    viewport_size: Size<DevicePixels>,
    clear_color: Option<metal::MTLClearColor>,
) -> &'a metal::RenderCommandEncoderRef {
    let render_pass_descriptor = metal::RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .unwrap();
    color_attachment.set_texture(Some(texture));
    color_attachment.set_store_action(metal::MTLStoreAction::Store);
    if let Some(clear_color) = clear_color {
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(clear_color);
    } else {
        color_attachment.set_load_action(metal::MTLLoadAction::Load);
    }

    let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    command_encoder.set_viewport(metal::MTLViewport {
        originX: 0.0,
        originY: 0.0,
        width: i32::from(viewport_size.width) as f64,
        height: i32::from(viewport_size.height) as f64,
        znear: 0.0,
        zfar: 1.0,
    });
    command_encoder
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
fn read_texture_to_image(texture: &metal::TextureRef) -> Result<RgbaImage> {
    let width = texture.width() as u32;
    let height = texture.height() as u32;
    let bytes_per_row = width as usize * 4;
    let mut pixels = vec![0u8; height as usize * bytes_per_row];

    let region = metal::MTLRegion {
        origin: metal::MTLOrigin { x: 0, y: 0, z: 0 },
        size: metal::MTLSize {
            width: width as u64,
            height: height as u64,
            depth: 1,
        },
    };
    texture.get_bytes(
        pixels.as_mut_ptr() as *mut std::ffi::c_void,
        bytes_per_row as u64,
        region,
        0,
    );

    // Convert BGRA to RGBA (swap B and R channels)
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    RgbaImage::from_raw(width, height, pixels).context("failed to create RgbaImage from pixel data")
}

/// An offscreen target created the first time a frame needs it and released once it has gone
/// `IDLE_RELEASE` without use.
struct Scratch {
    texture: metal::Texture,
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

/// Offscreen targets a frame needs to blur and mask what it has drawn, all made for one
/// drawable size. Each one is allocated the first time a frame draws through it, so a frame
/// with one shallow blur pays for the targets it touches rather than for every depth and level.
#[derive(Default)]
struct FilterTargets {
    size: Size<DevicePixels>,
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

/// The textures paths are rasterized through, made for one drawable size.
struct PathTargets {
    size: Size<DevicePixels>,
    intermediate: metal::Texture,
    /// The multisampled target paths are rasterized into and resolved from. Memoryless on
    /// Apple GPUs, so it only costs memory on Intel Macs.
    msaa: Option<metal::Texture>,
    used: Instant,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BlurParams {
    pub direction: [f32; 2],
    pub radius: f32,
    pub pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MaskParams {
    pub source_bounds: Bounds<ScaledPixels>,
    pub fade_bounds: Bounds<ScaledPixels>,
    pub transform_origin: [f32; 2],
    pub translation: [f32; 2],
    pub scale: f32,
    pub fade_top: f32,
    pub fade_bottom: f32,
    pub fade_left: f32,
    pub fade_right: f32,
    pub pad: f32,
}

const _: () = {
    assert!(mem::size_of::<MaskParams>() == 18 * mem::size_of::<f32>());
    assert!(mem::offset_of!(MaskParams, source_bounds) == 0);
    assert!(mem::offset_of!(MaskParams, fade_bounds) == 16);
    assert!(mem::offset_of!(MaskParams, transform_origin) == 32);
    assert!(mem::offset_of!(MaskParams, translation) == 40);
    assert!(mem::offset_of!(MaskParams, scale) == 48);
    assert!(mem::offset_of!(MaskParams, fade_top) == 52);
    assert!(mem::offset_of!(MaskParams, fade_bottom) == 56);
    assert!(mem::offset_of!(MaskParams, fade_left) == 60);
    assert!(mem::offset_of!(MaskParams, fade_right) == 64);
    assert!(mem::offset_of!(MaskParams, pad) == 68);
};

#[repr(C)]
pub enum FilterInputIndex {
    Vertices = 0,
    Backdrops = 1,
    ViewportSize = 2,
    Params = 3,
    Source = 4,
}

fn mask_params(layer: LayerEffect, destination_clip: Bounds<ScaledPixels>) -> MaskParams {
    MaskParams {
        source_bounds: if layer.filter.transforms() {
            layer.source_bounds
        } else {
            destination_clip
        },
        // Preserve the existing Metal fade clip for unscaled layers. A scaled layer must apply
        // the fade in source coordinates before the final transform.
        fade_bounds: if layer.filter.transforms() {
            layer.source_bounds
        } else {
            destination_clip
        },
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

/// How a filter pipeline lands on what it covers.
#[derive(Clone, Copy, PartialEq)]
enum FilterBlend {
    /// Replaces the destination outright.
    Replace,
    /// Premultiplied source-over.
    Over,
    /// Adds itself to the destination, for the second half of a backdrop's mix.
    Add,
    /// Keeps `1 - src.a` of the destination and contributes nothing of its own, for the first
    /// half of a backdrop's mix.
    Punch,
}

fn build_filter_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
    blend: FilterBlend,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(blend != FilterBlend::Replace);
    if let Some((source, destination)) = match blend {
        FilterBlend::Replace => None,
        FilterBlend::Over => Some((
            metal::MTLBlendFactor::One,
            metal::MTLBlendFactor::OneMinusSourceAlpha,
        )),
        FilterBlend::Add => Some((metal::MTLBlendFactor::One, metal::MTLBlendFactor::One)),
        FilterBlend::Punch => Some((
            metal::MTLBlendFactor::Zero,
            metal::MTLBlendFactor::OneMinusSourceAlpha,
        )),
    } {
        color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
        color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
        color_attachment.set_source_rgb_blend_factor(source);
        color_attachment.set_source_alpha_blend_factor(source);
        color_attachment.set_destination_rgb_blend_factor(destination);
        color_attachment.set_destination_alpha_blend_factor(destination);
    }

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::SourceAlpha);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

/// The globals the custom shader prelude reads, laid out as its `GpuiShaderGlobals`.
#[repr(C)]
struct CustomShaderGlobals {
    viewport_size: [f32; 2],
    premultiplied_alpha: u32,
    pad: u32,
}

/// Translates a custom shader to MSL, compiles it and builds a pipeline that blends it the way
/// quads are blended.
fn build_custom_pipeline_state(
    device: &metal::DeviceRef,
    shader: ShaderId,
    pixel_format: metal::MTLPixelFormat,
) -> Result<metal::RenderPipelineState> {
    let program = gpui::shader_program(shader)
        .with_context(|| format!("custom shader {} was never registered", shader.0))?;
    let msl = program.to_msl()?;
    let library = device
        .new_library_with_source(&msl.source, &metal::CompileOptions::new())
        .map_err(|error| anyhow::anyhow!("custom shader `{}`: {error}", program.label))?;
    let function = |name: &str| {
        library
            .get_function(name, None)
            .map_err(|error| anyhow::anyhow!("custom shader `{}`: {error}", program.label))
    };
    let vertex_fn = function(&msl.vertex_entry)?;
    let fragment_fn = function(&msl.fragment_entry)?;

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(&program.label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::SourceAlpha);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .map_err(|error| anyhow::anyhow!("custom shader `{}`: {error}", program.label))
}

fn build_path_sprite_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_path_rasterization_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
    path_sample_count: u32,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    if path_sample_count > 1 {
        descriptor.set_raster_sample_count(path_sample_count as _);
        descriptor.set_alpha_to_coverage_enabled(false);
    }
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

#[derive(Clone)]
struct InstanceBinding {
    buffer: metal::Buffer,
    offset: usize,
}

struct InstanceBindings {
    backdrops: InstanceBinding,
    quads: InstanceBinding,
    shaders: InstanceBinding,
    shadows: InstanceBinding,
    underlines: InstanceBinding,
    monochrome_sprites: InstanceBinding,
    polychrome_sprites: InstanceBinding,
    surfaces: InstanceBinding,
}

fn write_instances(scene: &Scene, writer: &mut InstanceBufferWriter) -> Result<InstanceBindings> {
    Ok(InstanceBindings {
        backdrops: writer.write(&scene.backdrops)?,
        quads: writer.write(&scene.quads)?,
        shaders: writer.write(&scene.shaders)?,
        shadows: writer.write(&scene.shadows)?,
        underlines: writer.write(&scene.underlines)?,
        monochrome_sprites: writer.write(&scene.monochrome_sprites)?,
        polychrome_sprites: writer.write(&scene.polychrome_sprites)?,
        surfaces: writer.write_iter(scene.surfaces.iter().map(|surface| SurfaceBounds {
            bounds: surface.bounds,
            content_mask: surface.content_mask,
        }))?,
    })
}

struct InstanceBufferWriter {
    device: metal::Device,
    pool: Arc<Mutex<InstanceBufferPool>>,
    unified_memory: bool,
    filled: Vec<(InstanceBuffer, usize)>,
    current: InstanceBuffer,
    offset: usize,
}

impl InstanceBufferWriter {
    fn new(
        device: &metal::Device,
        pool: &Arc<Mutex<InstanceBufferPool>>,
        unified_memory: bool,
    ) -> Self {
        let current = pool.lock().acquire(device, unified_memory);
        Self {
            device: device.clone(),
            pool: pool.clone(),
            unified_memory,
            filled: Vec::new(),
            current,
            offset: 0,
        }
    }

    fn allocate<T>(&mut self, count: usize) -> Result<(InstanceBinding, &mut [MaybeUninit<T>])> {
        let size = mem::size_of::<T>() * count;
        let mut offset = self.offset.next_multiple_of(INSTANCE_BUFFER_ALIGNMENT);
        if offset + size > self.current.size {
            self.grow(size)?;
            offset = 0;
        }
        self.offset = offset + size;

        let binding = InstanceBinding {
            buffer: self.current.metal_buffer.clone(),
            offset,
        };
        // Safety: the reservation lies within a buffer this frame owns
        // exclusively, and never overlaps one handed out earlier.
        let values = unsafe {
            let start = (self.current.metal_buffer.contents() as *mut u8).add(offset);
            slice::from_raw_parts_mut(start.cast::<MaybeUninit<T>>(), count)
        };
        Ok((binding, values))
    }

    fn write<T>(&mut self, values: &[T]) -> Result<InstanceBinding> {
        let (binding, destination) = self.allocate::<T>(values.len())?;
        unsafe {
            ptr::copy_nonoverlapping(
                values.as_ptr(),
                destination.as_mut_ptr().cast::<T>(),
                values.len(),
            );
        }
        Ok(binding)
    }

    fn write_iter<T>(
        &mut self,
        values: impl ExactSizeIterator<Item = T>,
    ) -> Result<InstanceBinding> {
        let (binding, destination) = self.allocate::<T>(values.len())?;
        for (slot, value) in destination.iter_mut().zip(values) {
            slot.write(value);
        }
        Ok(binding)
    }

    fn grow(&mut self, required: usize) -> Result<()> {
        let mut pool = self.pool.lock();
        let buffer_size = (pool.buffer_size * 2)
            .max(required.next_power_of_two())
            .min(MAX_INSTANCE_BUFFER_SIZE);
        anyhow::ensure!(
            buffer_size >= required,
            "instance buffer needs {required} bytes, above the maximum of {MAX_INSTANCE_BUFFER_SIZE}"
        );
        anyhow::ensure!(
            buffer_size > self.current.size,
            "frame instance data exceeds the {MAX_INSTANCE_BUFFER_SIZE}-byte maximum"
        );
        if buffer_size != pool.buffer_size {
            log::info!("increased instance buffer size to {buffer_size}");
            pool.reset(buffer_size);
        }
        let buffer = pool.acquire(&self.device, self.unified_memory);
        drop(pool);

        let filled = mem::replace(&mut self.current, buffer);
        self.filled.push((filled, self.offset));
        self.offset = 0;
        Ok(())
    }

    fn finish(self) -> InstanceBuffer {
        let Self {
            unified_memory,
            filled,
            current,
            offset,
            ..
        } = self;

        if !unified_memory {
            for (buffer, written) in &filled {
                if *written == 0 {
                    continue;
                }
                buffer.metal_buffer.did_modify_range(NSRange {
                    location: 0,
                    length: *written as NSUInteger,
                });
            }
            if offset > 0 {
                current.metal_buffer.did_modify_range(NSRange {
                    location: 0,
                    length: offset as NSUInteger,
                });
            }
        }

        // Metal retains encoded resources until the command buffer completes.
        // Only the final, largest buffer is worth keeping in the pool.
        drop(filled);
        current
    }
}

#[repr(C)]
enum ShadowInputIndex {
    Vertices = 0,
    Shadows = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum QuadInputIndex {
    Vertices = 0,
    Quads = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum UnderlineInputIndex {
    Vertices = 0,
    Underlines = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum SpriteInputIndex {
    Vertices = 0,
    Sprites = 1,
    ViewportSize = 2,
    AtlasTextureSize = 3,
    AtlasTexture = 4,
}

#[repr(C)]
enum SurfaceInputIndex {
    Vertices = 0,
    Surfaces = 1,
    ViewportSize = 2,
    TextureSize = 3,
    YTexture = 4,
    CbCrTexture = 5,
}

#[repr(C)]
enum PathRasterizationInputIndex {
    Vertices = 0,
    ViewportSize = 1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct PathSprite {
    pub bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SurfaceBounds {
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
pub struct MetalHeadlessRenderer {
    renderer: MetalRenderer,
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
impl MetalHeadlessRenderer {
    pub fn new() -> Self {
        let instance_buffer_pool = Arc::new(Mutex::new(InstanceBufferPool::default()));
        let renderer = MetalRenderer::new_headless(instance_buffer_pool);
        Self { renderer }
    }
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
impl gpui::PlatformHeadlessRenderer for MetalHeadlessRenderer {
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> anyhow::Result<image::RgbaImage> {
        self.renderer.render_scene_to_image(scene, size)
    }

    fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.renderer.render_scene(scene, size)
    }

    fn sprite_atlas(&self) -> Arc<dyn gpui::PlatformAtlas> {
        self.renderer.sprite_atlas().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_foundation::{CFDictionary, CFType};
    use objc2_core_video::{
        CVPixelBufferCreate, CVPixelBufferGetBaseAddressOfPlane,
        CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
        CVPixelBufferUnlockBaseAddress, kCVPixelBufferIOSurfacePropertiesKey,
    };

    #[test]
    fn render_core_video_surface() {
        let attributes = CFDictionary::from_slices(
            &[unsafe { kCVPixelBufferIOSurfacePropertiesKey }],
            &[&*CFDictionary::<CFType, CFType>::empty()],
        );
        let mut image_buffer = ptr::null_mut();
        assert_eq!(
            unsafe {
                CVPixelBufferCreate(
                    None,
                    16,
                    16,
                    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                    Some(attributes.as_opaque()),
                    ptr::NonNull::from(&mut image_buffer),
                )
            },
            kCVReturnSuccess
        );
        let image_buffer =
            unsafe { CFRetained::from_raw(ptr::NonNull::new(image_buffer).unwrap()) };
        unsafe {
            assert_eq!(
                CVPixelBufferLockBaseAddress(&image_buffer, CVPixelBufferLockFlags::empty()),
                kCVReturnSuccess
            );
            for plane in 0..2 {
                let bytes = CVPixelBufferGetBaseAddressOfPlane(&image_buffer, plane).cast::<u8>();
                let length = CVPixelBufferGetBytesPerRowOfPlane(&image_buffer, plane)
                    * CVPixelBufferGetHeightOfPlane(&image_buffer, plane);
                slice::from_raw_parts_mut(bytes, length).fill(128);
            }
            assert_eq!(
                CVPixelBufferUnlockBaseAddress(&image_buffer, CVPixelBufferLockFlags::empty()),
                kCVReturnSuccess
            );
        }

        let bounds = Bounds {
            origin: Point::default(),
            size: size(ScaledPixels(16.0), ScaledPixels(16.0)),
        };
        let mut scene = Scene::default();
        scene.insert_primitive(PaintSurface {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            image_buffer: image_buffer.clone(),
        });
        drop(image_buffer);
        scene.finish();

        let mut renderer = MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
        let image = renderer
            .render_scene_to_image(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();
        for image::Rgba([red, green, blue, alpha]) in image.pixels() {
            assert!(red.abs_diff(128) <= 2);
            assert!(green.abs_diff(128) <= 2);
            assert!(blue.abs_diff(128) <= 2);
            assert_eq!(*alpha, 255);
        }
    }
}

#[cfg(test)]
#[path = "../../gpui/src/backdrop_test_fixture.rs"]
mod backdrop_fixture;

#[cfg(test)]
#[test]
fn nested_backdrops_sample_ancestor_layers() -> anyhow::Result<()> {
    use gpui::PlatformHeadlessRenderer as _;
    let mut renderer = MetalHeadlessRenderer::new();
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
