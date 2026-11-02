//! Adapter listing and selection helpers.

use super::*;

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
pub(super) fn backend_mask(gpu: &GpuConfig) -> wgpu::Backends {
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
pub(super) fn effective_device(gpu: &GpuConfig) -> GpuDevice {
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
pub(super) fn effective_power(gpu: &GpuConfig) -> GpuPower {
  match adapter_choice(&gpu.adapter) {
    AdapterChoice::Integrated => GpuPower::Low,
    AdapterChoice::Discrete => GpuPower::High,
    AdapterChoice::Auto | AdapterChoice::Software | AdapterChoice::Other => gpu.power,
  }
}

/// The friendly spellings accepted by `[gpu] adapter` before it is treated as
/// an index or a name substring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AdapterChoice {
  Auto,
  Integrated,
  Discrete,
  Software,
  /// `index:N` or a name/driver substring.
  Other,
}

pub(super) fn adapter_choice(adapter: &str) -> AdapterChoice {
  match adapter.trim().to_ascii_lowercase().as_str() {
    "" | "auto" => AdapterChoice::Auto,
    "integrated" | "igpu" | "integrated-gpu" => AdapterChoice::Integrated,
    "discrete" | "dgpu" | "discrete-gpu" => AdapterChoice::Discrete,
    "software" | "cpu" => AdapterChoice::Software,
    _ => AdapterChoice::Other,
  }
}

pub(super) fn device_matches(wanted: GpuDevice, actual: wgpu::DeviceType) -> bool {
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
pub(super) fn rank_adapters(adapters: &[wgpu::Adapter], power: GpuPower) -> Vec<wgpu::Adapter> {
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
pub(super) fn order_candidates(
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
pub(super) fn adapter_position(adapters: &[wgpu::Adapter], spec: &str) -> Option<usize> {
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

pub(super) fn adapter_list(adapters: &[wgpu::Adapter]) -> String {
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
pub(super) fn choose_present(
  wanted: GpuPresent,
  available: &[wgpu::PresentMode],
) -> wgpu::PresentMode {
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

pub(super) fn device_label(device: wgpu::DeviceType) -> &'static str {
  match device {
    wgpu::DeviceType::Other => "other",
    wgpu::DeviceType::IntegratedGpu => "integrated",
    wgpu::DeviceType::DiscreteGpu => "discrete",
    wgpu::DeviceType::VirtualGpu => "virtual",
    wgpu::DeviceType::Cpu => "cpu",
  }
}

pub(super) fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
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

#[allow(clippy::too_many_arguments)]
pub(super) fn make_bind_group(
  device: &wgpu::Device,
  layout: &wgpu::BindGroupLayout,
  sampler: &wgpu::Sampler,
  globals: &wgpu::Buffer,
  background: &TextureSlot,
  previous: &TextureSlot,
  ui: &TextureSlot,
  decor: &TextureSlot,
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
      wgpu::BindGroupEntry {
        binding: 5,
        resource: wgpu::BindingResource::TextureView(&decor.view),
      },
    ],
  })
}

pub(super) fn make_blur_bind_group(
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
pub(super) fn padded_row(width: u32) -> u32 {
  let row = width.saturating_mul(4);
  let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
  row.saturating_add(align - 1) & !(align - 1)
}

/// Chooses a standard 8-bit non-sRGB format. The whole pipeline assumes the
/// swapchain stores the exact sRGB bytes that tiny-skia produced; formats such
/// as `Rgba16Float` are usually HDR surfaces and would change the interpretation
/// of every value. Returns `None` when no 8-bit non-sRGB format is offered and
/// the caller should use the CPU path instead.
pub(super) fn choose_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
  [
    wgpu::TextureFormat::Bgra8Unorm,
    wgpu::TextureFormat::Rgba8Unorm,
  ]
  .into_iter()
  .find(|format| formats.contains(format))
}

pub(super) fn pixmap_size(pixmap: &Pixmap) -> (u32, u32) {
  (pixmap.width(), pixmap.height())
}

pub(super) fn color_array(color: Rgb) -> [f32; 4] {
  [
    f32::from(color.r) / 255.0,
    f32::from(color.g) / 255.0,
    f32::from(color.b) / 255.0,
    1.0,
  ]
}

/// The CPU background gradient darkens the configured colour by 0.72. Keep the
/// same constant here so a GPU solid/gradient frame is indistinguishable.
pub(super) fn shade(color: Rgb, factor: f32) -> Rgb {
  let channel = |value: u8| (f32::from(value) * factor).round().clamp(0.0, 255.0) as u8;
  Rgb::solid(channel(color.r), channel(color.g), channel(color.b))
}
