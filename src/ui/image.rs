//! Safe decoding and resizing for wallpapers, album art, and plugin images.
//!
//! Every file that reaches this module is untrusted. We check the file size
//! before reading, give the decoder explicit dimension and allocation limits,
//! resize in `u32` space, and only then build a premultiplied tiny-skia pixmap.
//! Animated formats are decoded one frame at a time; the caller gets a frame
//! set that already fits a hard memory budget.
//!
//! This is intentionally the only place that depends on the `image` crate. If
//! a format is added later, it stays contained here.

use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::Path;
use std::time::Duration;

use image::{
  AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbaImage, codecs,
  imageops,
};
use tiny_skia::{IntSize, Pixmap};

use crate::color::Rgb;
use crate::layout::Fit;

/// Decoder limits. These are deliberately smaller than "whatever the machine
/// has": a greeter should fail a hostile wallpaper before it competes with the
/// kernel and the login process for memory.
#[derive(Debug, Clone, Copy)]
pub struct DecodeLimits {
  pub max_file_bytes: u64,
  pub max_dimension: u32,
  pub max_pixels: u64,
  pub max_alloc: u64,
}

impl Default for DecodeLimits {
  fn default() -> Self {
    Self {
      max_file_bytes: 64 * 1024 * 1024,
      max_dimension: 8192,
      max_pixels: 32 * 1024 * 1024,
      max_alloc: 256 * 1024 * 1024,
    }
  }
}

/// Loads a still image and fits it to `target` using `fit`.
pub fn load_static(
  path: &Path,
  limits: DecodeLimits,
  target: (u32, u32),
  fit: Fit,
  background: Rgb,
) -> Result<Pixmap, String> {
  let image = decode_static(path, limits)?;
  let fitted = fit_image(image, target, fit, background);
  to_pixmap(fitted).ok_or_else(|| "image dimensions are not representable".to_string())
}

/// Loads a still image at its native size. Used for logos and other decor,
/// where the renderer decides the display size later.
pub fn load_native(path: &Path, limits: DecodeLimits) -> Result<Pixmap, String> {
  let image = decode_static(path, limits)?;
  to_pixmap(image).ok_or_else(|| "image dimensions are not representable".to_string())
}

/// Decodes a still image into RGBA. The caller must not change its size.
pub fn decode_static(path: &Path, limits: DecodeLimits) -> Result<RgbaImage, String> {
  check_file(path, limits)?;
  let reader = ImageReader::open(path)
    .map_err(|error| format!("cannot open {}: {error}", path.display()))?
    .with_guessed_format()
    .map_err(|error| {
      format!(
        "cannot detect the image format of {}: {error}",
        path.display()
      )
    })?;
  let mut reader = reader;
  reader.limits(decoder_limits(limits));
  let image = reader
    .decode()
    .map_err(|error| format!("cannot decode {}: {error}", path.display()))?;
  let rgba = image.into_rgba8();
  check_pixels(path, rgba.width(), rgba.height(), limits)?;
  Ok(rgba)
}

/// The animation formats whose frames can be streamed one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AnimationFormat {
  Gif,
  WebP,
}

/// Detects whether a file is an animated GIF or WebP. Still WebP files return
/// `None`, so they keep using the still-image path. A one-frame GIF still
/// reports `Gif`; the stream simply loops that one frame.
pub(crate) fn animation_format(path: &Path) -> Result<Option<AnimationFormat>, String> {
  let format = ImageReader::open(path)
    .map_err(|error| format!("cannot open {}: {error}", path.display()))?
    .with_guessed_format()
    .map_err(|error| {
      format!(
        "cannot detect the image format of {}: {error}",
        path.display()
      )
    })?
    .format();
  match format {
    Some(ImageFormat::Gif) => Ok(Some(AnimationFormat::Gif)),
    Some(ImageFormat::WebP) if webp_is_animated(path)? => Ok(Some(AnimationFormat::WebP)),
    _ => Ok(None),
  }
}

/// Decodes one pass of an animation, handing each fitted frame to `on_frame`.
///
/// This is the streaming path. Only one frame is resident at a time: the
/// caller publishes the frame and drops it before the next decode. `on_frame`
/// returns `false` to stop early, which is how pause, stop, and a resize all
/// interrupt a pass without waiting for the file to finish. The return value is
/// the number of frames decoded.
#[allow(clippy::too_many_arguments)]
pub(crate) fn decode_animation<F>(
  path: &Path,
  format: AnimationFormat,
  target: (u32, u32),
  fit: Fit,
  background: Rgb,
  limits: DecodeLimits,
  fps: u32,
  memory_limit: u64,
  mut on_frame: F,
) -> Result<usize, String>
where
  F: FnMut(Pixmap, Duration) -> bool,
{
  check_file(path, limits)?;
  let file =
    File::open(path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;

  let (frames, width, height) = match format {
    AnimationFormat::Gif => {
      let mut decoder = codecs::gif::GifDecoder::new(BufReader::new(file))
        .map_err(|error| format!("cannot decode {}: {error}", path.display()))?;
      decoder
        .set_limits(decoder_limits(limits))
        .map_err(|error| format!("cannot decode {}: {error}", path.display()))?;
      let (width, height) = decoder.dimensions();
      (decoder.into_frames(), width, height)
    }
    AnimationFormat::WebP => {
      let decoder = codecs::webp::WebPDecoder::new(BufReader::new(file))
        .map_err(|error| format!("cannot decode {}: {error}", path.display()))?;
      let (width, height) = decoder.dimensions();
      (decoder.into_frames(), width, height)
    }
  };
  check_pixels(path, width, height, limits)?;

  // Even a streaming frame is kept in memory until the next one arrives, so
  // the per-frame budget still applies. Capping to the source also avoids
  // upscaling small GIFs on the way to the canvas.
  let (target, _warning) = clamp_target(target, Some((width, height)), memory_limit);
  // Zero means native timing. A positive value is a floor on the delay, which
  // caps the effective frame rate without speeding up a slow animation.
  let min_delay = if fps == 0 {
    Duration::ZERO
  } else {
    Duration::from_millis((1000 / fps.max(1)) as u64)
  };
  let mut count = 0;

  for frame in frames {
    let frame = frame.map_err(|error| format!("cannot decode {}: {error}", path.display()))?;
    let (delay_num, delay_den) = frame.delay().numer_denom_ms();
    let millis = if delay_den == 0 {
      100
    } else {
      u64::from(delay_num) / u64::from(delay_den)
    };
    let millis = if millis == 0 { 100 } else { millis };
    let delay = Duration::from_millis(millis).max(min_delay);
    let fitted = fit_image(frame.into_buffer(), target, fit, background);
    let Some(pixmap) = to_pixmap(fitted) else {
      continue;
    };
    count += 1;
    if !on_frame(pixmap, delay) {
      break;
    }
  }

  if count == 0 {
    return Err(format!("{} contains no decodable frames", path.display()));
  }
  Ok(count)
}

/// Reads only the WebP header to decide whether the file is an animation.
/// `WebPDecoder::has_animation` is cheap and does not decode any frame.
fn webp_is_animated(path: &Path) -> Result<bool, String> {
  let file =
    File::open(path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
  let decoder = codecs::webp::WebPDecoder::new(BufReader::new(file))
    .map_err(|error| format!("cannot decode {}: {error}", path.display()))?;
  Ok(decoder.has_animation())
}

/// Fits a decoded RGBA image into the target size. The output is exactly
/// `target` pixels, so animated frames can be blitted without per-frame math.
#[must_use]
pub fn fit_image(image: RgbaImage, target: (u32, u32), fit: Fit, background: Rgb) -> RgbaImage {
  let (target_w, target_h) = (target.0.max(1), target.1.max(1));
  let (source_w, source_h) = image.dimensions();
  // The common streaming case is a 1080p source on a 1080p output. Resizing
  // to the same size would run a filter over every pixel of every frame for
  // no reason.
  if source_w == target_w && source_h == target_h {
    return image;
  }
  if source_w == 0 || source_h == 0 {
    return RgbaImage::from_pixel(
      target_w,
      target_h,
      image::Rgba([background.r, background.g, background.b, 255]),
    );
  }

  match fit {
    Fit::Fill => imageops::resize(&image, target_w, target_h, imageops::FilterType::Triangle),
    Fit::Cover => {
      let scale = (target_w as f32 / source_w as f32).max(target_h as f32 / source_h as f32);
      let width = (source_w as f32 * scale).ceil().max(1.0) as u32;
      let height = (source_h as f32 * scale).ceil().max(1.0) as u32;
      let resized = imageops::resize(&image, width, height, imageops::FilterType::Triangle);
      let x = width.saturating_sub(target_w) / 2;
      let y = height.saturating_sub(target_h) / 2;
      imageops::crop_imm(&resized, x, y, target_w, target_h).to_image()
    }
    Fit::Contain => {
      let scale = (target_w as f32 / source_w as f32).min(target_h as f32 / source_h as f32);
      let width = (source_w as f32 * scale).ceil().max(1.0) as u32;
      let height = (source_h as f32 * scale).ceil().max(1.0) as u32;
      let resized = imageops::resize(&image, width, height, imageops::FilterType::Triangle);
      let mut canvas = RgbaImage::from_pixel(
        target_w,
        target_h,
        image::Rgba([background.r, background.g, background.b, 255]),
      );
      let x = i64::from(target_w.saturating_sub(width) / 2);
      let y = i64::from(target_h.saturating_sub(height) / 2);
      imageops::overlay(&mut canvas, &resized, x, y);
      canvas
    }
  }
}

/// Converts straight-alpha RGBA to the premultiplied layout tiny-skia expects.
pub(crate) fn to_pixmap(image: RgbaImage) -> Option<Pixmap> {
  let (width, height) = image.dimensions();
  let mut data = image.into_raw();
  for pixel in data.chunks_exact_mut(4) {
    let alpha = u32::from(pixel[3]);
    pixel[0] = ((u32::from(pixel[0]) * alpha) / 255) as u8;
    pixel[1] = ((u32::from(pixel[1]) * alpha) / 255) as u8;
    pixel[2] = ((u32::from(pixel[2]) * alpha) / 255) as u8;
  }
  let size = IntSize::from_wh(width, height)?;
  Pixmap::from_vec(data, size)
}

pub(crate) fn clamp_target(
  target: (u32, u32),
  source: Option<(u32, u32)>,
  memory_limit: u64,
) -> ((u32, u32), Option<String>) {
  let (mut width, mut height) = (target.0.max(1), target.1.max(1));

  // Never upscale the source into the frame cache. A 480p GIF on a 1080p
  // monitor should stay 480p and be scaled by the painter; upscaling every
  // stream frame would cost memory and decode time for no visible gain.
  if let Some((source_width, source_height)) = source {
    let source_pixels = u64::from(source_width) * u64::from(source_height);
    let target_pixels = u64::from(width) * u64::from(height);
    if source_pixels > 0 && target_pixels > source_pixels {
      let scale = (source_pixels as f64 / target_pixels as f64).sqrt();
      width = ((width as f64 * scale).round() as u32).max(1);
      height = ((height as f64 * scale).round() as u32).max(1);
    }
  }

  // Keep room for at least four frames before decimation starts. That gives
  // short animations a chance to look animated under the default budget.
  let per_frame = (memory_limit / 4).max(4);
  let bytes = u64::from(width) * u64::from(height) * 4;
  if bytes <= per_frame {
    return ((width, height), None);
  }
  let scale = (per_frame as f64 / bytes as f64).sqrt();
  let new_width = ((width as f64 * scale) as u32).max(1);
  let new_height = ((height as f64 * scale) as u32).max(1);
  (
    (new_width, new_height),
    Some(format!(
      "animation frames were downscaled to {new_width}x{new_height} to fit the {} MiB memory cap",
      memory_limit / 1024 / 1024
    )),
  )
}

fn decoder_limits(limits: DecodeLimits) -> image::Limits {
  let mut decoder = image::Limits::default();
  decoder.max_image_width = Some(limits.max_dimension);
  decoder.max_image_height = Some(limits.max_dimension);
  decoder.max_alloc = Some(limits.max_alloc);
  decoder
}

fn check_file(path: &Path, limits: DecodeLimits) -> Result<(), String> {
  let metadata = std::fs::metadata(path).map_err(|error| {
    format!(
      "cannot read {}: {error} (fix: correct the path or permissions)",
      path.display()
    )
  })?;
  if !metadata.is_file() {
    return Err(format!(
      "{} is not a regular file (fix: point at an image file)",
      path.display()
    ));
  }
  if metadata.len() > limits.max_file_bytes {
    return Err(format!(
      "{} is larger than the {} MiB limit (fix: downscale or re-encode it)",
      path.display(),
      limits.max_file_bytes / 1024 / 1024
    ));
  }
  Ok(())
}

fn check_pixels(path: &Path, width: u32, height: u32, limits: DecodeLimits) -> Result<(), String> {
  if width == 0
    || height == 0
    || width > limits.max_dimension
    || height > limits.max_dimension
    || u64::from(width) * u64::from(height) > limits.max_pixels
  {
    return Err(format!(
      "{} is {width}x{height}; the limit is {}x{} and {} megapixels",
      path.display(),
      limits.max_dimension,
      limits.max_dimension,
      limits.max_pixels / 1024 / 1024
    ));
  }
  Ok(())
}

/// Decodes bytes that came from somewhere other than a path, such as a
/// file:// URI read into memory. Kept for album art and tests.
pub fn decode_bytes(bytes: &[u8], limits: DecodeLimits) -> Result<RgbaImage, String> {
  let reader = ImageReader::new(Cursor::new(bytes))
    .with_guessed_format()
    .map_err(|error| format!("cannot detect the image format: {error}"))?;
  let mut reader = reader;
  reader.limits(decoder_limits(limits));
  let image: DynamicImage = reader
    .decode()
    .map_err(|error| format!("cannot decode image data: {error}"))?;
  let rgba = image.into_rgba8();
  check_pixels(Path::new("<memory>"), rgba.width(), rgba.height(), limits)?;
  Ok(rgba)
}

#[cfg(test)]
mod tests {
  use super::{DecodeLimits, fit_image, to_pixmap};
  use crate::color::Rgb;
  use crate::layout::Fit;

  fn solid(width: u32, height: u32) -> image::RgbaImage {
    image::RgbaImage::from_pixel(width, height, image::Rgba([200, 40, 20, 255]))
  }

  #[test]
  fn fit_modes_always_produce_the_target_size() {
    for fit in [Fit::Cover, Fit::Contain, Fit::Fill] {
      let fitted = fit_image(solid(400, 200), (100, 100), fit, Rgb::solid(0, 0, 0));
      assert_eq!(fitted.dimensions(), (100, 100), "{fit:?}");
    }
  }

  #[test]
  fn premultiplication_matches_tiny_skia() {
    let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([200, 100, 50, 128]));
    let pixmap = to_pixmap(image).unwrap();
    assert_eq!(&pixmap.data()[..4], &[100, 50, 25, 128]);
  }

  #[test]
  fn a_png_round_trip_produces_the_requested_size() {
    let dir = std::env::temp_dir().join(format!("dawn-image-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("wall.png");
    let mut source = tiny_skia::Pixmap::new(8, 4).unwrap();
    source.fill(tiny_skia::Color::from_rgba8(200, 40, 20, 255));
    source.save_png(&path).unwrap();

    let loaded = super::load_static(
      &path,
      DecodeLimits::default(),
      (16, 16),
      Fit::Cover,
      Rgb::solid(0, 0, 0),
    )
    .unwrap();
    assert_eq!((loaded.width(), loaded.height()), (16, 16));
    let _ = std::fs::remove_dir_all(&dir);
  }

  #[test]
  fn garbage_bytes_are_an_error_not_a_panic() {
    let error = super::decode_bytes(b"not really an image", DecodeLimits::default()).unwrap_err();
    assert!(error.contains("decode") || error.contains("format"));
  }

  #[test]
  fn animated_frames_are_not_upscaled_into_the_cache() {
    // A small GIF should stay at its native size and loop smoothly instead of
    // filling the memory budget with upscaled 1080p frames.
    let (target, warning) = super::clamp_target((1920, 1080), Some((480, 270)), 128 * 1024 * 1024);
    assert_eq!(target, (480, 270));
    assert!(warning.is_none());

    // A source larger than the monitor is still bounded by the monitor.
    let (target, warning) =
      super::clamp_target((1920, 1080), Some((3840, 2160)), 128 * 1024 * 1024);
    assert_eq!(target, (1920, 1080));
    assert!(warning.is_none());
  }

  #[test]
  fn a_still_webp_is_not_treated_as_an_animation() {
    // A WebP container can hold a still image. It must not be routed through
    // the animation decoder, which yields nothing for a still and would
    // silently fall back to the gradient.
    use image::ImageEncoder;

    let dir = std::env::temp_dir().join(format!("dawn-image-webp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("still.webp");
    let source = solid(8, 4);
    let file = std::fs::File::create(&path).unwrap();
    image::codecs::webp::WebPEncoder::new_lossless(file)
      .write_image(
        source.as_raw(),
        source.width(),
        source.height(),
        image::ExtendedColorType::Rgba8,
      )
      .unwrap();

    assert!(super::animation_format(&path).unwrap().is_none());
    let frame = super::load_static(
      &path,
      DecodeLimits::default(),
      (8, 8),
      Fit::Fill,
      Rgb::solid(0, 0, 0),
    )
    .unwrap();
    assert_eq!((frame.width(), frame.height()), (8, 8));
    let _ = std::fs::remove_dir_all(&dir);
  }

  #[test]
  fn default_limits_are_bounded() {
    let limits = DecodeLimits::default();
    assert!(limits.max_file_bytes > 0);
    assert!(limits.max_pixels > 0);
    assert!(limits.max_dimension < 100_000);
  }
}
