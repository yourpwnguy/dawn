//! GPU compositing for the live window.
//!
//! The UI pixels themselves still come from tiny-skia. That is deliberate: the
//! shader path has to keep every rounded corner, gradient, rotated layer,
//! bracket, and glyph identical to `--screenshot`, and reimplementing all of
//! that on the GPU would be a rewrite, not a performance pass.
//!
//! What moves to the GPU is the expensive part that used to run on the CPU for
//! every video frame: putting the wallpaper under a full-screen transparent UI
//! layer and handing the result to the compositor. The UI overlay is uploaded
//! only when the UI actually changes, so a 30 fps wallpaper costs one texture
//! upload and one full-screen triangle per frame instead of a full tiny-skia
//! repaint and a softbuffer copy.
//!
//! Colours stay in sRGB end to end. The UI and wallpaper textures are
//! `Rgba8Unorm`, the surface format is chosen to be non-sRGB, and the shader
//! blends the already-premultiplied UI pixels over the wallpaper. That matches
//! tiny-skia's 8-bit sRGB compositing; an sRGB surface would insert a linear
//! conversion at exactly the wrong point and shift every antialiased edge.
//!
//! Failure policy: construction returns a readable error and the window falls
//! back to the softbuffer path. A greeter must never fail to show a login form
//! because a driver is missing. Once a frame is lost, we reconfigure and retry
//! on the next redraw instead of exiting.

use std::sync::Arc;
use std::time::Instant;

use tiny_skia::Pixmap;
use tracing::{debug, info, warn};
use winit::event_loop::OwnedDisplayHandle;
use winit::window::Window;

use crate::color::Rgb;
use crate::config::{BlurConfig, GpuBackend, GpuConfig, GpuDevice, GpuPower, GpuPresent};
use crate::ui::background::{BackgroundSnapshot, SnapshotKind};
use crate::ui::motion::{CameraFrame, PostFrame};

/// Uniform data shared with `gpu.wgsl`. Keep this layout in sync with the WGSL
/// struct. The camera and post `vec4`s are the ENCORE compositor state: scale
/// and shake for the whole scene, then chromatic, vignette, and glitch values.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
  top: [f32; 4],
  bottom: [f32; 4],
  screen: [f32; 2],
  bg_size: [f32; 2],
  prev_size: [f32; 2],
  opacity: f32,
  mode: u32,
  has_prev: u32,
  _pad: u32,
  /// Two scalars of padding: WGSL aligns a `vec4` to 16 bytes, while a Rust
  /// `[f32; 4]` only needs 4. Without this the camera would start at offset 72
  /// and the shader would read every field from the wrong place.
  _align: [f32; 2],
  /// scale, dx in px, dy in px, unused.
  camera: [f32; 4],
  /// chromatic px, vignette, glitch strength, glitch seed.
  post: [f32; 4],
  /// x, y, width, height of the splash underline in physical pixels.
  splash: [f32; 4],
  /// straight RGBA for the underline.
  splash_color: [f32; 4],
  /// 1 while the shader should draw the cached splash underline.
  splash_on: u32,
  /// 1 while the static decor texture should be composited under the UI.
  decor_on: u32,
  _flags_pad: [u32; 2],
}

/// The animated part of a fully covering splash. The static title card is an
/// ordinary UI texture, so only this line changes between frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplashOverlay {
  pub rect: [f32; 4],
  pub color: [f32; 4],
}

const MODE_IMAGE: u32 = 0;
const MODE_SOLID: u32 = 1;
const MODE_GRADIENT: u32 = 2;

/// One texture plus the size it was created with. Texture views cannot change
/// size in place, so a resize means a new texture and a new bind group.
struct TextureSlot {
  texture: wgpu::Texture,
  view: wgpu::TextureView,
  size: (u32, u32),
}

impl TextureSlot {
  fn new(device: &wgpu::Device, size: (u32, u32), label: &str) -> Self {
    Self::with_usage(
      device,
      size,
      label,
      wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    )
  }

  /// Same slot with an explicit usage mask. The blur chain needs a render
  /// target and, on the last texture, a copy source; a slot built for the
  /// composite path must keep the original mask so its validation stays put.
  fn with_usage(
    device: &wgpu::Device,
    size: (u32, u32),
    label: &str,
    usage: wgpu::TextureUsages,
  ) -> Self {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label: Some(label),
      size: wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count: 1,
      dimension: wgpu::TextureDimension::D2,
      // Non-sRGB on purpose, same as the composite textures: the pixels are
      // already sRGB-encoded and we filter them in that space.
      format: wgpu::TextureFormat::Rgba8Unorm,
      usage,
      view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Self {
      texture,
      view,
      size,
    }
  }
}

/// Uniform shared with `blur.wgsl`. Three `vec4`s keep the layout at exactly
/// 48 bytes and 16-byte aligned, so the Rust and WGSL structs cannot drift.
/// The fields are named after the WGSL members, not their individual values.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurParams {
  /// dir.x, dir.y, radius, contrast
  dir_radius: [f32; 4],
  /// vibrancy, vibrancy_darkness, noise, brightness
  color: [f32; 4],
  /// finish (0 horizontal, 1 vertical), passes, unused, unused
  flags: [f32; 4],
}

/// Everything the blur chain owns for one source size. A size change means a
/// new input, temp, output, staging buffer, and two fresh bind groups, because
/// none of those can be resized in place.
struct BlurTargets {
  size: (u32, u32),
  input: TextureSlot,
  temp: TextureSlot,
  output: TextureSlot,
  staging: wgpu::Buffer,
  padded_row: u32,
  horizontal_params: wgpu::Buffer,
  vertical_params: wgpu::Buffer,
  horizontal: wgpu::BindGroup,
  vertical: wgpu::BindGroup,
}

/// The two wallpaper frames plus the kind of background currently bound.
/// `frame_id` catches animation steps, the pointer catches a reload that
/// happens to reuse a frame number.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FrameKey {
  frame_id: u64,
  ptr: usize,
}

impl FrameKey {
  fn of(frame_id: u64, frame: &Arc<Pixmap>) -> Self {
    Self {
      frame_id,
      ptr: Arc::as_ptr(frame) as usize,
    }
  }

  /// The previous frame only changes when the crossfade starts. It has no
  /// stable frame number of its own, so pointer identity is the key.
  fn pointer(frame: &Arc<Pixmap>) -> Self {
    Self {
      frame_id: 0,
      ptr: Arc::as_ptr(frame) as usize,
    }
  }
}

pub struct GpuRenderer {
  // The surface is declared before the instance so it is dropped first. The
  // instance owns the backend context that created the surface.
  surface: wgpu::Surface<'static>,
  _instance: wgpu::Instance,
  device: wgpu::Device,
  queue: wgpu::Queue,
  config: wgpu::SurfaceConfiguration,
  pipeline: wgpu::RenderPipeline,
  bind_group_layout: wgpu::BindGroupLayout,
  sampler: wgpu::Sampler,
  globals_buffer: wgpu::Buffer,
  globals: Globals,
  bind_group: wgpu::BindGroup,
  background: TextureSlot,
  previous: TextureSlot,
  ui: TextureSlot,
  /// Still decor baked into one screen-sized texture. It sits between the
  /// wallpaper and the live UI, so the wing image does not have to be blended
  /// into the CPU overlay every frame.
  decor: TextureSlot,
  bg_key: Option<FrameKey>,
  prev_key: Option<FrameKey>,
  decor_key: Option<u64>,
  ui_uploaded: bool,
  max_dimension: u32,
  reconfigure: bool,
  /// Separable Gaussian used by [`Self::blur`]. Built once; the textures and
  /// bind groups live in `blur_targets` and are rebuilt only on a size change.
  blur_pipeline: wgpu::RenderPipeline,
  blur_layout: wgpu::BindGroupLayout,
  blur_sampler: wgpu::Sampler,
  blur_targets: Option<BlurTargets>,
  /// Warn at most once per run while the GPU blur keeps failing. The CPU
  /// fallback is expected to recover visually, so a warning every refresh
  /// would just flood the journal.
  blur_failed_logged: bool,
}

mod adapters;
mod blur;
mod init;
mod present;
mod surface;

pub use adapters::list_adapters;
use adapters::*;

#[cfg(test)]
mod tests;
