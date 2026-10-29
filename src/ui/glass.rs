//! Frosted-glass panels without compositor blur.
//!
//! Under a kiosk compositor we cannot ask for backdrop blur, so this module
//! reproduces Hyprland's `decoration:blur` on the CPU: the background is
//! prepared with contrast and brightness, run through a dual Kawase pyramid
//! (`blur1` downsample, `blur2` tent upsample), then finished with grain and
//! brightness. The result is cached and clipped behind each panel, which then
//! gets a translucent tint, a one-pixel highlight, a soft shadow, and optional
//! noise.
//!
//! The names and defaults come straight from Hyprland's blur block. Colour
//! management and xray are compositor features that do not apply to a single
//! opaque greeter, so they are not exposed here.
//!
//! Animated wallpapers do not re-blur every frame. The cache refreshes on the
//! configured interval (300 ms by default), which is cheap enough to leave the
//! login form responsive while still following the video.

use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use tracing::debug;

use crate::ui::canvas::{Canvas, Rect};
use crate::ui::theme::Theme;

const NOISE_SIZE: u32 = 64;

pub struct GlassCache {
  size: Option<(u32, u32)>,
  frame_id: u64,
  prepared_at: Option<Instant>,
  blurred: Option<Pixmap>,
  noise: Option<Pixmap>,
  last_cost: Duration,
  /// A blur refresh running on a worker thread. Keeping it off the UI thread
  /// means a video wallpaper never loses a frame while the backdrop updates.
  pending: Option<(u64, mpsc::Receiver<Pixmap>)>,
}

impl GlassCache {
  #[must_use]
  pub fn new() -> Self {
    Self {
      size: None,
      frame_id: u64::MAX,
      prepared_at: None,
      blurred: None,
      noise: None,
      last_cost: Duration::ZERO,
      pending: None,
    }
  }

  /// Refreshes the blur cache when the frame changed or the refresh interval
  /// elapsed. `scale` converts the theme's logical blur radius to pixels.
  ///
  /// Returns true when the cached blurred pixmap changed and the caller must
  /// repaint the UI overlay. The GPU path relies on this: it skips the whole
  /// text and panel raster on frames where only the wallpaper moved.
  pub fn prepare(
    &mut self,
    canvas: &Canvas,
    frame_id: u64,
    now: Instant,
    theme: &Theme,
    scale: f32,
    animated: bool,
  ) -> bool {
    let mut changed = false;
    let size = (canvas.width(), canvas.height());
    if size.0 == 0 || size.1 == 0 {
      return false;
    }
    // Pick up a finished background blur before deciding whether another one
    // is due. The video waker already wakes us every frame, so this poll is
    // free on an animated background and dormant on a still one.
    if let Some((pending_frame, receiver)) = &self.pending {
      match receiver.try_recv() {
        Ok(pixmap) => {
          self.blurred = Some(pixmap);
          self.frame_id = *pending_frame;
          self.prepared_at = Some(now);
          self.pending = None;
          changed = true;
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => self.pending = None,
      }
    }
    let resized = self.size != Some(size);
    if resized {
      self.size = Some(size);
      self.blurred = None;
      self.frame_id = u64::MAX;
      self.pending = None;
      changed = true;
    }
    if theme.noise && self.noise.is_none() {
      self.noise = make_noise();
    }

    if !theme.blur.enabled || theme.blur.size <= 0.0 {
      if self.blurred.take().is_some() {
        changed = true;
      }
      self.pending = None;
      return changed;
    }
    if !resized && self.frame_id == frame_id && self.blurred.is_some() {
      return changed;
    }
    if animated
      && self.frame_id != u64::MAX
      && !resized
      && self
        .prepared_at
        .is_some_and(|at| now.duration_since(at) < theme.blur.refresh)
    {
      return changed;
    }

    // The first blur has to be synchronous: a sec later the panel would flash
    // without its backdrop. Every later refresh runs on a worker thread.
    if self.blurred.is_none() {
      let started = Instant::now();
      self.blurred = make_blurred(canvas.pixmap(), &theme.blur, scale);
      self.frame_id = frame_id;
      self.prepared_at = Some(now);
      self.last_cost = started.elapsed();
      debug!(
        elapsed_ms = self.last_cost.as_secs_f64() * 1000.0,
        width = size.0,
        height = size.1,
        "prepared glass backdrop"
      );
      return true;
    }
    if self.pending.is_some() {
      return changed;
    }
    let source = canvas.pixmap().clone();
    let blur = theme.blur.clone();
    let (sender, receiver) = mpsc::channel();
    let spawned = thread::Builder::new()
      .name("dawn-blur".to_string())
      .spawn(move || {
        if let Some(pixmap) = make_blurred(&source, &blur, scale) {
          let _ = sender.send(pixmap);
        }
      });
    if spawned.is_ok() {
      self.pending = Some((frame_id, receiver));
    }
    changed
  }

  /// Paints one panel: shadow, blurred backdrop, tint, border highlight, and
  /// noise. `opacity` is used by the boot fade.
  pub fn panel(&self, canvas: &mut Canvas, rect: Rect, radius: f32, theme: &Theme, opacity: f32) {
    self.panel_visible(canvas, rect, radius, theme, opacity, true, true);
  }

  /// Same panel with the shadow and border independently switchable. A config
  /// that wants the glass tint but no outline, or no card at all, uses these
  /// flags instead of guessing a transparent colour.
  #[allow(clippy::too_many_arguments)]
  pub fn panel_visible(
    &self,
    canvas: &mut Canvas,
    rect: Rect,
    radius: f32,
    theme: &Theme,
    opacity: f32,
    shadow: bool,
    border: bool,
  ) {
    let opacity = opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 || rect.w <= 0.0 || rect.h <= 0.0 {
      return;
    }

    // Three offset layers make a soft shadow without a real blur kernel. The
    // panel itself covers the inner part, so the layers only show at the edge.
    if shadow {
      for (index, alpha) in [(3.0, 0.22), (7.0, 0.16), (12.0, 0.10)] {
        canvas.fill(
          Rect::new(rect.x + index * 0.35, rect.y + index, rect.w, rect.h),
          theme
            .shadow
            .with_alpha((f32::from(theme.shadow.a) * alpha * opacity) as u8),
          radius,
        );
      }
    }

    if let Some(blurred) = &self.blurred {
      canvas.fill_blurred(rect, radius, blurred, opacity);
    }
    let tint = theme
      .tint
      .with_alpha((f32::from(theme.tint.a) * opacity) as u8);
    canvas.fill(rect, tint, radius);

    // A light border plus a brighter top edge reads as a glass highlight.
    if border {
      let border = theme
        .border
        .with_alpha((f32::from(theme.border.a) * opacity) as u8);
      canvas.stroke(rect, border, radius, 1.0);
      canvas.line(
        (rect.x + radius, rect.y + 0.5),
        (rect.right() - radius, rect.y + 0.5),
        theme
          .border
          .with_alpha((f32::from(theme.border.a) * 0.55 * opacity) as u8),
        1.0,
      );
    }

    if theme.noise
      && let Some(noise) = &self.noise
    {
      canvas.fill_noise(rect, radius, noise, 0.05 * opacity);
    }
  }

  /// True when a blurred backdrop is available for panels.
  #[must_use]
  pub fn has_blur(&self) -> bool {
    self.blurred.is_some()
  }

  #[must_use]
  pub fn cost(&self) -> Duration {
    self.last_cost
  }

  /// The frame id currently cached, for tests.
  #[must_use]
  pub fn cached_frame(&self) -> u64 {
    self.frame_id
  }
}

impl Default for GlassCache {
  fn default() -> Self {
    Self::new()
  }
}

fn make_blurred(source: &Pixmap, blur: &crate::config::BlurConfig, scale: f32) -> Option<Pixmap> {
  // Blurring at full resolution buys nothing behind a tint: the result is
  // sampled bilinearly anyway. Starting the pyramid on a reduced copy cuts
  // the CPU cost by up to 16x and keeps a 60 fps wallpaper smooth even when
  // the panel is blurred. Small canvases stay at their native size.
  let shortest = source.width().min(source.height());
  let factor = if shortest >= 720 {
    16
  } else if shortest >= 360 {
    8
  } else if shortest >= 180 {
    4
  } else {
    1
  };
  let working = if factor > 1 {
    resize(source, source.width() / factor, source.height() / factor)
  } else {
    source.clone()
  };
  // The working grid is coarse, so a radius below one pixel still maps to
  // several screen pixels after the bilinear upscale. Allowing sub-pixel
  // radii is what keeps `size = 8` from looking like `size = 16`.
  let radius = (blur.size * scale / factor as f32).max(0.25);
  let mut current = prepare(&working, blur.contrast, blur.brightness);
  let mut levels = Vec::with_capacity(blur.passes as usize);

  for pass in 0..blur.passes {
    let level_radius = (radius / (1 << pass) as f32).max(0.5);
    let next = downsample(&current, level_radius, blur, blur.passes);
    if next.width() == current.width() && next.height() == current.height() {
      break;
    }
    levels.push(current);
    current = next;
  }

  let mut pass = levels.len();
  while let Some(target) = levels.pop() {
    pass = pass.saturating_sub(1);
    let level_radius = (radius / (1 << pass) as f32).max(0.5);
    current = upsample(&current, target.width(), target.height(), level_radius);
  }

  finish(&mut current, blur.noise, blur.brightness);
  Some(current)
}

/// Hyprland's `blurprepare`: apply the contrast gain and any brightness above
/// 1.0 before the pyramid. The default brightness is below 1, so it darkens in
/// `finish` instead, exactly like the shader pair.
fn prepare(source: &Pixmap, contrast: f32, brightness: f32) -> Pixmap {
  let mut output = source.clone();
  let contrast = contrast.max(0.0);
  let brighten = brightness.max(1.0);
  for pixel in output.data_mut().chunks_exact_mut(4) {
    if pixel[3] == 0 {
      continue;
    }
    for channel in 0..3 {
      let value = f32::from(pixel[channel]) / 255.0;
      let gained = gain(value, contrast) * brighten;
      pixel[channel] = to_byte(gained * (f32::from(pixel[3]) / 255.0));
    }
  }
  output
}

/// The gain curve from Hyprland's `gain.glsl`. It is a symmetric power curve
/// around 0.5, which keeps black and white fixed and makes midtones punchier.
fn gain(value: f32, k: f32) -> f32 {
  let x = value.clamp(0.0, 1.0);
  if x < 0.5 {
    0.5 * (2.0 * x).powf(k)
  } else {
    1.0 - 0.5 * (2.0 * (1.0 - x)).powf(k)
  }
}

/// One downsample pass of the dual Kawase pyramid. Five taps: the centre with
/// weight 4 and the four diagonals with weight 1, divided by 8. This is the
/// `blur1` shader.
fn downsample(
  source: &Pixmap,
  radius: f32,
  blur: &crate::config::BlurConfig,
  passes: u32,
) -> Pixmap {
  let width = (source.width() / 2).max(1);
  let height = (source.height() / 2).max(1);
  let Some(mut output) = Pixmap::new(width, height) else {
    return source.clone();
  };
  let sx = source.width() as f32 / width as f32;
  let sy = source.height() as f32 / height as f32;
  for y in 0..height {
    for x in 0..width {
      let cx = (x as f32 + 0.5) * sx - 0.5;
      let cy = (y as f32 + 0.5) * sy - 0.5;
      let center = sample_bilinear(source, cx, cy);
      let mut sum = [
        center[0] * 4.0,
        center[1] * 4.0,
        center[2] * 4.0,
        center[3] * 4.0,
      ];
      for (dx, dy) in [(-1.0, -1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, 1.0)] {
        let sample = sample_bilinear(source, cx + dx * radius, cy + dy * radius);
        for channel in 0..4 {
          sum[channel] += sample[channel];
        }
      }
      let mut color = [sum[0] / 8.0, sum[1] / 8.0, sum[2] / 8.0, sum[3] / 8.0];
      if blur.vibrancy > 0.0 && color[3] > 0.0 {
        apply_vibrancy(&mut color, blur.vibrancy, blur.vibrancy_darkness, passes);
      }
      write_pixel(&mut output, x, y, color);
    }
  }
  output
}

/// The `blur2` tent upsample. Eight neighbours, weights 1 and 2, divided by
/// 12, which is the standard dual Kawase reconstruction filter.
fn upsample(source: &Pixmap, width: u32, height: u32, radius: f32) -> Pixmap {
  let Some(mut output) = Pixmap::new(width, height) else {
    return source.clone();
  };
  let sx = source.width() as f32 / width as f32;
  let sy = source.height() as f32 / height as f32;
  let taps = [
    (-2.0, 0.0, 1.0),
    (-1.0, 1.0, 2.0),
    (0.0, 2.0, 1.0),
    (1.0, 1.0, 2.0),
    (2.0, 0.0, 1.0),
    (1.0, -1.0, 2.0),
    (0.0, -2.0, 1.0),
    (-1.0, -1.0, 2.0),
  ];
  for y in 0..height {
    for x in 0..width {
      let cx = (x as f32 + 0.5) * sx - 0.5;
      let cy = (y as f32 + 0.5) * sy - 0.5;
      let mut sum = [0.0f32; 4];
      for (dx, dy, weight) in taps {
        let sample = sample_bilinear(source, cx + dx * radius, cy + dy * radius);
        for channel in 0..4 {
          sum[channel] += sample[channel] * weight;
        }
      }
      write_pixel(
        &mut output,
        x,
        y,
        [sum[0] / 12.0, sum[1] / 12.0, sum[2] / 12.0, sum[3] / 12.0],
      );
    }
  }
  output
}

/// `blurFinish`: deterministic per-pixel grain and the final brightness. The
/// grain depends on the pixel coordinate, not on time, so a still wallpaper
/// does not shimmer.
fn finish(source: &mut Pixmap, noise: f32, brightness: f32) {
  let pixel_width = source.width();
  let width = pixel_width as f32;
  let height = source.height() as f32;
  let brightness = brightness.min(1.0);
  for (index, pixel) in source.data_mut().chunks_exact_mut(4).enumerate() {
    let x = (index as u32 % pixel_width) as f32 / width;
    let y = (index as u32 / pixel_width) as f32 / height;
    let amount = (hash(x, y) - 0.5) * noise * (f32::from(pixel[3]) / 255.0);
    for channel in pixel.iter_mut().take(3) {
      let value = f32::from(*channel) / 255.0 + amount;
      *channel = to_byte(value * brightness);
    }
  }
}

fn hash(x: f32, y: f32) -> f32 {
  let fract = |value: f32| value - value.floor();
  let mut p = [
    fract(x * 1689.1984),
    fract(y * 1689.1984),
    fract(x * 1689.1984),
  ];
  let dot = p[0] * (p[1] + 33.33) + p[1] * (p[2] + 33.33) + p[2] * (p[0] + 33.33);
  for value in &mut p {
    *value += dot;
  }
  fract((p[0] + p[1]) * p[2])
}

fn resize(source: &Pixmap, width: u32, height: u32) -> Pixmap {
  let Some(mut output) = Pixmap::new(width.max(1), height.max(1)) else {
    return source.clone();
  };
  let sx = source.width() as f32 / width.max(1) as f32;
  let sy = source.height() as f32 / height.max(1) as f32;
  for y in 0..output.height() {
    for x in 0..output.width() {
      let color = sample_bilinear(
        source,
        (x as f32 + 0.5) * sx - 0.5,
        (y as f32 + 0.5) * sy - 0.5,
      );
      write_pixel(&mut output, x, y, color);
    }
  }
  output
}

fn sample_bilinear(source: &Pixmap, x: f32, y: f32) -> [f32; 4] {
  let max_x = source.width().saturating_sub(1) as f32;
  let max_y = source.height().saturating_sub(1) as f32;
  let x = x.clamp(0.0, max_x);
  let y = y.clamp(0.0, max_y);
  let x0 = x.floor() as u32;
  let y0 = y.floor() as u32;
  let x1 = (x0 + 1).min(source.width().saturating_sub(1));
  let y1 = (y0 + 1).min(source.height().saturating_sub(1));
  let fx = x - x0 as f32;
  let fy = y - y0 as f32;
  let data = source.data();
  let pixel = |px: u32, py: u32| -> [f32; 4] {
    let index = ((py * source.width() + px) * 4) as usize;
    [
      f32::from(data[index]) / 255.0,
      f32::from(data[index + 1]) / 255.0,
      f32::from(data[index + 2]) / 255.0,
      f32::from(data[index + 3]) / 255.0,
    ]
  };
  let top_left = pixel(x0, y0);
  let top_right = pixel(x1, y0);
  let bottom_left = pixel(x0, y1);
  let bottom_right = pixel(x1, y1);
  let mut color = [0.0f32; 4];
  for channel in 0..4 {
    let top = top_left[channel] + (top_right[channel] - top_left[channel]) * fx;
    let bottom = bottom_left[channel] + (bottom_right[channel] - bottom_left[channel]) * fx;
    color[channel] = top + (bottom - top) * fy;
  }
  color
}

fn write_pixel(pixmap: &mut Pixmap, x: u32, y: u32, color: [f32; 4]) {
  let index = ((y * pixmap.width() + x) * 4) as usize;
  let data = pixmap.data_mut();
  for channel in 0..4 {
    data[index + channel] = to_byte(color[channel]);
  }
}

fn to_byte(value: f32) -> u8 {
  (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The saturation boost from Hyprland's `blur1` shader. It boosts colour
/// without blowing out dark blues, which is what makes compositor glass look
/// vivid instead of grey.
fn apply_vibrancy(color: &mut [f32; 4], vibrancy: f32, darkness: f32, passes: u32) {
  let alpha = color[3];
  if alpha <= 0.0 {
    return;
  }
  let mut rgb = [color[0] / alpha, color[1] / alpha, color[2] / alpha];
  let hsl = rgb_to_hsl(rgb);
  let darkness = 1.0 - darkness.clamp(0.0, 1.0);
  let perceived = sigmoid(
    (rgb[0] * rgb[0] * 0.299 + rgb[1] * rgb[1] * 0.587 + rgb[2] * rgb[2] * 0.114).sqrt(),
    0.8 * darkness,
  );
  let base = 0.11 * darkness;
  let angle = 0.93f32;
  let boost = if hsl[1] > 0.0 {
    let value =
      1.0 - ((1.0 - hsl[1] * angle.cos()).powi(2) + (1.0 - perceived * angle.sin()).powi(2));
    smoothstep(base - 0.33, base + 0.33, value)
  } else {
    0.0
  };
  let saturation = (hsl[1] + boost * vibrancy / passes.max(1) as f32).clamp(0.0, 1.0);
  rgb = hsl_to_rgb([hsl[0], saturation, hsl[2]]);
  color[0] = rgb[0] * alpha;
  color[1] = rgb[1] * alpha;
  color[2] = rgb[2] * alpha;
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
  let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
  t * t * (3.0 - 2.0 * t)
}

fn sigmoid(x: f32, a: f32) -> f32 {
  let a = a.clamp(0.0, 1.0);
  if x <= a {
    a - (a * a - x * x).max(0.0).sqrt()
  } else {
    a + ((1.0 - a).powi(2) - (x - 1.0).powi(2)).max(0.0).sqrt()
  }
}

fn rgb_to_hsl(color: [f32; 3]) -> [f32; 3] {
  let (r, g, b) = (color[0], color[1], color[2]);
  let max = r.max(g).max(b);
  let min = r.min(g).min(b);
  let delta = max - min;
  let lightness = (max + min) / 2.0;
  let saturation = if delta <= 0.0 {
    0.0
  } else {
    delta / (1.0 - (2.0 * lightness - 1.0).abs())
  };
  let mut hue = if delta <= 0.0 {
    0.0
  } else if max == r {
    ((g - b) / delta).rem_euclid(6.0)
  } else if max == g {
    (b - r) / delta + 2.0
  } else {
    (r - g) / delta + 4.0
  };
  hue /= 6.0;
  [hue, saturation, lightness]
}

fn hsl_to_rgb(color: [f32; 3]) -> [f32; 3] {
  let (hue, saturation, lightness) = (color[0], color[1], color[2]);
  let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
  let sector = hue * 6.0;
  let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
  let (r, g, b) = if sector < 1.0 {
    (chroma, x, 0.0)
  } else if sector < 2.0 {
    (x, chroma, 0.0)
  } else if sector < 3.0 {
    (0.0, chroma, x)
  } else if sector < 4.0 {
    (0.0, x, chroma)
  } else if sector < 5.0 {
    (x, 0.0, chroma)
  } else {
    (chroma, 0.0, x)
  };
  let m = lightness - chroma / 2.0;
  [r + m, g + m, b + m]
}

fn make_noise() -> Option<Pixmap> {
  let mut pixmap = Pixmap::new(NOISE_SIZE, NOISE_SIZE)?;
  let mut state = 0x9e37_79b9_7f4a_7c15u64;
  for pixel in pixmap.data_mut().chunks_exact_mut(4) {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    let alpha = (state & 0x07) as u8;
    // tiny-skia pixmaps are premultiplied, so a white pixel with coverage
    // `alpha` must store the same value in RGB and A. Storing 255 here would
    // paint bright specks instead of a faint grain.
    pixel.copy_from_slice(&[alpha, alpha, alpha, alpha]);
  }
  Some(pixmap)
}

#[cfg(test)]
mod tests {
  use super::GlassCache;
  use crate::config::Config;
  use crate::ui::canvas::Canvas;
  use crate::ui::theme::Theme;
  use std::time::Instant;

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
}
