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

use tiny_skia::Pixmap;
use tracing::{debug, info};
use winit::event_loop::OwnedDisplayHandle;
use winit::window::Window;

use crate::color::Rgb;
use crate::ui::background::{BackgroundSnapshot, SnapshotKind};

/// Uniform data shared with `gpu.wgsl`. Keep this layout in sync with the WGSL
/// struct; the final two scalars exist only to satisfy the 16-byte uniform
/// alignment of the two `vec4`s at the front.
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
  _tail: [f32; 2],
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
      // Non-sRGB on purpose. The pixels are already sRGB-encoded and we blend
      // them in that space, just like the CPU canvas.
      format: wgpu::TextureFormat::Rgba8Unorm,
      usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
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
  bg_key: Option<FrameKey>,
  prev_key: Option<FrameKey>,
  ui_uploaded: bool,
  max_dimension: u32,
  reconfigure: bool,
}

impl GpuRenderer {
  /// Builds the compositor for one window. The caller owns the fallback: an
  /// `Err` here means "use softbuffer", never "the greeter cannot start".
  pub fn new(
    window: Arc<Window>,
    display: OwnedDisplayHandle,
    size: (u32, u32),
  ) -> Result<Self, String> {
    let size = (size.0.max(1), size.1.max(1));

    let mut descriptor = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display));
    // Vulkan first, GL as the older-hardware fallback. Both are compiled in;
    // wgpu picks whichever can present to this surface.
    descriptor.backends = wgpu::Backends::VULKAN | wgpu::Backends::GL;
    let instance = wgpu::Instance::new(descriptor);

    let surface = instance
      .create_surface(window)
      .map_err(|error| format!("cannot create a GPU surface: {error}"))?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
      power_preference: wgpu::PowerPreference::LowPower,
      force_fallback_adapter: false,
      compatible_surface: Some(&surface),
      apply_limit_buckets: false,
    }))
    .map_err(|error| format!("no usable GPU adapter: {error}"))?;
    let info = adapter.get_info();
    info!(
      name = %info.name,
      backend = ?info.device_type,
      driver = %info.driver,
      "GPU compositor adapter"
    );

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
      label: Some("dawn"),
      required_features: wgpu::Features::empty(),
      required_limits: wgpu::Limits::default(),
      ..wgpu::DeviceDescriptor::default()
    }))
    .map_err(|error| format!("cannot create a GPU device: {error}"))?;
    device.on_uncaptured_error(Arc::new(|error| {
      tracing::error!(%error, "GPU validation error");
    }));

    let max_dimension = device.limits().max_texture_dimension_2d;
    if size.0 > max_dimension || size.1 > max_dimension {
      return Err(format!(
        "window {}x{} exceeds the GPU texture limit of {max_dimension}",
        size.0, size.1
      ));
    }

    let caps = surface.get_capabilities(&adapter);
    let format = choose_format(&caps.formats)
      .ok_or_else(|| format!("GPU surface has no non-sRGB format: {:?}", caps.formats))?;
    let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
      wgpu::CompositeAlphaMode::Opaque
    } else {
      *caps
        .alpha_modes
        .first()
        .unwrap_or(&wgpu::CompositeAlphaMode::Auto)
    };
    let config = wgpu::SurfaceConfiguration {
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
      format,
      color_space: wgpu::SurfaceColorSpace::Auto,
      width: size.0,
      height: size.1,
      present_mode: wgpu::PresentMode::Fifo,
      desired_maximum_frame_latency: 2,
      alpha_mode,
      view_formats: Vec::new(),
    };
    surface.configure(&device, &config);

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("dawn-composite-shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("gpu.wgsl").into()),
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("dawn-composite-layout"),
      entries: &[
        texture_entry(0),
        texture_entry(1),
        texture_entry(2),
        wgpu::BindGroupLayoutEntry {
          binding: 3,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 4,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
          },
          count: None,
        },
      ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("dawn-composite-pipeline-layout"),
      bind_group_layouts: &[Some(&bind_group_layout)],
      immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("dawn-composite-pipeline"),
      layout: Some(&pipeline_layout),
      vertex: wgpu::VertexState {
        module: &shader,
        entry_point: Some("vs_main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        buffers: &[],
      },
      primitive: wgpu::PrimitiveState::default(),
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      fragment: Some(wgpu::FragmentState {
        module: &shader,
        entry_point: Some("fs_main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        targets: &[Some(wgpu::ColorTargetState {
          format,
          blend: None,
          write_mask: wgpu::ColorWrites::ALL,
        })],
      }),
      multiview_mask: None,
      cache: None,
    });

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("dawn-composite-sampler"),
      address_mode_u: wgpu::AddressMode::ClampToEdge,
      address_mode_v: wgpu::AddressMode::ClampToEdge,
      address_mode_w: wgpu::AddressMode::ClampToEdge,
      mag_filter: wgpu::FilterMode::Linear,
      min_filter: wgpu::FilterMode::Linear,
      mipmap_filter: wgpu::MipmapFilterMode::Nearest,
      ..wgpu::SamplerDescriptor::default()
    });
    let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("dawn-composite-globals"),
      size: std::mem::size_of::<Globals>() as u64,
      usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    let background = TextureSlot::new(&device, size, "dawn-background");
    let previous = TextureSlot::new(&device, size, "dawn-previous");
    let ui = TextureSlot::new(&device, size, "dawn-ui");
    let globals = Globals {
      top: [0.0, 0.0, 0.0, 1.0],
      bottom: [0.0, 0.0, 0.0, 1.0],
      screen: [size.0 as f32, size.1 as f32],
      bg_size: [size.0 as f32, size.1 as f32],
      prev_size: [size.0 as f32, size.1 as f32],
      opacity: 1.0,
      mode: MODE_SOLID,
      has_prev: 0,
      _pad: 0,
      _tail: [0.0; 2],
    };
    let bind_group = make_bind_group(
      &device,
      &bind_group_layout,
      &sampler,
      &globals_buffer,
      &background,
      &previous,
      &ui,
    );

    debug!(
      format = ?format,
      alpha = ?alpha_mode,
      width = size.0,
      height = size.1,
      "GPU compositor ready"
    );

    Ok(Self {
      surface,
      _instance: instance,
      device,
      queue,
      config,
      pipeline,
      bind_group_layout,
      sampler,
      globals_buffer,
      globals,
      bind_group,
      background,
      previous,
      ui,
      bg_key: None,
      prev_key: None,
      ui_uploaded: false,
      max_dimension,
      reconfigure: false,
    })
  }

  /// Resizes the swapchain and, when the size actually changed, the UI
  /// texture. Background textures keep their old size; the shader cover-crops
  /// them until the background loader catches up with the new window size.
  ///
  /// Returns true when the caller must re-rasterise and re-upload the UI.
  pub fn resize(&mut self, size: (u32, u32)) -> Result<bool, String> {
    let size = (size.0.max(1), size.1.max(1));
    if size.0 > self.max_dimension || size.1 > self.max_dimension {
      return Err(format!(
        "window {}x{} exceeds the GPU texture limit of {}",
        size.0, size.1, self.max_dimension
      ));
    }
    let changed = size != (self.config.width, self.config.height);
    if !changed && !self.reconfigure {
      return Ok(false);
    }
    self.config.width = size.0;
    self.config.height = size.1;
    self.surface.configure(&self.device, &self.config);
    self.reconfigure = false;
    if changed {
      self.ui = TextureSlot::new(&self.device, size, "dawn-ui");
      self.globals.screen = [size.0 as f32, size.1 as f32];
      self.ui_uploaded = false;
      self.rebuild_bind_group();
    }
    Ok(changed)
  }

  /// Uploads the full-screen transparent UI overlay. This is only called when
  /// the UI changed, not on wallpaper frames.
  pub fn set_ui(&mut self, pixmap: &Pixmap) -> Result<(), String> {
    let size = (pixmap.width(), pixmap.height());
    if size.0 == 0 || size.1 == 0 {
      return Ok(());
    }
    if size != self.ui.size {
      self.ui = TextureSlot::new(&self.device, size, "dawn-ui");
      self.rebuild_bind_group();
    }
    self.upload(&self.ui.texture, pixmap);
    self.ui_uploaded = true;
    Ok(())
  }

  /// Updates the wallpaper uniforms and, when the frame actually changed,
  /// uploads the new current and/or previous frame.
  pub fn set_background(&mut self, snapshot: &BackgroundSnapshot) -> Result<(), String> {
    let mut mode = MODE_IMAGE;
    let mut top = [0.0, 0.0, 0.0, 1.0];
    let mut bottom = [0.0, 0.0, 0.0, 1.0];
    let mut has_prev = 0;

    match snapshot.kind {
      SnapshotKind::Solid(color) => {
        mode = MODE_SOLID;
        top = color_array(color);
      }
      SnapshotKind::Gradient(color) => {
        mode = MODE_GRADIENT;
        top = color_array(color);
        bottom = color_array(shade(color, 0.72));
      }
      SnapshotKind::Image => {
        let Some(current) = &snapshot.current else {
          // Defensive: an image snapshot without a frame means "nothing has
          // arrived yet", which is the placeholder's job.
          mode = MODE_SOLID;
          top = [0.0, 0.0, 0.0, 1.0];
          self.globals = Globals {
            top,
            bottom,
            mode,
            has_prev: 0,
            ..self.globals
          };
          return Ok(());
        };
        let key = FrameKey::of(snapshot.frame_id, current);
        if self.bg_key != Some(key) || self.background.size != pixmap_size(current) {
          self.ensure_background_size(pixmap_size(current));
          self.upload(&self.background.texture, current);
          self.bg_key = Some(key);
        }
        self.globals.bg_size = [current.width() as f32, current.height() as f32];

        if let Some(previous) = &snapshot.previous {
          let key = FrameKey::pointer(previous);
          if self.prev_key != Some(key) || self.previous.size != pixmap_size(previous) {
            self.ensure_previous_size(pixmap_size(previous));
            self.upload(&self.previous.texture, previous);
            self.prev_key = Some(key);
          }
          self.globals.prev_size = [previous.width() as f32, previous.height() as f32];
          has_prev = 1;
        }
      }
    }

    self.globals.top = top;
    self.globals.bottom = bottom;
    self.globals.mode = mode;
    self.globals.has_prev = has_prev;
    self.globals.opacity = snapshot.opacity.clamp(0.0, 1.0);
    if has_prev == 0 {
      // Drop the old key so a future crossfade that happens to reuse an
      // allocation address still gets a fresh upload.
      self.prev_key = None;
    }
    Ok(())
  }

  /// Writes the 80-byte uniform, draws the full-screen triangle, and presents.
  /// Timeout and occlusion are normal on a compositor; they skip the frame.
  pub fn present(&mut self) -> Result<(), String> {
    if self.reconfigure {
      self.surface.configure(&self.device, &self.config);
      self.reconfigure = false;
    }
    if !self.ui_uploaded {
      return Ok(());
    }
    self
      .queue
      .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&self.globals));

    let Some(frame) = self.acquire()? else {
      return Ok(());
    };
    let view = frame
      .texture
      .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = self
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("dawn-composite"),
      });
    {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("dawn-composite-pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      pass.set_pipeline(&self.pipeline);
      pass.set_bind_group(0, &self.bind_group, &[]);
      pass.draw(0..3, 0..1);
    }
    self.queue.submit(Some(encoder.finish()));
    self.queue.present(frame);
    Ok(())
  }

  /// Acquires one swapchain texture, recovering once from a stale surface.
  /// Timeout and occlusion are normal on a compositor and simply skip a frame.
  fn acquire(&mut self) -> Result<Option<wgpu::SurfaceTexture>, String> {
    match self.surface.get_current_texture() {
      wgpu::CurrentSurfaceTexture::Success(frame) => Ok(Some(frame)),
      wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
        self.reconfigure = true;
        Ok(Some(frame))
      }
      wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => Ok(None),
      wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
        // The compositor resized or recreated the surface. Reconfigure and
        // retry once instead of tearing down a working login form.
        self.surface.configure(&self.device, &self.config);
        self.reconfigure = false;
        match self.surface.get_current_texture() {
          wgpu::CurrentSurfaceTexture::Success(frame)
          | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(Some(frame)),
          wgpu::CurrentSurfaceTexture::Validation => {
            Err("GPU surface validation failed after reconfigure".to_string())
          }
          wgpu::CurrentSurfaceTexture::Timeout
          | wgpu::CurrentSurfaceTexture::Occluded
          | wgpu::CurrentSurfaceTexture::Outdated
          | wgpu::CurrentSurfaceTexture::Lost => Ok(None),
        }
      }
      wgpu::CurrentSurfaceTexture::Validation => Err("GPU surface validation failed".to_string()),
    }
  }

  fn upload(&self, texture: &wgpu::Texture, pixmap: &Pixmap) {
    let size = (pixmap.width(), pixmap.height());
    self.queue.write_texture(
      wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      pixmap.data(),
      wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(size.0 * 4),
        rows_per_image: Some(size.1),
      },
      wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
      },
    );
  }

  fn ensure_background_size(&mut self, size: (u32, u32)) {
    if self.background.size == size {
      return;
    }
    self.background = TextureSlot::new(&self.device, size, "dawn-background");
    self.bg_key = None;
    self.rebuild_bind_group();
  }

  fn ensure_previous_size(&mut self, size: (u32, u32)) {
    if self.previous.size == size {
      return;
    }
    self.previous = TextureSlot::new(&self.device, size, "dawn-previous");
    self.prev_key = None;
    self.rebuild_bind_group();
  }

  fn rebuild_bind_group(&mut self) {
    self.bind_group = make_bind_group(
      &self.device,
      &self.bind_group_layout,
      &self.sampler,
      &self.globals_buffer,
      &self.background,
      &self.previous,
      &self.ui,
    );
  }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
  wgpu::BindGroupLayoutEntry {
    binding,
    visibility: wgpu::ShaderStages::FRAGMENT,
    ty: wgpu::BindingType::Texture {
      sample_type: wgpu::TextureSampleType::Float { filterable: true },
      view_dimension: wgpu::TextureViewDimension::D2,
      multisampled: false,
    },
    count: None,
  }
}

fn make_bind_group(
  device: &wgpu::Device,
  layout: &wgpu::BindGroupLayout,
  sampler: &wgpu::Sampler,
  globals: &wgpu::Buffer,
  background: &TextureSlot,
  previous: &TextureSlot,
  ui: &TextureSlot,
) -> wgpu::BindGroup {
  device.create_bind_group(&wgpu::BindGroupDescriptor {
    label: Some("dawn-composite-bind-group"),
    layout,
    entries: &[
      wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::TextureView(&background.view),
      },
      wgpu::BindGroupEntry {
        binding: 1,
        resource: wgpu::BindingResource::TextureView(&previous.view),
      },
      wgpu::BindGroupEntry {
        binding: 2,
        resource: wgpu::BindingResource::TextureView(&ui.view),
      },
      wgpu::BindGroupEntry {
        binding: 3,
        resource: wgpu::BindingResource::Sampler(sampler),
      },
      wgpu::BindGroupEntry {
        binding: 4,
        resource: globals.as_entire_binding(),
      },
    ],
  })
}

/// Chooses a standard 8-bit non-sRGB format. The whole pipeline assumes the
/// swapchain stores the exact sRGB bytes that tiny-skia produced; formats such
/// as `Rgba16Float` are usually HDR surfaces and would change the interpretation
/// of every value. Returns `None` when no 8-bit non-sRGB format is offered and
/// the caller should use the CPU path instead.
fn choose_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
  [
    wgpu::TextureFormat::Bgra8Unorm,
    wgpu::TextureFormat::Rgba8Unorm,
  ]
  .into_iter()
  .find(|format| formats.contains(format))
}

fn pixmap_size(pixmap: &Pixmap) -> (u32, u32) {
  (pixmap.width(), pixmap.height())
}

fn color_array(color: Rgb) -> [f32; 4] {
  [
    f32::from(color.r) / 255.0,
    f32::from(color.g) / 255.0,
    f32::from(color.b) / 255.0,
    1.0,
  ]
}

/// The CPU background gradient darkens the configured colour by 0.72. Keep the
/// same constant here so a GPU solid/gradient frame is indistinguishable.
fn shade(color: Rgb, factor: f32) -> Rgb {
  let channel = |value: u8| (f32::from(value) * factor).round().clamp(0.0, 255.0) as u8;
  Rgb::solid(channel(color.r), channel(color.g), channel(color.b))
}

#[cfg(test)]
mod tests {
  use super::{Globals, MODE_IMAGE, choose_format};
  use wgpu::TextureFormat;

  #[test]
  fn uniform_layout_matches_the_shader_size() {
    // WGSL rounds the struct up to 16 bytes; the explicit tail keeps the Rust
    // side at the same size so the uniform binding cannot be short.
    assert_eq!(std::mem::size_of::<Globals>(), 80);
    assert_eq!(std::mem::align_of::<Globals>(), 4);
    assert_eq!(MODE_IMAGE, 0);
  }

  #[test]
  fn non_srgb_surfaces_win_over_srgb() {
    let formats = [
      TextureFormat::Rgba16Float,
      TextureFormat::Bgra8UnormSrgb,
      TextureFormat::Rgba8UnormSrgb,
      TextureFormat::Bgra8Unorm,
    ];
    assert_eq!(choose_format(&formats), Some(TextureFormat::Bgra8Unorm));
    assert_eq!(
      choose_format(&[TextureFormat::Bgra8UnormSrgb, TextureFormat::Rgba16Float]),
      None,
      "an sRGB-only or HDR-only surface must fall back to the CPU renderer"
    );
  }
}
