use super::{
  AdapterChoice, BlurParams, Globals, MODE_IMAGE, adapter_choice, choose_format, effective_device,
  effective_power, padded_row,
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
  // WGSL aligns every vec4 to 16 bytes; the explicit `_align` and splash
  // padding keep the Rust offsets identical to the shader's.
  assert_eq!(std::mem::size_of::<Globals>(), 160);
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
