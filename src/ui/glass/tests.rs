use super::{GlassCache, make_blurred};
use crate::config::{BlurConfig, Config};
use crate::ui::canvas::Canvas;
use crate::ui::theme::Theme;
use std::time::Instant;
use tiny_skia::Pixmap;

#[test]
fn blur_cache_refreshes_only_when_needed() {
  let config = Config::default();
  let theme = Theme::from(&config);
  let mut canvas = Canvas::new(320, 200).unwrap();
  canvas.clear(config.background.color);
  let mut cache = GlassCache::new();
  let now = Instant::now();
  cache.prepare(&canvas, 1, now, &theme, 1.0, false);
  assert!(cache.has_blur());
  assert_eq!(cache.cached_frame(), 1);
  cache.prepare(&canvas, 1, now, &theme, 1.0, false);
  assert_eq!(cache.cached_frame(), 1);
}

#[test]
fn minimal_preset_skips_blur() {
  let mut config = Config::default();
  config.theme.blur.enabled = false;
  let theme = Theme::from(&config);
  let mut canvas = Canvas::new(64, 64).unwrap();
  canvas.clear(config.background.color);
  let mut cache = GlassCache::new();
  cache.prepare(&canvas, 1, Instant::now(), &theme, 1.0, false);
  assert!(!cache.has_blur());
}

/// A GPU blur that returns pixels must be stored as-is, not reprocessed by
/// the CPU pyramid. The 8x8 marker size is impossible for a 32x32 canvas, so
/// the assertion can only pass if the closure result won.
#[test]
fn prepare_with_prefers_the_gpu_result() {
  let config = Config::default();
  let theme = Theme::from(&config);
  let mut canvas = Canvas::new(32, 32).unwrap();
  canvas.clear(config.background.color);
  let mut cache = GlassCache::new();
  let now = Instant::now();
  let changed = cache.prepare_with(&canvas, 7, now, &theme, 1.0, false, |_, _, _| {
    Pixmap::new(8, 8)
  });
  assert!(changed);
  assert_eq!(cache.cached_frame(), 7);
  assert_eq!(cache.blur_size(), Some((8, 8)));
}

/// When the GPU closure declines, the same call must fall through to the CPU
/// path and still cache a full-size backdrop. This is the live fallback after
/// a driver or readback failure.
#[test]
fn prepare_with_falls_back_to_the_cpu_path() {
  let config = Config::default();
  let theme = Theme::from(&config);
  let mut canvas = Canvas::new(32, 32).unwrap();
  canvas.clear(config.background.color);
  let mut cache = GlassCache::new();
  let now = Instant::now();
  let changed = cache.prepare_with(&canvas, 3, now, &theme, 1.0, false, |_, _, _| None);
  assert!(changed);
  assert_eq!(cache.cached_frame(), 3);
  assert_eq!(cache.blur_size(), Some((32, 32)));
}

/// A checkerboard is the worst case for any blur. If the result is not
/// measurably flatter than the source, the kernel is not doing its job.
#[test]
fn blurring_a_checkerboard_reduces_variance() {
  let mut source = Pixmap::new(16, 16).unwrap();
  for (index, pixel) in source
    .data_mut()
    .as_chunks_mut::<4>()
    .0
    .iter_mut()
    .enumerate()
  {
    let value = if (index % 16 + index / 16) % 2 == 0 {
      255
    } else {
      0
    };
    pixel.copy_from_slice(&[value, value, value, 255]);
  }
  let blur = BlurConfig {
    size: 4.0,
    passes: 3,
    noise: 0.0,
    ..BlurConfig::default()
  };
  let output = make_blurred(&source, &blur, 1.0).expect("the CPU blur must produce a pixmap");
  assert!(variance(output.data()) < variance(source.data()));
}

/// Luminance variance over the RGB channels of an RGBA byte buffer.
fn variance(data: &[u8]) -> f64 {
  let values: Vec<f64> = data
    .as_chunks::<4>()
    .0
    .iter()
    .map(|pixel| {
      0.299 * f64::from(pixel[0]) + 0.587 * f64::from(pixel[1]) + 0.114 * f64::from(pixel[2])
    })
    .collect();
  let mean = values.iter().sum::<f64>() / values.len() as f64;
  values
    .iter()
    .map(|value| (value - mean).powi(2))
    .sum::<f64>()
    / values.len() as f64
}
