//! Field pass for the renderer.

use super::*;

/// A thin accent outline while a widget is hovered or being dragged. The
/// outline sits just outside the widget so it never covers text.
pub(super) fn draw_drag_highlight(
  canvas: &mut Canvas,
  rect: Rect,
  id: &str,
  scene: &Scene<'_>,
  accent: Rgb,
  scale: f32,
) {
  let active = scene.drag.is_active(id);
  let hovered = scene.drag.is_hovered(id);
  if !active && !hovered {
    return;
  }
  let alpha = if active { 170 } else { 90 };
  canvas.stroke(
    rect.inset(-4.0 * scale),
    accent.with_alpha(alpha),
    (14.0 * scale).min(rect.h / 2.0 + 4.0 * scale),
    1.5 * scale,
  );
}

/// Paints an optional label and one input field. Returns the vertical space
/// consumed and the field rectangle, which the window uses for hit testing.
#[allow(clippy::too_many_arguments)]
/// Age of the keystroke pop in milliseconds, or `None` when it is off or has
/// already settled. Keeping the split here means the field painter only has to
/// deal with a time value and never with `Instant`.
pub(super) fn key_pop_age(
  scene: &Scene<'_>,
  field: &crate::config::FieldConfig,
  focused: bool,
) -> Option<f32> {
  if !focused || field.key_pop_ms <= 0.0 {
    return None;
  }
  let at = scene.view.typed_at?;
  let age = scene.frame_now.saturating_duration_since(at).as_secs_f32() * 1000.0;
  (age < field.key_pop_ms).then_some(age)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_field_block(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  config: &crate::config::FieldConfig,
  left: f32,
  top: f32,
  width: f32,
  scale: f32,
  value: &str,
  focused: bool,
  cursor: usize,
  caret_visible: bool,
  key_pop_age: Option<f32>,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
  focus: Rgb,
  clip: Rect,
) -> (f32, Rect) {
  let label = label_height(
    text,
    &config.label,
    label_size(scene.theme, config, scale),
    width,
    config.label_tracking,
  );
  let label_gap = if label > 0.0 {
    scene.theme.login.gap * scale * 0.35
  } else {
    0.0
  };
  let height = field_height(scene.theme, config, scale);
  let field_rect = Rect::new(left, top + label + label_gap, width, height);
  let used = label + label_gap + height;
  let pad = config.bracket_gap.max(config.icon_size) * scale + 8.0 * scale;
  let bounds = Rect::new(left, top, width, used).inset(-pad);
  paint_rotated(
    canvas,
    bounds,
    field_rect.center(),
    config.rotation,
    |layer| {
      draw_field_block_inner(
        layer,
        text,
        scene,
        config,
        left,
        top,
        width,
        scale,
        value,
        focused,
        cursor,
        caret_visible,
        key_pop_age,
        primary,
        muted,
        accent,
        focus,
        clip,
      );
    },
  );
  (used, field_rect)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_field_block_inner(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  config: &crate::config::FieldConfig,
  left: f32,
  top: f32,
  width: f32,
  scale: f32,
  value: &str,
  focused: bool,
  cursor: usize,
  caret_visible: bool,
  key_pop_age: Option<f32>,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
  focus: Rgb,
  clip: Rect,
) {
  let mut y = top;
  if !config.label.is_empty() {
    let label_size = label_size(scene.theme, config, scale);
    let (_, label_h) = text.measure_with(
      &config.label,
      label_size,
      width,
      false,
      Weight::NORMAL,
      config.label_tracking,
    );
    text.draw_line(
      canvas,
      &config.label,
      Style::new(label_size, config.label_color.unwrap_or(muted)).tracking(config.label_tracking),
      Area::new(left, y, width).clip(clip),
    );
    y += label_h + scene.theme.login.gap * scale * 0.35;
  }
  let height = field_height(scene.theme, config, scale);
  let rect = Rect::new(left, y, width, height);
  draw_field(
    canvas,
    text,
    scene.theme,
    scale,
    &Field {
      rect,
      value,
      focused,
      cursor,
      caret_visible,
      key_pop_age,
      focus_t: scene.motion.focus_t,
      primary,
      muted,
      accent,
      focus,
      config,
    },
  );
}

/// Everything needed to paint one input field. Grouped so the function does
/// not grow a positional argument for every property.
pub(super) struct Field<'a> {
  pub(super) rect: Rect,
  pub(super) value: &'a str,
  pub(super) focused: bool,
  pub(super) cursor: usize,
  pub(super) caret_visible: bool,
  /// Milliseconds since the last character landed, when the pop is active.
  pub(super) key_pop_age: Option<f32>,
  pub(super) focus_t: f32,
  pub(super) primary: Rgb,
  pub(super) muted: Rgb,
  pub(super) accent: Rgb,
  pub(super) focus: Rgb,
  pub(super) config: &'a crate::config::FieldConfig,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_field(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  theme: &Theme,
  scale: f32,
  field: &Field<'_>,
) {
  let base = field_size(theme, field.config, scale);
  let rect = field.rect;
  let radius = if field.config.radius > 0.0 {
    field.config.radius * scale
  } else {
    base * 0.55
  };
  if field.config.bracket_gap > 0.0 {
    let bracket_color = field
      .config
      .bracket
      .unwrap_or_else(|| theme.shadow.with_alpha(220));
    draw_bracket(
      canvas,
      rect.inset(-field.config.bracket_gap * scale),
      bracket_color,
      (1.4 * scale).max(1.0),
    );
  }
  let background = field
    .config
    .background
    .unwrap_or_else(|| theme.tint.with_alpha(110));
  if let Some(end) = field.config.background_end {
    canvas.horizontal_gradient_stops(rect, radius, &[(0.0, background), (1.0, end)]);
  } else {
    canvas.fill(rect, background, radius);
  }

  let focus_color = field.config.focus.unwrap_or(field.focus);
  let focus = if field.focused {
    0.35 + 0.65 * field.focus_t.clamp(0.0, 1.0)
  } else {
    0.0
  };
  let underline = field.config.underline;
  let rule_width = |focus: f32| {
    let base = if field.config.underline_size > 0.0 {
      field.config.underline_size * scale
    } else {
      scale
    };
    (base * (1.0 + 1.2 * focus)).max(1.0)
  };
  if let Some(color) = underline {
    let thickness = rule_width(0.0);
    canvas.fill(
      Rect::new(rect.x, rect.bottom() - thickness, rect.w, thickness),
      color,
      0.0,
    );
  }
  if focus > 0.0 {
    if underline.is_some() {
      // A line field does not need a box on focus. Grow the rule and add a
      // soft underglow so the active input is obvious without breaking the
      // cardless composition.
      let thickness = rule_width(focus);
      canvas.fill(
        Rect::new(
          rect.x,
          rect.bottom() - thickness - 2.0 * scale,
          rect.w,
          thickness + 2.0 * scale,
        ),
        focus_color.with_alpha((110.0 * focus) as u8),
        0.0,
      );
      canvas.fill(
        Rect::new(rect.x, rect.bottom() - thickness, rect.w, thickness),
        focus_color.with_alpha((150.0 + 105.0 * focus) as u8),
        0.0,
      );
    } else {
      canvas.stroke(
        rect.inset(-2.0),
        focus_color.with_alpha((70.0 * focus) as u8),
        radius + 2.0,
        2.0,
      );
      canvas.stroke(
        rect,
        focus_color.with_alpha((120.0 + 135.0 * focus) as u8),
        radius,
        1.0 + focus,
      );
    }
  } else if underline.is_none() {
    let border = field
      .config
      .border
      .unwrap_or_else(|| theme.border.with_alpha(28));
    canvas.stroke(rect, border, radius, 1.0);
  }

  let inset = if field.config.padding > 0.0 {
    field.config.padding * scale
  } else {
    base * 0.9
  };
  let mut text_x = rect.x + inset;
  if !field.config.icon.is_empty() {
    let icon_size = if field.config.icon_size > 0.0 {
      field.config.icon_size * scale
    } else {
      base * 1.1
    };
    let icon_color = field.config.icon_color.unwrap_or(field.primary);
    draw_field_icon(
      canvas,
      &field.config.icon,
      text_x,
      rect.center().1,
      icon_size,
      icon_color,
    );
    let gap = if field.config.icon_gap > 0.0 {
      field.config.icon_gap * scale
    } else {
      base * 0.55
    };
    text_x += icon_size + gap;
  }
  let empty = field.value.is_empty();
  let shown = if empty {
    field.config.placeholder.as_str()
  } else {
    field.value
  };
  let color = if empty {
    field.config.placeholder_color.unwrap_or(field.muted)
  } else {
    field.config.text.unwrap_or(field.primary)
  };

  // Centre the actual ink, not the line box. The line box carries the font's
  // ascent and descent, so centring it leaves the glyphs a pixel or two above
  // a centred icon. Ink bounds make the placeholder, the typed text, and the
  // icon share one optical centre for any family.
  let (ink_top, ink_bottom) = text.ink_bounds(shown, base, cosmic_text::Weight::NORMAL, 0.0);
  let text_y = rect.center().1 - (ink_top + ink_bottom) / 2.0;
  let text_area =
    Area::new(text_x, text_y, (rect.right() - inset - text_x).max(1.0)).clip(rect.inset(2.0));
  if let Some(age) = field.key_pop_age.filter(|_| !empty) {
    // The pop draws the prefix as its own run and the newest character with a
    // larger, warmer style. Splitting on a char boundary is what keeps a
    // multibyte glyph intact; slicing by bytes would panic on the wrong input.
    if let Some((prefix, last)) = split_last_char(shown) {
      if !prefix.is_empty() {
        text.draw_line(canvas, prefix, Style::new(base, color), text_area);
      }
      let t = (age / field.config.key_pop_ms).clamp(0.0, 1.0);
      let settle = ease_out_expo(t);
      let scale = 1.0 + (field.config.key_pop_scale - 1.0) * (1.0 - settle);
      let pop_color = field
        .config
        .key_pop_color
        .unwrap_or(field.accent)
        .mix(color, settle);
      // Advance keeps the popped glyph in the same cell it would occupy at
      // rest, so the caret does not jump while the character grows and
      // settles.
      let advance = text.line_width(prefix, base);
      let pop_y = text_y - (base * (scale - 1.0)) * 0.32;
      let pop_area = Area::new(
        text_x + advance,
        pop_y,
        (rect.right() - text_x - advance).max(1.0),
      )
      .clip(rect.inset(2.0));
      text.draw_line(canvas, last, Style::new(base * scale, pop_color), pop_area);
    } else {
      text.draw_line(canvas, shown, Style::new(base, color), text_area);
    }
  } else {
    text.draw_line(canvas, shown, Style::new(base, color), text_area);
  }

  if field.focused && field.caret_visible {
    let prefix: String = shown.chars().take(field.cursor).collect();
    let caret_x = text_x + text.line_width(&prefix, base);
    let caret_color = field.config.caret.unwrap_or(field.accent);
    let caret_width = if field.config.caret_width > 0.0 {
      field.config.caret_width * scale
    } else {
      match field.config.caret_style {
        CaretStyle::Bar => (base * 0.12).max(1.0),
        // A block stands in for one character cell, so it is much wider than
        // a bar. The floor of two pixels keeps it visible on tiny fields.
        CaretStyle::Block => (base * 0.55).max(2.0),
      }
    };
    // The whole caret has to fit inside the field, not just its left edge.
    // A block is wide enough that the old left-edge check would let it paint
    // over the border, so use the right edge of the caret here.
    if caret_x + caret_width <= rect.right() - inset * 0.5 {
      let caret_height = (ink_bottom - ink_top).max(base * 0.8) + base * 0.1;
      let caret_radius = match field.config.caret_style {
        CaretStyle::Bar => radius,
        CaretStyle::Block => 0.0,
      };
      canvas.fill(
        Rect::new(
          caret_x,
          rect.center().1 - caret_height / 2.0,
          caret_width,
          caret_height,
        ),
        caret_color,
        caret_radius,
      );
    }
  }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_session_row(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  view: &View,
  theme: &Theme,
  base: f32,
  area: Rect,
  clip: Option<Rect>,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
  error: Rgb,
) {
  let session = &theme.login.session;
  let size = base * 0.9;
  let y = area.y;
  let mut cursor_x = area.x;
  let label = if session.label.is_empty() {
    String::new()
  } else {
    format!("{}  ", session.label)
  };
  if !label.is_empty() {
    text.draw_line(
      canvas,
      &label,
      Style::new(size, muted),
      Area {
        clip,
        ..Area::new(cursor_x, y, area.w)
      },
    );
    cursor_x += text.line_width(&label, size);
  }

  let (name, color) = if view.session.is_empty() {
    (session.empty.as_str(), error)
  } else {
    (view.session.as_str(), accent)
  };
  let remaining = (area.w - (cursor_x - area.x)).max(0.0);
  text.draw_line(
    canvas,
    name,
    Style::new(size, color),
    Area {
      clip,
      ..Area::new(cursor_x, y, remaining)
    },
  );
  cursor_x += text.line_width(name, size);

  if view.session_count > 1 && theme.login.show_picker && !session.change_hint.is_empty() {
    let hint = format!("    {}", session.change_hint);
    let remaining = (area.w - (cursor_x - area.x)).max(0.0);
    text.draw_line(
      canvas,
      &hint,
      Style::new(size, muted.with_alpha(170)),
      Area {
        clip,
        ..Area::new(cursor_x, y, remaining)
      },
    );
  }
  let _ = primary;
}

pub(super) fn hint_text(view: &View, login: &crate::config::LoginConfig) -> String {
  if !login.show_hints {
    return String::new();
  }
  if view.busy {
    return if login.hints.busy.is_empty() {
      login.busy_text.clone()
    } else {
      login.hints.busy.clone()
    };
  }
  let hint = match view.focus {
    crate::ui::view::Focus::Username => &login.hints.username,
    crate::ui::view::Focus::Password => &login.hints.password,
    crate::ui::view::Focus::Picker => &login.hints.picker,
  };
  hint.clone()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_picker(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  screen: (f32, f32),
  scale: f32,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
) {
  // Dim the login form so it is obvious the picker owns the keyboard.
  canvas.fill(
    Rect::new(0.0, 0.0, screen.0, screen.1),
    Rgb::rgba(0, 0, 0, 150),
    0.0,
  );

  let base = scene.theme.font_size.max(10.0) * scale;
  let row_h = base * 2.3;
  let pad = base * 1.2 * scene.theme.spacing;
  let min_width = 260.0 * scale;
  let width =
    (scene.theme.login.width * scale).clamp(min_width, (screen.0 - base * 3.0).max(min_width));
  let rows = scene.view.picker.len().min(MAX_PICKER_ROWS);
  let header_h = base * 1.6;
  let height = pad * 2.0 + header_h + rows.max(1) as f32 * row_h;
  let x = (screen.0 - width) / 2.0;
  let y = ((screen.1 - height) / 2.0).max(base);
  let panel = Rect::new(x, y, width, height);

  glass.panel(
    canvas,
    panel,
    scene.theme.corner_radius * scale,
    scene.theme,
    1.0,
  );

  let left = panel.x + pad;
  let inner = panel.w - pad * 2.0;
  if !scene.theme.login.picker_title.is_empty() {
    text.draw_line(
      canvas,
      &scene.theme.login.picker_title,
      Style::new(base * 0.85, muted),
      Area::new(left, panel.y + pad, inner),
    );
  }

  if scene.view.picker.is_empty() {
    text.draw_line(
      canvas,
      &scene.theme.login.session.empty,
      Style::new(base, scene.theme.error_on(scene.background_color)),
      Area::new(left, panel.y + pad + header_h, inner),
    );
    return;
  }

  // Keep the selected row visible without needing a scrollbar. With more than
  // ten sessions the window slides, and the ends simply stay put.
  let first = scene
    .view
    .picker_selected
    .saturating_sub(rows.saturating_sub(1));
  let mut row_y = panel.y + pad + header_h;
  for (offset, name) in scene.view.picker.iter().enumerate().skip(first).take(rows) {
    let row = Rect::new(left, row_y, inner, row_h);
    if offset == scene.view.picker_selected {
      canvas.fill(row, accent.with_alpha(45), base * 0.45);
      canvas.stroke(row, accent.with_alpha(80), base * 0.45, 1.0);
    }
    text.draw_line(
      canvas,
      name,
      Style::new(
        base,
        if offset == scene.view.picker_selected {
          primary
        } else {
          muted
        },
      ),
      Area::new(
        left + base * 0.6,
        row_y + (row_h - base * 1.35) / 2.0,
        inner - base * 1.2,
      )
      .clip(row),
    );
    row_y += row_h;
  }
}

pub(super) fn draw_badge(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  scale: f32,
  accent: Rgb,
) {
  let login = &scene.theme.login;
  if !login.show_dry_run_badge || login.dry_run_badge.is_empty() {
    return;
  }
  let size = scene.theme.font_size * 0.8 * scale;
  let label = login.dry_run_badge.as_str();
  let width = text.line_width(label, size);
  let pad = size * 0.6;
  let margin = size * 1.1;
  let radius = size * 0.4;
  let rect = Rect::new(margin, margin, width + pad * 2.0, size * 1.6 + pad);
  glass.panel(canvas, rect, radius, scene.theme, 1.0);
  text.draw_line(
    canvas,
    label,
    Style::new(size, accent),
    Area::new(rect.x + pad, rect.y + (rect.h - size * 1.35) / 2.0, rect.w),
  );
}

pub(super) fn draw_warnings(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  scene: &Scene<'_>,
  screen: (f32, f32),
  scale: f32,
  error: Rgb,
) {
  let mut messages = Vec::new();
  if let Some(warning) = &scene.view.warning {
    messages.push(warning.as_str());
  }
  if let Some(warning) = scene.asset_warning {
    messages.push(warning);
  }
  if messages.is_empty() {
    return;
  }

  let size = scene.theme.font_size * 0.82 * scale;
  let margin = scene.theme.font_size * 1.9 * scale;
  let width = (screen.0 - margin * 2.0).max(1.0);
  let mut y = screen.1 - margin * 0.5;
  for message in messages.iter().rev() {
    let (_, height) = text.measure(message, size, width, true);
    y -= height;
    text.draw(
      canvas,
      message,
      Style::new(size, error.with_alpha(230)),
      Area::new(margin, y, width).align(Align::Center),
    );
    y -= size * 0.3;
  }
}
