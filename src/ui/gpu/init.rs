//! Init code for the GPU compositor.

use super::*;

impl GpuRenderer {
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
      source: wgpu::ShaderSource::Wgsl(include_str!("../gpu.wgsl").into()),
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
        texture_entry(5),
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
      source: wgpu::ShaderSource::Wgsl(include_str!("../blur.wgsl").into()),
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
    let decor = TextureSlot::new(&device, size, "dawn-decor");
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
      _align: [0.0; 2],
      camera: [1.0, 0.0, 0.0, 0.0],
      post: [0.0, 0.0, 0.0, 0.0],
      splash: [0.0; 4],
      splash_color: [0.0; 4],
      splash_on: 0,
      decor_on: 0,
      _flags_pad: [0; 2],
    };
    let bind_group = make_bind_group(
      &device,
      &bind_group_layout,
      &sampler,
      &globals_buffer,
      &background,
      &previous,
      &ui,
      &decor,
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
      decor,
      bg_key: None,
      prev_key: None,
      decor_key: None,
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
}
