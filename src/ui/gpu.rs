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
  bg_key: Option<FrameKey>,
  prev_key: Option<FrameKey>,
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

impl GpuRenderer {
  /// Builds the compositor for one window. The caller owns the fallback: an
  /// `Err` here means "try the next policy", never "the greeter cannot start".
  ///
  /// Adapter selection is deliberately separate from device creation. A driver
  /// can advertise a surface and still fail when asked for a device, so the
  /// chosen adapter is only the first candidate; the loop keeps walking the
  /// ranked list until one produces both a device and a usable format.
  pub fn new(
    window: Arc<Window>,
    display: OwnedDisplayHandle,
    size: (u32, u32),
    gpu: &GpuConfig,
  ) -> Result<Self, String> {
    let size = (size.0.max(1), size.1.max(1));
    let backends = backend_mask(gpu);

    let mut descriptor = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display));
    descriptor.backends = backends;
    let instance = wgpu::Instance::new(descriptor);

    let surface = instance
      .create_surface(window)
      .map_err(|error| format!("cannot create a GPU surface: {error}"))?;
    let all = pollster::block_on(instance.enumerate_adapters(backends));
    let device_class = effective_device(gpu);
    let supported: Vec<wgpu::Adapter> = all
      .into_iter()
      .filter(|adapter| {
        adapter.is_surface_supported(&surface)
          && device_matches(device_class, adapter.get_info().device_type)
      })
      .collect();
    let candidates = order_candidates(&supported, gpu)?;

    let mut failure = "no adapter could create a device and surface".to_string();
    let mut chosen = None;
    for adapter in candidates {
      let info = adapter.get_info();
      let device_result = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("dawn"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        ..wgpu::DeviceDescriptor::default()
      }));
      let (device, queue) = match device_result {
        Ok(pair) => pair,
        Err(error) => {
          failure = format!("{}: cannot create a GPU device: {error}", info.name);
          continue;
        }
      };
      let caps = surface.get_capabilities(&adapter);
      let Some(format) = choose_format(&caps.formats) else {
        failure = format!(
          "{}: no non-sRGB surface format ({:?})",
          info.name, caps.formats
        );
        continue;
      };
      let present_mode = choose_present(gpu.present, &caps.present_modes);
      chosen = Some((device, queue, info, format, caps.alpha_modes, present_mode));
      break;
    }

    let Some((device, queue, info, format, alpha_modes, present_mode)) = chosen else {
      return Err(format!("no usable GPU adapter: {failure}"));
    };

    info!(
      name = %info.name,
      backend = %info.backend,
      device = ?info.device_type,
      driver = %info.driver,
      present = ?present_mode,
      "GPU compositor adapter"
    );
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

    let alpha_mode = if alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
      wgpu::CompositeAlphaMode::Opaque
    } else {
      *alpha_modes
        .first()
        .unwrap_or(&wgpu::CompositeAlphaMode::Auto)
    };
    let config = wgpu::SurfaceConfiguration {
      usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
      format,
      color_space: wgpu::SurfaceColorSpace::Auto,
      width: size.0,
      height: size.1,
      present_mode,
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

    // The blur is a second, independent pipeline. It always renders into
    // Rgba8Unorm regardless of the surface format, because its output is read
    // back into a tiny-skia pixmap rather than presented.
    let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("dawn-blur-shader"),
      source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()),
    });
    let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
      label: Some("dawn-blur-layout"),
      entries: &[
        texture_entry(0),
        wgpu::BindGroupLayoutEntry {
          binding: 1,
          visibility: wgpu::ShaderStages::FRAGMENT,
          ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
          count: None,
        },
        wgpu::BindGroupLayoutEntry {
          binding: 2,
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
    let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
      label: Some("dawn-blur-pipeline-layout"),
      bind_group_layouts: &[Some(&blur_layout)],
      immediate_size: 0,
    });
    let blur_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
      label: Some("dawn-blur-pipeline"),
      layout: Some(&blur_pipeline_layout),
      vertex: wgpu::VertexState {
        module: &blur_shader,
        entry_point: Some("vs_main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        buffers: &[],
      },
      primitive: wgpu::PrimitiveState::default(),
      depth_stencil: None,
      multisample: wgpu::MultisampleState::default(),
      fragment: Some(wgpu::FragmentState {
        module: &blur_shader,
        entry_point: Some("fs_main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        targets: &[Some(wgpu::ColorTargetState {
          format: wgpu::TextureFormat::Rgba8Unorm,
          blend: None,
          write_mask: wgpu::ColorWrites::ALL,
        })],
      }),
      multiview_mask: None,
      cache: None,
    });
    let blur_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
      label: Some("dawn-blur-sampler"),
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
      blur_pipeline,
      blur_layout,
      blur_sampler,
      blur_targets: None,
      blur_failed_logged: false,
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

  /// Renders a full-resolution separable Gaussian on the GPU and reads the
  /// result back into a pixmap.
  ///
  /// The whole colour pipeline lives in `blur.wgsl`: the horizontal pass does
  /// Hyprland's contrast/brightness prepare and vibrancy, the vertical pass adds
  /// grain and the final brightness. The CPU only uploads the raw source and
  /// copies the finished rows back, which is what keeps a 1080p refresh fast.
  /// The CPU pyramid remains the fallback and the `--screenshot` path.
  ///
  /// The first call for a given size builds the texture chain. Every later
  /// call with the same size reuses it. Any failure is returned to the caller
  /// once and logged once; the glass cache then falls back to the CPU path.
  pub fn blur(&mut self, source: &Pixmap, blur: &BlurConfig, scale: f32) -> Result<Pixmap, String> {
    let result = self.blur_inner(source, blur, scale);
    match &result {
      Ok(_) => self.blur_failed_logged = false,
      Err(error) => {
        if !self.blur_failed_logged {
          warn!(%error, "GPU backdrop blur failed; using the CPU blur");
          self.blur_failed_logged = true;
        }
      }
    }
    result
  }

  fn blur_inner(
    &mut self,
    source: &Pixmap,
    blur: &BlurConfig,
    scale: f32,
  ) -> Result<Pixmap, String> {
    let size = (source.width(), source.height());
    if size.0 == 0 || size.1 == 0 {
      return Err("cannot blur an empty pixmap".to_string());
    }
    if size.0 > self.max_dimension || size.1 > self.max_dimension {
      return Err(format!(
        "blur source {}x{} exceeds the GPU texture limit of {}",
        size.0, size.1, self.max_dimension
      ));
    }
    if self
      .blur_targets
      .as_ref()
      .is_none_or(|targets| targets.size != size)
    {
      self.create_blur_targets(size)?;
    }
    let started = Instant::now();

    // Upload the raw source. Every colour operation after this point runs in
    // the shader, so the CPU never touches the 1080p buffer.
    let Some(targets) = self.blur_targets.as_ref() else {
      return Err("blur targets disappeared after creation".to_string());
    };
    self.upload(&targets.input.texture, source);

    let radius = (blur.size.max(0.0) * scale).max(0.0);
    let contrast = blur.contrast.max(0.0);
    let color = [
      blur.vibrancy,
      blur.vibrancy_darkness,
      blur.noise,
      blur.brightness,
    ];
    let passes = blur.passes as f32;
    self.queue.write_buffer(
      &targets.horizontal_params,
      0,
      bytemuck::bytes_of(&BlurParams {
        dir_radius: [1.0, 0.0, radius, contrast],
        color,
        flags: [0.0, passes, 0.0, 0.0],
      }),
    );
    self.queue.write_buffer(
      &targets.vertical_params,
      0,
      bytemuck::bytes_of(&BlurParams {
        dir_radius: [0.0, 1.0, radius, contrast],
        color,
        flags: [1.0, passes, 0.0, 0.0],
      }),
    );

    let mut encoder = self
      .device
      .create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("dawn-blur"),
      });
    // Horizontal pass: input -> temp.
    {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("dawn-blur-horizontal"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &targets.temp.view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      pass.set_pipeline(&self.blur_pipeline);
      pass.set_bind_group(0, &targets.horizontal, &[]);
      pass.draw(0..3, 0..1);
    }
    // Vertical pass: temp -> output.
    {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("dawn-blur-vertical"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: &targets.output.view,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      pass.set_pipeline(&self.blur_pipeline);
      pass.set_bind_group(0, &targets.vertical, &[]);
      pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
      wgpu::TexelCopyTextureInfo {
        texture: &targets.output.texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
      },
      wgpu::TexelCopyBufferInfo {
        buffer: &targets.staging,
        layout: wgpu::TexelCopyBufferLayout {
          offset: 0,
          bytes_per_row: Some(targets.padded_row),
          rows_per_image: Some(size.1),
        },
      },
      wgpu::Extent3d {
        width: size.0,
        height: size.1,
        depth_or_array_layers: 1,
      },
    );
    self.queue.submit(Some(encoder.finish()));

    targets
      .staging
      .slice(..)
      .map_async(wgpu::MapMode::Read, |_| {});
    if let Err(error) = self.device.poll(wgpu::PollType::Wait {
      submission_index: None,
      timeout: None,
    }) {
      // Cancel the pending map so the next attempt starts from a clean buffer.
      targets.staging.unmap();
      return Err(format!("GPU blur readback poll failed: {error}"));
    }

    let readback = (|| -> Result<Pixmap, String> {
      let mapped = targets
        .staging
        .slice(..)
        .get_mapped_range()
        .map_err(|error| format!("GPU blur readback did not map: {error}"))?;
      let mut output = Pixmap::new(size.0, size.1)
        .ok_or_else(|| "cannot allocate the blur output pixmap".to_string())?;
      let row_bytes = (size.0 * 4) as usize;
      let padded = targets.padded_row as usize;
      for y in 0..size.1 as usize {
        let source_start = y * padded;
        let source_row = &mapped[source_start..source_start + row_bytes];
        let target_start = y * row_bytes;
        output.data_mut()[target_start..target_start + row_bytes].copy_from_slice(source_row);
      }
      Ok(output)
    })();
    // The buffer must be unmapped before it can be a copy destination again,
    // whether or not the row copy above succeeded.
    targets.staging.unmap();
    let output = readback?;

    debug!(
      elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
      width = size.0,
      height = size.1,
      "GPU backdrop blur"
    );
    Ok(output)
  }

  fn create_blur_targets(&mut self, size: (u32, u32)) -> Result<(), String> {
    let input = TextureSlot::with_usage(
      &self.device,
      size,
      "dawn-blur-input",
      wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    );
    let temp = TextureSlot::with_usage(
      &self.device,
      size,
      "dawn-blur-temp",
      wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    );
    let output = TextureSlot::with_usage(
      &self.device,
      size,
      "dawn-blur-output",
      wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_SRC,
    );
    let padded_row = padded_row(size.0);
    let staging_size = u64::from(padded_row) * u64::from(size.1);
    let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
      label: Some("dawn-blur-readback"),
      size: staging_size,
      usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
      mapped_at_creation: false,
    });
    let uniform = |label: &str| {
      self.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: std::mem::size_of::<BlurParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
      })
    };
    let horizontal_params = uniform("dawn-blur-horizontal-params");
    let vertical_params = uniform("dawn-blur-vertical-params");
    let horizontal = make_blur_bind_group(
      &self.device,
      &self.blur_layout,
      &self.blur_sampler,
      &input,
      &horizontal_params,
    );
    let vertical = make_blur_bind_group(
      &self.device,
      &self.blur_layout,
      &self.blur_sampler,
      &temp,
      &vertical_params,
    );
    self.blur_targets = Some(BlurTargets {
      size,
      input,
      temp,
      output,
      staging,
      padded_row,
      horizontal_params,
      vertical_params,
      horizontal,
      vertical,
    });
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

/// Lists the adapters the configured policy can see, without a window.
///
/// This is the `--list-gpus` half of the selection logic. There is no surface,
/// so we cannot know which adapters can present; the footer says so, and the
/// real window still tries the chosen adapter and walks the ranked list when a
/// driver cannot create a device. Keeping the backend, device, power, and spec
/// helpers shared is what makes the index in this table match `adapter =
/// "index:N"`.
pub fn list_adapters(gpu: &GpuConfig) -> Result<String, String> {
  let backends = backend_mask(gpu);
  let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
  descriptor.backends = backends;
  let instance = wgpu::Instance::new(descriptor);
  let all = pollster::block_on(instance.enumerate_adapters(backends));
  let device_class = effective_device(gpu);
  let filtered: Vec<wgpu::Adapter> = all
    .into_iter()
    .filter(|adapter| device_matches(device_class, adapter.get_info().device_type))
    .collect();

  let selected = order_candidates(&filtered, gpu)
    .ok()
    .and_then(|ordered| ordered.first().cloned());

  let mut out = format!("dawn {} GPUs\n", env!("CARGO_PKG_VERSION"));
  if filtered.is_empty() {
    out.push_str("no adapters found\n");
  } else {
    for (index, adapter) in filtered.iter().enumerate() {
      let info = adapter.get_info();
      let marker = if selected.as_ref() == Some(adapter) {
        " * selected"
      } else {
        ""
      };
      out.push_str(&format!(
        "  [{index}] {} | backend={} device={} driver={}{marker}\n",
        info.name,
        info.backend,
        device_label(info.device_type),
        info.driver
      ));
    }
    if selected.is_none() {
      out.push_str(&format!(
        "no adapter matches {:?}; available adapters: {}\n",
        gpu.adapter,
        adapter_list(&filtered)
      ));
    }
  }
  out.push_str(
    "note: surface support cannot be checked without a window, so the window may skip an adapter\n",
  );
  out.push_str(&format!("gpu: {}\n", gpu.summary()));
  Ok(out)
}

/// Maps the configured backend to the wgpu mask used by both the instance and
/// adapter enumeration. Software stays on Vulkan and GL because lavapipe and
/// llvmpipe live behind different APIs; the device filter below narrows the
/// result to CPU adapters.
fn backend_mask(gpu: &GpuConfig) -> wgpu::Backends {
  match gpu.backend {
    GpuBackend::Auto | GpuBackend::Software => wgpu::Backends::VULKAN | wgpu::Backends::GL,
    GpuBackend::Vulkan => wgpu::Backends::VULKAN,
    GpuBackend::Gl => wgpu::Backends::GL,
  }
}

/// `backend = "software"` is a stronger statement than `device = "cpu"`: it
/// means "only a CPU adapter is acceptable", so it overrides whatever device
/// value the same table contains. The `adapter` keywords do the same thing for
/// the common cases, which lets most configs name a class instead of combining
/// a device filter with a power ranking.
fn effective_device(gpu: &GpuConfig) -> GpuDevice {
  match adapter_choice(&gpu.adapter) {
    AdapterChoice::Integrated => GpuDevice::Integrated,
    AdapterChoice::Discrete => GpuDevice::Discrete,
    AdapterChoice::Software => GpuDevice::Cpu,
    AdapterChoice::Auto if gpu.backend == GpuBackend::Software => GpuDevice::Cpu,
    AdapterChoice::Auto | AdapterChoice::Other => gpu.device,
  }
}

/// Power ranking implied by an adapter keyword. `device` and `power` remain for
/// callers that need a filter or an order the keywords cannot express.
fn effective_power(gpu: &GpuConfig) -> GpuPower {
  match adapter_choice(&gpu.adapter) {
    AdapterChoice::Integrated => GpuPower::Low,
    AdapterChoice::Discrete => GpuPower::High,
    AdapterChoice::Auto | AdapterChoice::Software | AdapterChoice::Other => gpu.power,
  }
}

/// The friendly spellings accepted by `[gpu] adapter` before it is treated as
/// an index or a name substring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdapterChoice {
  Auto,
  Integrated,
  Discrete,
  Software,
  /// `index:N` or a name/driver substring.
  Other,
}

fn adapter_choice(adapter: &str) -> AdapterChoice {
  match adapter.trim().to_ascii_lowercase().as_str() {
    "" | "auto" => AdapterChoice::Auto,
    "integrated" | "igpu" | "integrated-gpu" => AdapterChoice::Integrated,
    "discrete" | "dgpu" | "discrete-gpu" => AdapterChoice::Discrete,
    "software" | "cpu" => AdapterChoice::Software,
    _ => AdapterChoice::Other,
  }
}

fn device_matches(wanted: GpuDevice, actual: wgpu::DeviceType) -> bool {
  match wanted {
    GpuDevice::Auto => true,
    GpuDevice::Discrete => actual == wgpu::DeviceType::DiscreteGpu,
    GpuDevice::Integrated => actual == wgpu::DeviceType::IntegratedGpu,
    GpuDevice::Virtual => actual == wgpu::DeviceType::VirtualGpu,
    GpuDevice::Cpu => actual == wgpu::DeviceType::Cpu,
  }
}

/// Stable order for the fallback walk. `Default` keeps enumeration order, which
/// is the only way to see exactly what the backend reported.
fn rank_adapters(adapters: &[wgpu::Adapter], power: GpuPower) -> Vec<wgpu::Adapter> {
  let priority = |device: wgpu::DeviceType| -> u8 {
    match power {
      GpuPower::Low => match device {
        wgpu::DeviceType::IntegratedGpu => 0,
        wgpu::DeviceType::Other => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::DiscreteGpu => 3,
        wgpu::DeviceType::Cpu => 4,
      },
      GpuPower::High => match device {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Other => 3,
        wgpu::DeviceType::Cpu => 4,
      },
      GpuPower::Default => 0,
    }
  };
  let mut ranked = adapters.to_vec();
  if power != GpuPower::Default {
    // Stable sort, so equal classes keep the backend's enumeration order.
    ranked.sort_by_key(|adapter| priority(adapter.get_info().device_type));
  }
  ranked
}

/// Turns a filtered adapter list into the order `GpuRenderer::new` should try.
///
/// `auto` takes the first ranked adapter. `index:N` and name matching read the
/// unranked list, so the index printed by `--list-gpus` is stable regardless of
/// the power preference. The chosen adapter is always first, followed by the
/// rest of the ranking, which is what lets one broken driver fall through
/// instead of killing the compositor.
fn order_candidates(
  adapters: &[wgpu::Adapter],
  gpu: &GpuConfig,
) -> Result<Vec<wgpu::Adapter>, String> {
  if adapters.is_empty() {
    return Err(
      "no GPU adapters matched the configured backend and device; run `dawn --list-gpus`"
        .to_string(),
    );
  }
  let ranked = rank_adapters(adapters, effective_power(gpu));
  let chosen = if matches!(
    adapter_choice(&gpu.adapter),
    AdapterChoice::Auto
      | AdapterChoice::Integrated
      | AdapterChoice::Discrete
      | AdapterChoice::Software
  ) {
    ranked.first().cloned()
  } else {
    adapter_position(adapters, &gpu.adapter).and_then(|index| adapters.get(index).cloned())
  };
  let Some(chosen) = chosen else {
    return Err(format!(
      "no adapter matches {:?}; available adapters: {}",
      gpu.adapter,
      adapter_list(adapters)
    ));
  };
  let mut ordered = Vec::with_capacity(adapters.len());
  ordered.push(chosen.clone());
  for adapter in ranked {
    if adapter != chosen {
      ordered.push(adapter);
    }
  }
  Ok(ordered)
}

/// Resolves the `adapter` spec against the filtered list before ranking.
/// `None` covers both "not an index" and "out of range"; the caller turns that
/// into a readable error listing the names.
fn adapter_position(adapters: &[wgpu::Adapter], spec: &str) -> Option<usize> {
  if let Some(rest) = spec.strip_prefix("index:") {
    return rest
      .trim()
      .parse::<usize>()
      .ok()
      .filter(|index| *index < adapters.len());
  }
  let needle = spec.to_ascii_lowercase();
  if needle.is_empty() {
    return None;
  }
  adapters.iter().position(|adapter| {
    let info = adapter.get_info();
    info.name.to_ascii_lowercase().contains(&needle)
      || info.driver.to_ascii_lowercase().contains(&needle)
  })
}

fn adapter_list(adapters: &[wgpu::Adapter]) -> String {
  if adapters.is_empty() {
    return "none".to_string();
  }
  adapters
    .iter()
    .map(|adapter| adapter.get_info().name)
    .collect::<Vec<_>>()
    .join(", ")
}

/// Present modes are a preference, never a hard requirement. Fifo is the one
/// mode every backend must provide, so an unavailable request degrades to it
/// (or to whatever the surface does offer) with a warning.
fn choose_present(wanted: GpuPresent, available: &[wgpu::PresentMode]) -> wgpu::PresentMode {
  let desired = match wanted {
    GpuPresent::Fifo => wgpu::PresentMode::Fifo,
    GpuPresent::Mailbox => wgpu::PresentMode::Mailbox,
    GpuPresent::Immediate => wgpu::PresentMode::Immediate,
  };
  if available.contains(&desired) {
    return desired;
  }
  let fallback = if available.contains(&wgpu::PresentMode::Fifo) {
    wgpu::PresentMode::Fifo
  } else {
    available
      .first()
      .copied()
      .unwrap_or(wgpu::PresentMode::Fifo)
  };
  warn!(
    requested = ?desired,
    using = ?fallback,
    "GPU present mode is not supported by this surface"
  );
  fallback
}

fn device_label(device: wgpu::DeviceType) -> &'static str {
  match device {
    wgpu::DeviceType::Other => "other",
    wgpu::DeviceType::IntegratedGpu => "integrated",
    wgpu::DeviceType::DiscreteGpu => "discrete",
    wgpu::DeviceType::VirtualGpu => "virtual",
    wgpu::DeviceType::Cpu => "cpu",
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

fn make_blur_bind_group(
  device: &wgpu::Device,
  layout: &wgpu::BindGroupLayout,
  sampler: &wgpu::Sampler,
  source: &TextureSlot,
  params: &wgpu::Buffer,
) -> wgpu::BindGroup {
  device.create_bind_group(&wgpu::BindGroupDescriptor {
    label: Some("dawn-blur-bind-group"),
    layout,
    entries: &[
      wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::TextureView(&source.view),
      },
      wgpu::BindGroupEntry {
        binding: 1,
        resource: wgpu::BindingResource::Sampler(sampler),
      },
      wgpu::BindGroupEntry {
        binding: 2,
        resource: params.as_entire_binding(),
      },
    ],
  })
}

/// Rounds a row of RGBA pixels up to the 256-byte alignment required by
/// `copy_texture_to_buffer`. Without this, a width whose row is not a multiple
/// of 256 is a validation error on every backend, not just a slow path.
fn padded_row(width: u32) -> u32 {
  let row = width.saturating_mul(4);
  let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
  row.saturating_add(align - 1) & !(align - 1)
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
  use super::{
    AdapterChoice, BlurParams, Globals, MODE_IMAGE, adapter_choice, choose_format,
    effective_device, effective_power, padded_row,
  };
  use crate::config::{GpuBackend, GpuConfig, GpuDevice, GpuPower};
  use wgpu::TextureFormat;

  #[test]
  fn adapter_keywords_pick_a_class_and_a_ranking() {
    let discrete = GpuConfig {
      adapter: "discrete".to_string(),
      ..GpuConfig::default()
    };
    assert_eq!(adapter_choice(&discrete.adapter), AdapterChoice::Discrete);
    assert_eq!(effective_device(&discrete), GpuDevice::Discrete);
    assert_eq!(effective_power(&discrete), GpuPower::High);

    let integrated = GpuConfig {
      adapter: "integrated".to_string(),
      ..GpuConfig::default()
    };
    assert_eq!(effective_device(&integrated), GpuDevice::Integrated);
    assert_eq!(effective_power(&integrated), GpuPower::Low);

    let software = GpuConfig {
      adapter: "software".to_string(),
      ..GpuConfig::default()
    };
    assert_eq!(effective_device(&software), GpuDevice::Cpu);

    let named = GpuConfig {
      adapter: "index:1".to_string(),
      ..GpuConfig::default()
    };
    assert_eq!(adapter_choice(&named.adapter), AdapterChoice::Other);
    assert_eq!(effective_device(&named), GpuDevice::Auto);
    assert_eq!(effective_power(&named), GpuPower::Low);
  }

  #[test]
  fn backend_software_still_forces_the_cpu_with_adapter_auto() {
    let mut gpu = GpuConfig {
      backend: GpuBackend::Software,
      ..GpuConfig::default()
    };
    assert_eq!(effective_device(&gpu), GpuDevice::Cpu);
    gpu.adapter = "discrete".to_string();
    assert_eq!(
      effective_device(&gpu),
      GpuDevice::Discrete,
      "an explicit adapter keyword is the more specific request"
    );
  }

  #[test]
  fn uniform_layout_matches_the_shader_size() {
    // WGSL rounds the struct up to 16 bytes; the explicit tail keeps the Rust
    // side at the same size so the uniform binding cannot be short.
    assert_eq!(std::mem::size_of::<Globals>(), 80);
    assert_eq!(std::mem::align_of::<Globals>(), 4);
    assert_eq!(MODE_IMAGE, 0);
  }

  #[test]
  fn blur_uniform_is_three_vec4s() {
    // The shader declares three vec4<f32> fields. If this ever changes, the
    // uniform write and the WGSL struct have drifted and the colours will be
    // read from the wrong offsets.
    assert_eq!(std::mem::size_of::<BlurParams>(), 48);
    assert_eq!(std::mem::align_of::<BlurParams>(), 4);
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

  #[test]
  fn readback_rows_are_aligned_to_256_bytes() {
    // A 1920-wide RGBA row is already 7680 bytes, which is 256-aligned, while
    // the odd widths between the multiples need real padding. Getting this
    // wrong makes `copy_texture_to_buffer` a validation error on every frame.
    assert_eq!(padded_row(1), 256);
    assert_eq!(padded_row(64), 256);
    assert_eq!(padded_row(65), 512);
    assert_eq!(padded_row(1920), 7680);
    assert_eq!(padded_row(1921), 7936);
  }
}
