//! Ticker pass for the renderer.

use super::*;

/// Scales a colour's existing alpha by `opacity`. Alpha is the only channel
/// the plate multiplier touches, so hue stays exactly what the theme wrote.
pub(super) fn alpha(color: Rgb, opacity: f32) -> Rgb {
  let value = (f32::from(color.a) * opacity.clamp(0.0, 1.0))
    .round()
    .clamp(0.0, 255.0) as u8;
  color.with_alpha(value)
}

/// Distance a marquee has travelled at `elapsed`, wrapped at one full run.
///
/// Keeping this tiny function pure is what makes the loop testable: the draw
/// path measures the run, asks for this offset, and paints two copies one run
/// width apart. There is no accumulated position that could drift after a long
/// session or a resize.
pub(super) fn ticker_offset(run_width: f32, speed: f32, elapsed: f32) -> f32 {
  if !run_width.is_finite() || run_width <= 0.0 {
    return 0.0;
  }
  let travelled = speed.max(0.0) * elapsed.max(0.0);
  if !travelled.is_finite() {
    return 0.0;
  }
  travelled % run_width
}

/// True for the short beat after the selected session changes. It is a pure
/// time window so the ticker can tint its run without any mutable flash state,
/// and so the window's frame scheduler can decide to keep waking up.
pub(super) fn ticker_flash(now: Instant, changed_at: Option<Instant>) -> bool {
  changed_at.is_some_and(|at| {
    now.saturating_duration_since(at).as_secs_f32() * 1000.0 < crate::config::SESSION_FLASH_MS
  })
}

/// Paints the `[ticker]` marquee for one layer.
///
/// The run is the configured items joined by the separator, with a trailing
/// separator at the seam. We measure it with the exact weight and tracking it
/// will be painted with, then draw two copies so the wrap has no visible gap.
/// The plate is the same one the clock uses, so a translucent glass strip is
/// free and the form still paints over a `behind` ticker.
pub(super) fn draw_ticker(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  layer: WidgetLayer,
) {
  let ticker = &scene.theme.ticker;
  if !ticker.enabled || ticker.layer != layer || (ticker.items.is_empty() && !ticker.session_first)
  {
    return;
  }
  let run = if ticker.session_first {
    ticker.run_with(Some(&scene.view.session))
  } else {
    ticker.run()
  };
  if run.is_empty() {
    return;
  }
  let scale = normalized_scale(scene.scale);
  let size = ticker.size * scale;
  if !size.is_finite() || size <= 0.0 {
    return;
  }
  let weight = if ticker.bold {
    Weight::BOLD
  } else {
    Weight::NORMAL
  };
  let (run_width, line_height) =
    text.measure_with(&run, size, 65_536.0, false, weight, ticker.tracking);
  if run_width <= 0.0 || line_height <= 0.0 {
    return;
  }
  let screen = (canvas.width() as f32, canvas.height() as f32);
  let margin_x = (ticker.offset.0 as f32 * scale).max(0.0);
  let margin_y = (ticker.offset.1 as f32 * scale).max(0.0);
  let strip_width = (screen.0 - margin_x * 2.0).max(1.0);
  let y = match ticker.anchor {
    TickerAnchor::Top => margin_y,
    TickerAnchor::Bottom => screen.1 - margin_y - line_height,
  }
  .clamp(0.0, (screen.1 - line_height).max(0.0));
  let strip = Rect::new(margin_x, y, strip_width, line_height);

  draw_widget_plate(
    canvas,
    strip,
    &ticker.plate,
    scene.theme,
    scale,
    ticker.plate.opacity,
    glass,
  );

  let opacity = ticker.plate.opacity.clamp(0.0, 1.0);
  if opacity <= 0.0 {
    return;
  }
  let flash = ticker_flash(scene.frame_now, scene.view.session_changed_at);
  let base_color = if flash {
    scene.theme.accent
  } else {
    ticker.color
  };
  let color = base_color.with_alpha((f32::from(base_color.a) * opacity).round() as u8);
  let style = if ticker.bold {
    Style::new(size, color).bold().tracking(ticker.tracking)
  } else {
    Style::new(size, color).tracking(ticker.tracking)
  };
  let beat_speed = if ticker.pulse_on_beat && scene.motion.beat_ms > 0.0 {
    ticker_beat_speed(scene.motion.beat_phase)
  } else {
    1.0
  };
  let shift = ticker_offset(
    run_width,
    ticker.speed * scale * beat_speed,
    ambient_elapsed(scene),
  );
  let clip = Rect::new(strip.x, 0.0, strip.w, screen.1);
  let start = strip.x - shift;
  // Two copies are enough: the second one enters from the right exactly as the
  // first one leaves on the left, and the clip drops the rest.
  for copy in 0..2 {
    let x = start + run_width * copy as f32;
    text.draw_line(
      canvas,
      &run,
      style,
      Area::new(x, strip.y, run_width.max(1.0)).clip(clip),
    );
  }
}
