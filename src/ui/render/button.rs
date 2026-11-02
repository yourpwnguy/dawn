//! Button pass for the renderer.

use super::*;

/// The target-style submit button: a solid fill inside a broken dark bracket.
/// The text is separately configurable so a theme can keep the same geometry
/// with its own language.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_login_button(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  config: &ButtonConfig,
  rect: Rect,
  card_scale: f32,
  primary: Rgb,
  accent: Rgb,
  clip: Rect,
) {
  let hovered = scene.drag.is_hovered("login.button");
  let reduced = scene.theme.motion.reduced || scene.theme.low_power;
  let duration = Duration::from_secs_f32((config.hover_ms / 1000.0).max(0.0));
  let hover_t = if reduced {
    f32::from(u8::from(hovered))
  } else {
    scene
      .drag
      .hover_t("login.button", scene.frame_now, duration)
  };
  let ease = ease_out_cubic(hover_t);
  let age = scene.drag.hover_elapsed("login.button", scene.frame_now);

  let scale = 1.0 + (config.hover_scale - 1.0) * ease;
  let lift = config.hover_lift * ease * card_scale;
  let center = rect.center();
  let width = rect.w * scale;
  let height = rect.h * scale;
  let animated = Rect::new(
    center.0 - width / 2.0,
    center.1 - height / 2.0 - lift,
    width,
    height,
  );
  let bounds =
    animated.inset(-(config.bracket_gap.max(config.glow_size) * card_scale + 10.0 * card_scale));
  paint_rotated(
    canvas,
    bounds,
    animated.center(),
    config.rotation,
    |layer| {
      draw_login_button_body(
        layer, text, scene, config, animated, card_scale, scale, primary, accent, clip, hovered,
        ease, age, reduced,
      );
    },
  );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_login_button_body(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  config: &ButtonConfig,
  rect: Rect,
  card_scale: f32,
  scale: f32,
  primary: Rgb,
  accent: Rgb,
  clip: Rect,
  hovered: bool,
  ease: f32,
  age: Duration,
  reduced: bool,
) {
  let base = config.background.unwrap_or(accent);
  let base_end = config.background_end.unwrap_or(base);
  let hover = config
    .hover_background
    .unwrap_or_else(|| base.lighten(0.16));
  let hover_end = config
    .hover_background_end
    .unwrap_or_else(|| base_end.lighten(0.16));
  let radius = config.radius * card_scale;

  let glow_size = if config.effect == ButtonEffect::Glow {
    config.glow_size.max(8.0)
  } else {
    config.glow_size
  };
  if glow_size > 0.0 && ease > 0.0 {
    let glow = config.glow.unwrap_or(hover);
    let pulse = if matches!(config.effect, ButtonEffect::Pulse | ButtonEffect::Glow) && !reduced {
      0.55
        + 0.45
          * (cycle(
            age,
            Duration::from_secs_f32((config.effect_ms / 1000.0).max(0.1)),
          ) * std::f32::consts::TAU)
            .sin()
    } else {
      1.0
    };
    let size = glow_size * card_scale;
    for (index, falloff) in [(1.0f32, 0.30f32), (0.66, 0.22), (0.33, 0.14)] {
      let expand = size * index;
      let alpha = (255.0 * falloff * ease * pulse) as u8;
      canvas.fill(rect.inset(-expand), glow.with_alpha(alpha), radius + expand);
    }
  }

  if config.bracket_gap > 0.0 {
    let color = config.bracket.unwrap_or(scene.theme.shadow);
    draw_bracket(
      canvas,
      rect.inset(-config.bracket_gap * card_scale),
      color,
      (1.4 * card_scale).max(1.0),
    );
  }

  draw_button_fill(
    canvas,
    rect,
    radius,
    config.gradient_angle,
    base,
    base_end,
    config.background_end.is_some(),
  );

  if config.hover_direction == HoverReveal::Fade {
    let start = base.mix(hover, ease);
    let end = base_end.mix(hover_end, ease);
    draw_button_fill(
      canvas,
      rect,
      radius,
      config.gradient_angle,
      start,
      end,
      config.background_end.is_some() || config.hover_background_end.is_some(),
    );
  } else if ease > 0.0 {
    let (angle, stops) = reveal_stops(config.hover_direction, ease, hover, hover_end);
    canvas.linear_gradient_stops(rect, radius, angle, &stops);
  }

  if hovered && !reduced {
    draw_button_effect(canvas, config, rect, radius, ease, age);
  }

  let size = if config.font_size > 0.0 {
    config.font_size * card_scale * scale
  } else {
    scene.theme.font_size.max(10.0) * card_scale * 1.05 * scale
  };
  let text_color = config.text_color.unwrap_or(primary).mix(
    config
      .hover_text_color
      .unwrap_or_else(|| config.text_color.unwrap_or(primary)),
    ease,
  );
  let mut style = Style::new(size, text_color);
  if config.bold {
    style = style.bold();
  }
  style = style.tracking(config.tracking);
  let (ink_top, ink_bottom) = text.ink_bounds(&config.text, size, style.weight, style.tracking);
  text.draw_line(
    canvas,
    &config.text,
    style,
    Area::new(
      rect.x,
      rect.center().1 - (ink_top + ink_bottom) / 2.0,
      rect.w,
    )
    .align(Align::Center)
    .clip(clip),
  );
}

pub(super) fn draw_button_fill(
  canvas: &mut Canvas,
  rect: Rect,
  radius: f32,
  angle: f32,
  start: Rgb,
  end: Rgb,
  gradient: bool,
) {
  if gradient && start != end {
    canvas.linear_gradient_stops(rect, radius, angle, &[(0.0, start), (1.0, end)]);
  } else {
    canvas.fill(rect, start, radius);
  }
}

/// Directional hover wipes. The edge is feathered so it reads as paint being
/// poured in rather than a rectangle sliding across the button.
pub(super) fn reveal_stops(
  direction: HoverReveal,
  progress: f32,
  color: Rgb,
  end_color: Rgb,
) -> (f32, Vec<(f32, Rgb)>) {
  let t = progress.clamp(0.0, 1.0);
  let feather = 0.08;
  let clear = color.with_alpha(0);
  let clear_end = end_color.with_alpha(0);
  let mut stops = match direction {
    HoverReveal::Left => vec![
      (0.0, color),
      (t, end_color),
      ((t + feather).min(1.0), clear_end),
      (1.0, clear),
    ],
    HoverReveal::Right => vec![
      (0.0, clear),
      ((1.0 - t - feather).max(0.0), clear_end),
      (1.0 - t, end_color),
      (1.0, color),
    ],
    HoverReveal::Up => vec![
      (0.0, color),
      (t, end_color),
      ((t + feather).min(1.0), clear_end),
      (1.0, clear),
    ],
    HoverReveal::Down => vec![
      (0.0, clear),
      ((1.0 - t - feather).max(0.0), clear_end),
      (1.0 - t, end_color),
      (1.0, color),
    ],
    HoverReveal::Center => {
      let edge = (t / 2.0).min(0.5);
      vec![
        (0.0, color),
        (edge, end_color),
        ((edge + feather).min(1.0), clear_end),
        ((1.0 - edge - feather).max(0.0), clear_end),
        (1.0 - edge, end_color),
        (1.0, color),
      ]
    }
    HoverReveal::Fade => vec![(0.0, color), (1.0, end_color)],
  };
  stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
  let angle = match direction {
    HoverReveal::Left | HoverReveal::Center | HoverReveal::Fade => 0.0,
    HoverReveal::Right => 180.0,
    HoverReveal::Up => 270.0,
    HoverReveal::Down => 90.0,
  };
  (angle, stops)
}

pub(super) fn draw_button_effect(
  canvas: &mut Canvas,
  config: &ButtonConfig,
  rect: Rect,
  radius: f32,
  ease: f32,
  age: Duration,
) {
  let period = Duration::from_secs_f32((config.effect_ms / 1000.0).max(0.1));
  let phase = cycle(age, period);
  let color = config
    .effect_color
    .unwrap_or(Rgb::solid(255, 255, 255))
    .with_alpha((255.0 * config.effect_strength * ease) as u8);
  match config.effect {
    ButtonEffect::None => {}
    ButtonEffect::Sweep | ButtonEffect::Sheen => {
      let width = if config.effect == ButtonEffect::Sheen {
        0.05
      } else {
        0.16
      };
      let center = -0.25 + phase * 1.5;
      let stops = [
        (0.0, color.with_alpha(0)),
        ((center - width).max(0.0), color.with_alpha(0)),
        (center.clamp(0.0, 1.0), color),
        ((center + width).min(1.0), color.with_alpha(0)),
        (1.0, color.with_alpha(0)),
      ];
      canvas.linear_gradient_stops(rect, radius, config.effect_angle, &stops);
    }
    ButtonEffect::Glow => {}
    ButtonEffect::Pulse => {
      let wave = 0.5 + 0.5 * (phase * std::f32::consts::TAU).sin();
      let alpha = (255.0 * config.effect_strength * ease * wave * 0.35) as u8;
      canvas.fill(rect, color.with_alpha(alpha), radius);
    }
    ButtonEffect::Corners => {
      let grow = 0.5 + 0.5 * (phase * std::f32::consts::TAU).sin();
      let length = (8.0 + 10.0 * grow) * rect.h.max(1.0) / 48.0;
      draw_corner_marks(canvas, rect.inset(-4.0), color, length.max(4.0));
    }
  }
}

/// Four L-shaped marks that breathe outward around the button. This is the
/// "target acquired" cue, and it is deliberately drawn outside the fill so it
/// never makes the label harder to read.
pub(super) fn draw_corner_marks(canvas: &mut Canvas, rect: Rect, color: Rgb, length: f32) {
  let length = length.min(rect.w / 3.0).min(rect.h / 3.0);
  let width = (length * 0.12).max(1.0);
  for (x, y, sx, sy) in [
    (rect.x, rect.y, 1.0, 1.0),
    (rect.right(), rect.y, -1.0, 1.0),
    (rect.x, rect.bottom(), 1.0, -1.0),
    (rect.right(), rect.bottom(), -1.0, -1.0),
  ] {
    canvas.line((x, y), (x + sx * length, y), color, width);
    canvas.line((x, y), (x, y + sy * length), color, width);
  }
}

/// The broken targeting bracket used by hero fields and buttons. The
/// top and bottom edges are split in the middle, while the sides stay solid.
pub(super) fn draw_bracket(canvas: &mut Canvas, rect: Rect, color: Rgb, width: f32) {
  if rect.w <= 0.0 || rect.h <= 0.0 {
    return;
  }
  let gap = (rect.w * 0.28).min(rect.w * 0.6);
  let left_end = rect.x + (rect.w - gap) / 2.0;
  let right_start = left_end + gap;
  canvas.line((rect.x, rect.y), (left_end, rect.y), color, width);
  canvas.line((right_start, rect.y), (rect.right(), rect.y), color, width);
  canvas.line(
    (rect.x, rect.bottom()),
    (left_end, rect.bottom()),
    color,
    width,
  );
  canvas.line(
    (right_start, rect.bottom()),
    (rect.right(), rect.bottom()),
    color,
    width,
  );
  canvas.line((rect.x, rect.y), (rect.x, rect.bottom()), color, width);
  canvas.line(
    (rect.right(), rect.y),
    (rect.right(), rect.bottom()),
    color,
    width,
  );
}

/// Draws the leading user/lock glyph. These are vector shapes instead of font
/// glyphs so the icon stays crisp at every card_scale and never depends on the
/// configured family.
pub(super) fn draw_field_icon(
  canvas: &mut Canvas,
  icon: &str,
  x: f32,
  center_y: f32,
  size: f32,
  color: Rgb,
) {
  if size <= 0.0 {
    return;
  }
  // Both glyphs are laid out inside the same -0.38..+0.38 box. That symmetry
  // is what keeps the icon on the same optical line as the centred text.
  match icon {
    "user" => {
      canvas.fill_circle((x + size * 0.5, center_y - size * 0.22), size * 0.18, color);
      canvas.fill(
        Rect::new(
          x + size * 0.16,
          center_y + size * 0.02,
          size * 0.68,
          size * 0.36,
        ),
        color,
        size * 0.2,
      );
    }
    "lock" => {
      canvas.stroke_circle(
        (x + size * 0.5, center_y - size * 0.17),
        size * 0.2,
        color,
        (size * 0.1).max(1.0),
      );
      canvas.fill(
        Rect::new(
          x + size * 0.18,
          center_y + size * 0.03,
          size * 0.64,
          size * 0.34,
        ),
        color,
        size * 0.08,
      );
    }
    _ => {}
  }
}
