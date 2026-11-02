//! Media pass for the renderer.

use super::*;

/// The album art a layout should paint. `Disc` prefers the circular pixmap
/// masked once in [`crate::ui::media`], but falls back to the square crop when
/// a snapshot was built without one (tests and older cached tracks).
pub(super) fn media_art(
  snapshot: &MediaSnapshot,
  show_art: bool,
  art: MediaArt,
) -> Option<&tiny_skia::Pixmap> {
  if !show_art {
    return None;
  }
  match art {
    MediaArt::None => None,
    MediaArt::Cover => snapshot.art.as_deref(),
    MediaArt::Disc => snapshot.art_disc.as_deref().or(snapshot.art.as_deref()),
  }
}

/// Height a progress style needs inside its layout. A wave needs vertical room
/// to be visible; a line stays a hairline.
pub(super) fn progress_height(style: MediaProgress, size: f32, scale: f32) -> f32 {
  match style {
    MediaProgress::Line => 2.0 * scale,
    MediaProgress::Segments => 4.0 * scale,
    MediaProgress::Wave => size * 0.8,
  }
}

/// The rectangle a media widget will occupy. Split from `draw_media` so the
/// rotation layer can size itself before painting. The layout switch lives
/// here and in `draw_media`; keeping the two matches in the same file is what
/// stops the hit region from drifting away from the pixels.
#[allow(clippy::too_many_arguments)]
pub(super) fn media_bounds(
  text: &mut TextEngine,
  snapshot: &MediaSnapshot,
  anchor: crate::layout::Anchor,
  offset: (i32, i32),
  viewport: Rect,
  scale: f32,
  logical_size: f32,
  show_art: bool,
  show_before_login: bool,
  authenticated: bool,
  look: MediaLook,
) -> Rect {
  let size = logical_size * scale;
  let screen = (viewport.w, viewport.h);
  if (!authenticated && !show_before_login) || !snapshot.available {
    let label = "No media";
    let (width, height) = text.measure(label, size * 0.85, 320.0 * scale, false);
    let (x, y) = anchor.place(screen, (width, height), offset);
    return Rect::new(viewport.x + x, viewport.y + y, width, height);
  }
  let art_visible = media_art(snapshot, show_art, look.art).is_some();
  let (width, height) = match look.layout {
    MediaLayout::Card => {
      let art_size = if art_visible { size * 3.0 } else { 0.0 };
      let mut height = art_size.max(size * 2.8);
      if look.controls {
        height = height.max(size * 4.2);
      }
      ((300.0 * scale).max(art_size + size * 12.0), height)
    }
    MediaLayout::Compact => {
      let height = if look.controls {
        size * 3.4
      } else {
        size * 2.6
      };
      (320.0 * scale, height)
    }
    MediaLayout::Vinyl => {
      let disc = if art_visible { size * 4.0 } else { 0.0 };
      let mut height = disc + size * 3.4;
      if look.controls {
        height += size * 1.2;
      }
      ((size * 5.0).max(disc + size * 0.8), height)
    }
  };
  let (x, y) = anchor.place(screen, (width, height), offset);
  Rect::new(viewport.x + x, viewport.y + y, width, height)
}

/// The music progress indicator. None of these styles reads audio samples:
/// the wave is a deterministic sine that only encodes the playback fraction,
/// so it looks alive without claiming to be a real spectrum.
pub(super) fn draw_progress(
  canvas: &mut Canvas,
  rect: Rect,
  fraction: f32,
  style: MediaProgress,
  accent: Rgb,
  muted: Rgb,
  scale: f32,
) {
  let fraction = fraction.clamp(0.0, 1.0);
  match style {
    MediaProgress::Line => {
      canvas.fill(rect, muted.with_alpha(50), 1.0);
      canvas.fill(
        Rect::new(rect.x, rect.y, rect.w * fraction, rect.h),
        accent,
        1.0,
      );
    }
    MediaProgress::Segments => {
      const BLOCKS: usize = 12;
      let gap = 3.0 * scale;
      let block_width = ((rect.w - gap * (BLOCKS - 1) as f32) / BLOCKS as f32).max(1.0);
      let filled = ((fraction * BLOCKS as f32).round() as usize).min(BLOCKS);
      for index in 0..BLOCKS {
        let x = rect.x + index as f32 * (block_width + gap);
        let color = if index < filled {
          accent
        } else {
          muted.with_alpha(70)
        };
        canvas.fill(
          Rect::new(x, rect.y, block_width, rect.h.max(4.0)),
          color,
          1.0,
        );
      }
    }
    MediaProgress::Wave => {
      let amplitude = (rect.h * 0.5).max(3.0);
      let baseline = rect.y + rect.h * 0.5;
      let step = (4.0 * scale).max(1.0);
      let steps = (rect.w / step).ceil().max(2.0) as usize;
      let mut previous = None;
      for index in 0..=steps {
        let progress = index as f32 / steps as f32;
        let point = (
          rect.x + progress * rect.w,
          baseline + (progress * TAU * 3.0).sin() * amplitude,
        );
        if let Some(from) = previous {
          let color = if progress <= fraction {
            accent
          } else {
            muted.with_alpha(80)
          };
          canvas.line(from, point, color, 1.5 * scale);
        }
        previous = Some(point);
      }
    }
  }
}

/// The three transport glyphs and their click targets.
///
/// The glyph geometry is the same one the widget always drew. The new part is
/// that every centre also gets a button rectangle, returned in previous, play,
/// next order so the caller can push hit regions without guessing. A hovered
/// button gets a faint accent pill behind the glyph. The rectangles are always
/// returned; the caller decides whether they are really clickable, which is how
/// a rotated widget can keep the picture and still drop the hit regions.
pub(super) fn draw_controls(
  canvas: &mut Canvas,
  rect: Rect,
  size: f32,
  color: Rgb,
  accent: Rgb,
  playing: bool,
  hovered: [bool; 3],
) -> [Rect; 3] {
  let center_y = rect.y + rect.h * 0.5;
  let center_x = rect.x + rect.w * 0.5;
  let gap = size * 1.1;
  let half = rect.h * 0.45;
  let stroke = (size * 0.08).max(1.0);
  let button_w = (gap * 0.85).max(size * 0.7);
  let button_h = (rect.h * 1.15).max(size * 0.9);
  let buttons = [
    Rect::new(
      center_x - gap - button_w * 0.5,
      center_y - button_h * 0.5,
      button_w,
      button_h,
    ),
    Rect::new(
      center_x - button_w * 0.5,
      center_y - button_h * 0.5,
      button_w,
      button_h,
    ),
    Rect::new(
      center_x + gap - button_w * 0.5,
      center_y - button_h * 0.5,
      button_w,
      button_h,
    ),
  ];
  for (button, active) in buttons.iter().zip(hovered) {
    if active {
      canvas.fill(*button, accent.with_alpha(40), (size * 0.25).max(2.0));
    }
  }
  let colors = [
    if hovered[0] { accent } else { color },
    if hovered[1] { accent } else { color },
    if hovered[2] { accent } else { color },
  ];
  let triangle = |canvas: &mut Canvas, points: [(f32, f32); 3], color: Rgb| {
    for index in 0..3 {
      let from = points[index];
      let to = points[(index + 1) % 3];
      canvas.line(from, to, color, stroke);
    }
  };
  let previous_x = center_x - gap;
  canvas.fill(
    Rect::new(previous_x - half * 0.8, center_y - half, stroke, half * 2.0),
    colors[0],
    1.0,
  );
  triangle(
    canvas,
    [
      (previous_x + half * 0.55, center_y - half),
      (previous_x + half * 0.55, center_y + half),
      (previous_x - half * 0.45, center_y),
    ],
    colors[0],
  );

  if playing {
    canvas.fill(
      Rect::new(
        center_x - half * 0.45,
        center_y - half,
        half * 0.3,
        half * 2.0,
      ),
      colors[1],
      1.0,
    );
    canvas.fill(
      Rect::new(
        center_x + half * 0.15,
        center_y - half,
        half * 0.3,
        half * 2.0,
      ),
      colors[1],
      1.0,
    );
  } else {
    triangle(
      canvas,
      [
        (center_x - half * 0.4, center_y - half),
        (center_x - half * 0.4, center_y + half),
        (center_x + half * 0.65, center_y),
      ],
      colors[1],
    );
  }

  let next_x = center_x + gap;
  triangle(
    canvas,
    [
      (next_x - half * 0.55, center_y - half),
      (next_x - half * 0.55, center_y + half),
      (next_x + half * 0.45, center_y),
    ],
    colors[2],
  );
  canvas.fill(
    Rect::new(next_x + half * 0.55, center_y - half, stroke, half * 2.0),
    colors[2],
    1.0,
  );
  buttons
}

/// Circular progress used by the vinyl layout. Drawing the ring as short line
/// segments avoids needing an arc primitive on the canvas.
pub(super) fn draw_ring_progress(
  canvas: &mut Canvas,
  center: (f32, f32),
  radius: f32,
  fraction: f32,
  accent: Rgb,
  muted: Rgb,
  width: f32,
) {
  const STEPS: usize = 96;
  let fraction = fraction.clamp(0.0, 1.0);
  for index in 0..STEPS {
    let start = index as f32 / STEPS as f32 * TAU - TAU / 4.0;
    let end = (index + 1) as f32 / STEPS as f32 * TAU - TAU / 4.0;
    let color = if (index + 1) as f32 / STEPS as f32 <= fraction {
      accent
    } else {
      muted.with_alpha(70)
    };
    canvas.line(
      (
        center.0 + start.cos() * radius,
        center.1 + start.sin() * radius,
      ),
      (center.0 + end.cos() * radius, center.1 + end.sin() * radius),
      color,
      width,
    );
  }
}

/// What one media paint produced.
///
/// `bounds` is the widget rectangle the rotation layer and the drag region
/// already used. `controls` holds clickable previous, play, and next rectangles
/// in screen space, or `[None; 3]` when the widget has no metadata, controls
/// are off, or the widget is rotated. Keeping both in one return value means
/// the hit regions cannot drift from the pixels that were actually painted.
pub(super) struct MediaDraw {
  pub(super) bounds: Rect,
  pub(super) controls: [Option<Rect>; 3],
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_media(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  media_index: usize,
  snapshot: &MediaSnapshot,
  anchor: crate::layout::Anchor,
  offset: (i32, i32),
  viewport: Rect,
  scale: f32,
  logical_size: f32,
  show_art: bool,
  show_before_login: bool,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
  plate: Option<&WidgetPlate>,
  look: MediaLook,
  angle: f32,
) -> MediaDraw {
  let size = logical_size * scale;
  let rect = media_bounds(
    text,
    snapshot,
    anchor,
    offset,
    viewport,
    scale,
    logical_size,
    show_art,
    show_before_login,
    scene.view.authenticated,
    look,
  );
  // A greeter normally runs before the user's session exists. `show_before_login`
  // lets a desktop-session test or a single-user machine display the last
  // track; shared machines can turn it off and keep metadata off the pixels.
  if (!scene.view.authenticated && !show_before_login) || !snapshot.available {
    let label = "No media";
    paint_widget_backdrop(canvas, rect, scene.theme, scale, plate, glass);
    text.draw(
      canvas,
      label,
      Style::new(size * 0.85, muted.with_alpha(160)),
      Area::new(rect.x, rect.y, rect.w.max(1.0)),
    );
    return MediaDraw {
      bounds: rect,
      controls: [None; 3],
    };
  }

  // A rotated widget keeps its controls as decoration but exposes no clicks:
  // the hit math for a rotated button is easy to get subtly wrong, and a
  // transport button that fires from the wrong pixel is worse than a static
  // one. The same rule covers a motion sample that rotated the whole widget.
  let interactive = angle.abs() <= 0.01;
  let mut controls: [Option<Rect>; 3] = [None; 3];

  // Art tinting is opt-in and derived from pixels, never from metadata. The
  // progress bar and an existing plate border/glow pick up the same hue so the
  // widget reads as one object instead of a coloured bar on a grey pill.
  let art_tint = if look.tint_from_art {
    snapshot.art_tint
  } else {
    None
  };
  let accent = art_tint.unwrap_or(accent);
  let tinted_plate = plate.map(|plate| {
    let mut plate = *plate;
    if let Some(tint) = art_tint {
      plate.border = Some(tint.with_alpha(180));
      if plate.glow.is_some() && plate.glow_size > 0.0 {
        plate.glow = Some(tint);
      }
    }
    plate
  });
  let plate = tinted_plate.as_ref();
  let art = media_art(snapshot, show_art, look.art);
  paint_widget_backdrop(canvas, rect, scene.theme, scale, plate, glass);

  let (x, y) = (rect.x, rect.y);
  let width = rect.w;
  let playing = snapshot.playing;
  // Hover is resolved by the same ids the hit regions will use, so the glyph
  // and the click target cannot disagree about which button is under the
  // pointer. When controls are off we never even format the ids.
  let hovered = [
    look.controls && scene.drag.is_hovered(&format!("media:{media_index}:prev")),
    look.controls && scene.drag.is_hovered(&format!("media:{media_index}:play")),
    look.controls && scene.drag.is_hovered(&format!("media:{media_index}:next")),
  ];
  match look.layout {
    MediaLayout::Card => {
      let art_size = if art.is_some() { size * 3.0 } else { 0.0 };
      let mut text_x = x;
      if let Some(art) = art {
        canvas.image_in(art, Rect::new(x, y, art_size, art_size));
        text_x += art_size + size * 0.7;
      }
      let text_width = (width - (text_x - x)).max(1.0);
      text.draw_line(
        canvas,
        &snapshot.title,
        Style::new(
          size,
          primary.with_alpha(if snapshot.stale { 210 } else { 255 }),
        ),
        Area::new(text_x, y, text_width),
      );
      text.draw_line(
        canvas,
        &snapshot.artist,
        Style::new(size * 0.85, muted),
        Area::new(text_x, y + size * 1.25, text_width),
      );
      if snapshot.stale {
        text.draw_line(
          canvas,
          "Last played",
          Style::new(size * 0.7, muted.with_alpha(150)),
          Area::new(text_x, y + size * 2.25, text_width),
        );
      } else if !snapshot.length.is_zero() {
        let fraction =
          (snapshot.position.as_secs_f32() / snapshot.length.as_secs_f32()).clamp(0.0, 1.0);
        let bar = Rect::new(
          text_x,
          y + size * 2.35,
          text_width,
          progress_height(look.progress, size, scale),
        );
        draw_progress(canvas, bar, fraction, look.progress, accent, muted, scale);
      }
      if look.controls {
        let rects = draw_controls(
          canvas,
          Rect::new(text_x, y + size * 3.15, text_width, size * 0.9),
          size,
          muted,
          accent,
          playing,
          hovered,
        );
        if interactive {
          controls = rects.map(Some);
        }
      }
    }
    MediaLayout::Compact => {
      let thumb = if art.is_some() { size * 1.6 } else { 0.0 };
      let mut text_x = x;
      if let Some(art) = art {
        let thumb_y = y + ((size * 2.6 - thumb) * 0.5).max(0.0);
        canvas.image_in(art, Rect::new(x, thumb_y, thumb, thumb));
        text_x += thumb + size * 0.5;
      }
      let text_width = (width - (text_x - x)).max(1.0);
      text.draw_line(
        canvas,
        &snapshot.title,
        Style::new(
          size,
          primary.with_alpha(if snapshot.stale { 210 } else { 255 }),
        ),
        Area::new(text_x, y + size * 0.1, text_width),
      );
      text.draw_line(
        canvas,
        &snapshot.artist,
        Style::new(size * 0.8, muted),
        Area::new(text_x, y + size * 1.05, text_width),
      );
      if snapshot.stale {
        text.draw_line(
          canvas,
          "Last played",
          Style::new(size * 0.7, muted.with_alpha(150)),
          Area::new(text_x, y + size * 2.05, text_width),
        );
      } else if !snapshot.length.is_zero() {
        let fraction =
          (snapshot.position.as_secs_f32() / snapshot.length.as_secs_f32()).clamp(0.0, 1.0);
        let bar = Rect::new(
          text_x,
          y + size * 2.05,
          text_width,
          progress_height(look.progress, size, scale),
        );
        draw_progress(canvas, bar, fraction, look.progress, accent, muted, scale);
      }
      if look.controls {
        let rects = draw_controls(
          canvas,
          Rect::new(text_x, y + size * 2.75, text_width, size * 0.9),
          size,
          muted,
          accent,
          playing,
          hovered,
        );
        if interactive {
          controls = rects.map(Some);
        }
      }
    }
    MediaLayout::Vinyl => {
      let disc = if art.is_some() { size * 4.0 } else { 0.0 };
      let center_x = x + width * 0.5;
      if let Some(art) = art {
        canvas.image_in(art, Rect::new(center_x - disc * 0.5, y, disc, disc));
      }
      let fraction = if snapshot.length.is_zero() {
        0.0
      } else {
        (snapshot.position.as_secs_f32() / snapshot.length.as_secs_f32()).clamp(0.0, 1.0)
      };
      let ring = look.progress == MediaProgress::Line && disc > 0.0;
      if ring && !snapshot.stale {
        draw_ring_progress(
          canvas,
          (center_x, y + disc * 0.5),
          disc * 0.5 + size * 0.1,
          fraction,
          accent,
          muted,
          (size * 0.12).max(1.0),
        );
      }
      // The ring sits just outside the disc, so the title starts a little
      // lower than the card layout to keep clear of it.
      let text_y = y + disc + size * 0.35;
      text.draw_line(
        canvas,
        &snapshot.title,
        Style::new(
          size * 0.78,
          primary.with_alpha(if snapshot.stale { 210 } else { 255 }),
        ),
        Area::new(x, text_y, width).align(Align::Center),
      );
      text.draw_line(
        canvas,
        &snapshot.artist,
        Style::new(size * 0.68, muted),
        Area::new(x, text_y + size * 0.95, width).align(Align::Center),
      );
      if snapshot.stale {
        text.draw_line(
          canvas,
          "Last played",
          Style::new(size * 0.7, muted.with_alpha(150)),
          Area::new(x, text_y + size * 2.0, width).align(Align::Center),
        );
      } else if !ring && !snapshot.length.is_zero() {
        let bar = Rect::new(
          x + width * 0.12,
          text_y + size * 2.0,
          width * 0.76,
          progress_height(look.progress, size, scale),
        );
        draw_progress(canvas, bar, fraction, look.progress, accent, muted, scale);
      }
      if look.controls {
        let rects = draw_controls(
          canvas,
          Rect::new(
            x + width * 0.25,
            text_y + size * 2.9,
            width * 0.5,
            size * 0.9,
          ),
          size,
          muted,
          accent,
          playing,
          hovered,
        );
        if interactive {
          controls = rects.map(Some);
        }
      }
    }
  }
  MediaDraw {
    bounds: rect,
    controls,
  }
}
