//! Post pass for the renderer.

use super::*;

pub(super) fn draw_splash(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  view: &crate::ui::view::SplashView,
  scale: f32,
) {
  if let Some(art) = scene.splash_art {
    art.paint(canvas, view);
    return;
  }
  let accent = view
    .accent
    .unwrap_or_else(|| scene.theme.accent_on(scene.background_color));
  splash::paint(canvas, text, view, scene.background_color, accent, scale);
}

/// Draws the CPU-side post effects. Camera, chromatic aberration, and glitch
/// slices live in the GPU shader; letterbox, vignette, and speed lines are
/// painted here so the software path still shows them.
pub(super) fn draw_post(canvas: &mut Canvas, scene: &Scene<'_>, screen: (f32, f32)) {
  draw_post_parts(
    canvas,
    scene.motion.post,
    screen,
    scene.motion.beat_index,
    true,
  );
}

/// The GPU compositor draws the vignette in the shader, so its overlay only
/// needs the letterbox and speed lines. Keeping the 22 ms radial gradient out
/// of the per-frame overlay is what lets a fullscreen video stay at 60 fps.
pub(super) fn draw_post_gpu(canvas: &mut Canvas, scene: &Scene<'_>, screen: (f32, f32)) {
  draw_post_parts(
    canvas,
    scene.motion.post,
    screen,
    scene.motion.beat_index,
    false,
  );
}

pub(super) fn draw_post_parts(
  canvas: &mut Canvas,
  post: crate::ui::motion::PostFrame,
  screen: (f32, f32),
  seed: u64,
  draw_vignette: bool,
) {
  if screen.0 <= 0.0 || screen.1 <= 0.0 {
    return;
  }
  if draw_vignette && post.vignette > 0.0 {
    let radius = (screen.0.hypot(screen.1) / 2.0).max(1.0);
    let edge = Rgb::solid(0, 0, 0).with_alpha((post.vignette * 190.0) as u8);
    canvas.radial_gradient(
      (screen.0 / 2.0, screen.1 / 2.0),
      radius,
      &[
        (0.0, Rgb::solid(0, 0, 0).with_alpha(0)),
        (0.55, Rgb::solid(0, 0, 0).with_alpha(0)),
        (1.0, edge),
      ],
    );
  }
  if post.speed_lines > 0.0 {
    draw_speed_lines(canvas, screen, post.speed_lines, seed);
  }
  if post.letterbox > 0.0 {
    let bar = (post.letterbox * screen.1).max(0.0);
    let black = Rgb::solid(0, 0, 0);
    canvas.fill(Rect::new(0.0, 0.0, screen.0, bar), black, 0.0);
    canvas.fill(Rect::new(0.0, screen.1 - bar, screen.0, bar), black, 0.0);
  }
}

/// Translucent diagonal streaks from the edges toward the middle. The layout is
/// deterministic from the screen size and the beat index, so a captured frame
/// is reproducible and the effect does not flicker when a repaint is skipped.
pub(super) fn draw_speed_lines(canvas: &mut Canvas, screen: (f32, f32), intensity: f32, seed: u64) {
  let intensity = intensity.clamp(0.0, 1.0);
  if intensity <= 0.0 {
    return;
  }
  let alpha = (intensity * 85.0).round().clamp(0.0, 255.0) as u8;
  if alpha == 0 {
    return;
  }
  let color = Rgb::solid(255, 255, 255).with_alpha(alpha);
  let (cx, cy) = (screen.0 / 2.0, screen.1 / 2.0);
  let reach = screen.0.hypot(screen.1);
  let count = 18u64;
  for index in 0..count {
    let hash = seed
      .wrapping_mul(0x9E37_79B9_7F4A_7C15)
      .wrapping_add(index.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    let unit = (hash >> 40) as f32 / 16_777_216.0;
    let along = (index as f32 + unit) / count as f32;
    // Two diagonal families read as speed rather than a radial burst.
    let angle: f32 = if index % 2 == 0 { 0.62 } else { 3.76 };
    let perpendicular = (along - 0.5) * reach * 0.9;
    let nx = angle.cos();
    let ny = angle.sin();
    let px = -ny;
    let py = nx;
    let outer = reach * (0.42 + unit * 0.18);
    let inner = outer * (0.35 + 0.18 * (1.0 - intensity));
    let start = (
      cx + nx * outer + px * perpendicular,
      cy + ny * outer + py * perpendicular,
    );
    let end = (
      cx + nx * inner + px * perpendicular,
      cy + ny * inner + py * perpendicular,
    );
    canvas.line(start, end, color, (1.0 + intensity * 1.5).max(1.0));
  }
}

/// Seconds since the UI started, frozen at zero when motion is reduced or the
/// greeter is in low-power mode. Ambient features use this instead of reading
/// the wall clock directly so they pause together.
pub(super) fn ambient_elapsed(scene: &Scene<'_>) -> f32 {
  if scene.theme.motion.reduced || scene.theme.low_power {
    return 0.0;
  }
  scene
    .frame_now
    .saturating_duration_since(scene.started)
    .as_secs_f32()
    .max(0.0)
}

/// The window and screenshot paths both accept a caller-provided scale, but a
/// zero or negative value would collapse every layout constant. Clamping here
/// keeps the two render entry points on the same rule.
pub(super) fn normalized_scale(scale: f32) -> f32 {
  if scale.is_finite() && scale > 0.25 {
    scale
  } else {
    1.0
  }
}

/// Splits a run into everything before the final character and the final
/// character itself. `char_indices` is the only safe way to do this: a byte
/// slice at `len - 1` would panic on a multibyte glyph or split it in half.
pub(super) fn split_last_char(text: &str) -> Option<(&str, &str)> {
  let (index, _) = text.char_indices().next_back()?;
  Some((&text[..index], &text[index..]))
}

/// Expo out: fast first frame, long settle. Used by the keystroke pop, where
/// the new character should overshoot immediately and then calm down rather
/// than glide up from its resting size.
pub(super) fn ease_out_expo(t: f32) -> f32 {
  let t = t.clamp(0.0, 1.0);
  if t >= 1.0 {
    1.0
  } else {
    1.0 - 2.0_f32.powf(-10.0 * t)
  }
}
