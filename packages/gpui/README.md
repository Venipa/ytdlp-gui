# gpui

[Sonora](https://github.com/sonorahq/sonora)'s fork of [GPUI](https://github.com/zed-industries/zed),
the GPU-accelerated UI framework behind Zed. Everything below works on wgpu, Metal and DirectX.

## What the fork adds

- **Shapes and shadows**
  - `Styled::corner_smoothing` and `Styled::squircle` for squircle corners
  - `Styled::text_shadow` for CSS-style text shadows
  - Box shadows clipped to outside their element

- **Blur and layers**
  - `Styled::blur` blurs an element and its subtree as a layer
  - `Styled::backdrop_blur`, like CSS `backdrop-filter: blur()`
  - `Styled::fade_edges` and `Styled::fade_sides` fade a layer at its edges
  - Dual Kawase blur, with a Gaussian for small radii
  - Neighbouring layers with the same filter merge into one pass
  - `GPUI_FILTERS=0` turns layer filters off

- **Paint-only transforms**
  - `Styled::layer_scale` and `Styled::layer_scale_origin`
  - `Styled::layer_translate`, which leaves layout and hitboxes alone
  - Fractional values for smooth spring animations

- **Pixel snapping**
  - `Window::with_pixel_snapping` keeps a subtree off the pixel grid

- **Text**
  - `Styled::letter_spacing`, like CSS `letter-spacing`, in ems
  - Requested font weights honoured in cosmic-text
  - Variable-font `wght` axis
  - Weight-aware font fallback
  - Colour emoji detection and rendering
  - ClearType text inside layers on Windows and wgpu

- **Memory**
  - Offscreen targets allocated on first use and freed when idle
  - Instance buffers shrink after a quiet period on wgpu and Metal
  - Atlas tiles written straight to the wgpu queue

- **App and windows**
  - `App::hide_cursor` hides the cursor until the mouse moves
  - Window controls keep their hitboxes in cached views
  - Explicitly sized images keep their size
  - Wayland edges kept off half device pixels, and better fractional scaling

- **Profiling and debugging**
  - `GPUI_DEBUG_OVERLAY=minimal|full` for frame timings and view statistics
  - `GPUI_SHOW_REPAINTS=1` highlights what each frame redraws
  - `GPUI_LOG_NOTIFIES=1` logs notifies and re-renders

## Licence

Apache-2.0, inherited from Zed. See `LICENSE` and `NOTICE`.
