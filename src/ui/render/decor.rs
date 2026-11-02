//! Decor pass for the renderer.

use super::*;

/// Pointer-driven parallax offset for one decor item or built-in widget.
///
/// The result is in logical pixels unless `scale` is non-zero, in which case it
/// also carries the output scale for callers that paint in physical pixels.
/// `depth` is the per-item strength and `[motion].parallax` is the global
/// multiplier. Either one at zero disables the effect, and a missing pointer or
/// a zero-sized screen returns zero so screenshots and headless runs never
/// invent a cursor position.
pub(super) fn parallax_offset(scene: &Scene<'_>, depth: f32, scale: f32) -> (f32, f32) {
  let Some(cursor) = scene.cursor else {
    return (0.0, 0.0);
  };
  let strength = scene.theme.motion.parallax * depth;
  if strength <= 0.0 || scene.screen.0 <= 0.0 || scene.screen.1 <= 0.0 {
    return (0.0, 0.0);
  }
  let nx = (cursor.0 / scene.screen.0 - 0.5) * 2.0;
  let ny = (cursor.1 / scene.screen.1 - 0.5) * 2.0;
  (-nx * strength * 28.0 * scale, -ny * strength * 20.0 * scale)
}

/// Whether the glitch window is open at `elapsed`.
///
/// The window is the first 8 percent of each `glitch_ms` period. Text changes
/// for a short burst and then rests, which reads as a corrupted signal instead
/// of a constant shimmer. A zero strength or period keeps it closed.
pub(super) fn glitch_window(elapsed: f32, glitch: f32, glitch_ms: f32) -> bool {
  if glitch <= 0.0 || glitch_ms <= 0.0 {
    return false;
  }
  (elapsed / (glitch_ms / 1000.0)) % 1.0 < 0.08
}

/// One pseudo-random glyph for the scramble effect.
///
/// We deliberately do not pull in a random number generator. A tiny integer
/// hash keyed by the character index and the current tick is deterministic
/// across runs, so the same frame always looks the same, while a new tick
/// reshuffles the tail. That makes screenshots and tests reproducible.
pub(super) fn scramble_glyph(index: usize, tick: u64) -> char {
  const GLYPHS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#$%&";
  let mut hash =
    (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ tick.wrapping_mul(0xBF58_476D_1CE4_E5B9);
  hash ^= hash >> 33;
  hash = hash.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
  hash ^= hash >> 33;
  GLYPHS[(hash % GLYPHS.len() as u64) as usize] as char
}

/// Applies the scramble reveal to one run.
///
/// Characters become real from the head as `reveal` grows. Every character has
/// a threshold of `(index + 1) / count`, so a reveal of 0.5 keeps the first
/// half and replaces the rest. Whitespace is preserved verbatim: scrambling a
/// space into a glyph would change the line width and make the text jump.
pub(super) fn scramble_run(run: &str, reveal: f32, tick: u64) -> String {
  let chars: Vec<char> = run.chars().collect();
  let count = chars.len();
  if count == 0 {
    return String::new();
  }
  let mut out = String::with_capacity(run.len());
  for (index, ch) in chars.into_iter().enumerate() {
    let threshold = (index + 1) as f32 / count as f32;
    if ch.is_whitespace() || reveal >= threshold {
      out.push(ch);
    } else {
      out.push(scramble_glyph(index, tick));
    }
  }
  out
}

/// Computes the visible scramble run from the motion clock.
///
/// At `elapsed = 0` the final text is returned unchanged. That is the one
/// exception to "start at the beginning": a screenshot has no time axis, so
/// showing a wall of noise would make the captured frame useless. Live, the
/// effect resolves over `scramble_ms`.
pub(super) fn scramble_text(run: &str, elapsed: f32, scramble_ms: f32, tick: u64) -> String {
  if elapsed <= 0.0 || scramble_ms <= 0.0 {
    return run.to_string();
  }
  let reveal = (elapsed * 1000.0 / scramble_ms).clamp(0.0, 1.0);
  scramble_run(run, reveal, tick)
}

/// Draws a text run one character at a time with a sine wave per glyph.
///
/// Each character is measured and advanced independently, which is what lets
/// the glyphs ride different vertical offsets without changing the run width.
/// We ignore `stroke` here on purpose: the outline path draws eight copies per
/// character, and eight outline passes for every glyph is a lot of pixels for
/// an effect that already moves. A waving fill reads clearly without it.
/// One wavy text run. Bundling the shaping and wave inputs keeps the helper
/// readable; passing eleven positional arguments made every call site a game
/// of remembering which float was which.
pub(super) struct WaveText<'a> {
  pub(super) run: &'a str,
  pub(super) style: Style,
  pub(super) area: Area,
  pub(super) size: f32,
  pub(super) weight: cosmic_text::Weight,
  pub(super) tracking: f32,
  pub(super) amplitude: f32,
  pub(super) phase: f32,
  pub(super) step: f32,
}

pub(super) fn draw_wave_text(text: &mut TextEngine, layer: &mut Canvas, wave: WaveText<'_>) {
  let WaveText {
    run,
    style,
    area,
    size,
    weight,
    tracking,
    amplitude,
    phase,
    step,
  } = wave;
  let mut x = area.x;
  for (index, ch) in run.chars().enumerate() {
    let mut buffer = [0u8; 4];
    let glyph = ch.encode_utf8(&mut buffer);
    let (char_w, _) = text.measure_with(glyph, size, f32::MAX / 4.0, false, weight, tracking);
    let y = (TAU * (phase + index as f32 * step)).sin() * amplitude;
    let mut char_area = Area::new(x, area.y + y, char_w.max(1.0));
    char_area.clip = area.clip;
    text.draw_line(layer, glyph, style, char_area);
    x += char_w;
  }
}

/// Which decor items to paint. A full-bleed card can bake its still ornament
/// and text into one texture and leave only the moving items in the overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DecorFilter {
  All,
  Static,
  Dynamic,
}

/// Draws the configured decor inside the card. Images are already decoded, so
/// this only card_scales them to their display width. Items paint in list
/// order, so a later item covers an earlier one. The form paints afterwards,
/// which means a decor item can never cover an input or the submit button.
pub(super) fn draw_login_decor(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  card: Rect,
  card_scale: f32,
  filter: DecorFilter,
) {
  let animate = !scene.theme.motion.reduced && !scene.theme.low_power;
  let elapsed = if animate { ambient_elapsed(scene) } else { 0.0 };
  for item in &scene.theme.decor {
    let dynamic = (item.pulse_on_beat && scene.motion.beat_ms > 0.0) || item.motion.active();
    if (filter == DecorFilter::Static && dynamic) || (filter == DecorFilter::Dynamic && !dynamic) {
      continue;
    }
    let sample = item.motion.sample(elapsed);
    let opacity = item.opacity * sample.opacity;
    let angle = item.rotation + sample.angle;
    // The beat pop multiplies the item's own pulse instead of replacing it, so
    // a breathing ornament keeps breathing and a static one still pops.
    let beat_scale = if item.pulse_on_beat && scene.motion.beat_ms > 0.0 {
      decor_beat_scale(scene.motion.beat_phase)
    } else {
      1.0
    };
    let scale = sample.scale * beat_scale;
    // Parallax is a logical offset, so it joins the configured offset before
    // the card scale is applied. Passing 1.0 here keeps the helper from
    // painting the display scale twice.
    let parallax = parallax_offset(scene, item.motion.parallax_depth, 1.0);
    let offset = (
      ((item.offset.0 as f32 + sample.offset.0 + parallax.0) * card_scale).round() as i32,
      ((item.offset.1 as f32 + sample.offset.1 + parallax.1) * card_scale).round() as i32,
    );
    if item.panel {
      // Typer and ticker only have a text run to work on, so a panel simply
      // ignores them and stays static. Blink is different: it gates the whole
      // plate, which is how a theme gets a radar blip.
      if !sample.blink_on {
        continue;
      }
      let width = item.width * card_scale;
      let height = item.height * card_scale;
      if width <= 0.0 || height <= 0.0 {
        continue;
      }
      let (x, y) = item.anchor.place((card.w, card.h), (width, height), offset);
      let rect = Rect::new(card.x + x, card.y + y, width, height);
      paint_transformed(
        canvas,
        rect,
        rect.center(),
        angle,
        scale,
        opacity,
        |layer, _| {
          let radius = item.radius * card_scale;
          // `glass` asks for the shared backdrop under the fill. The panel's
          // own `color` is still painted on top, so a translucent colour is
          // what lets the blur show through.
          if item.glass {
            glass.backdrop(layer, rect, radius, 1.0);
          }
          // Use the same primitive as a field background. A solid fill skips
          // the shader entirely, which keeps the common plate cheap.
          if let Some(end) = item.color_end {
            layer.linear_gradient_stops(
              rect,
              radius,
              item.gradient_angle,
              &[(0.0, item.color), (1.0, end)],
            );
          } else {
            layer.fill(rect, item.color, radius);
          }
          if let Some(border) = item.border {
            layer.stroke(rect, border, radius, 1.0);
          }
        },
      );
    } else if let Some(image) = &item.image {
      // Blink is a hard visibility gate for images too, so a chip or cursor
      // can strobe without fading. Same rule as the panel and text branches.
      if !sample.blink_on {
        continue;
      }
      let width = item.width * card_scale;
      if width <= 0.0 || image.width() == 0 {
        continue;
      }
      let height = width * image.height() as f32 / image.width() as f32;
      let (x, y) = item.anchor.place((card.w, card.h), (width, height), offset);
      let rect = Rect::new(card.x + x, card.y + y, width, height);
      canvas.image_transformed(image, rect, angle, scale, opacity);
    } else {
      // Blink is a hard visibility gate, not a fade: a terminal cursor is
      // either there or it is not. Do this before any measuring so a hidden
      // item costs nothing and cannot leave a partial paint behind.
      if !sample.blink_on {
        continue;
      }
      let size = item.size * card_scale;
      let style = if item.bold {
        Style::new(size, item.color).bold()
      } else {
        Style::new(size, item.color)
      }
      .tracking(item.tracking);
      if item.motion.ticker_ms > 0.0 && item.motion.ticker_width > 0 {
        // A ticker repeats a whole run at a fixed cadence, so it keeps the
        // fill style only. Stroke is deliberately ignored for tickered items.
        // The strip is a fixed window placed like a normal text run. We build
        // one tile per text run plus the configured gap, then repeat the tile
        // far enough to cover the window at the current scroll offset. The
        // tile keeps its exact measured width so the wrap at `loop_frac = 0`
        // lands on the same pixels as the previous frame.
        let strip_w = (item.motion.ticker_width as f32 * card_scale).max(1.0);
        // Measure with the style we paint with. The default measurement is
        // normal-weight and untracked, so a bold or letter-spaced ticker would
        // come back narrower than the glyphs and adjacent tiles would collide.
        let (tile_w, height) = text.measure_with(
          &item.text,
          size,
          f32::MAX / 4.0,
          false,
          style.weight,
          style.tracking,
        );
        let tile = (tile_w + item.motion.ticker_gap as f32 * card_scale).max(1.0);
        let (x, y) = item
          .anchor
          .place((card.w, card.h), (strip_w, height), offset);
        let strip = Rect::new(card.x + x, card.y + y, strip_w, height);
        let scroll = sample.loop_frac * tile;
        let tiles = ((strip_w + scroll) / tile).ceil() as usize;
        paint_transformed(
          canvas,
          strip,
          strip.center(),
          angle,
          scale,
          opacity,
          |layer, _| {
            for k in 0..=tiles {
              let tile_x = strip.x + (k as f32) * tile - scroll;
              text.draw_line(
                layer,
                &item.text,
                style,
                Area::new(tile_x, strip.y, tile_w.max(1.0)).clip(strip),
              );
            }
          },
        );
      } else {
        // The scramble reveal wins over the typewriter on the same item, and
        // a ticker would have taken the branch above. At `elapsed = 0` the
        // scramble helper returns the full run so a screenshot stays readable
        // instead of showing a wall of noise.
        //
        // The typewriter reveals whole characters, never partial glyphs.
        // `round` means the last character appears once we are past its
        // midpoint, which keeps the cadence even for short strings. A
        // tickered item never reaches this branch because a marquee already
        // shows the run.
        let shown = if item.motion.scramble_ms > 0.0 {
          scramble_text(
            &item.text,
            sample.elapsed,
            item.motion.scramble_ms,
            (sample.elapsed * 12.0) as u64,
          )
        } else if item.motion.typer_ms > 0.0 {
          let count = item.text.chars().count();
          let visible = (sample.reveal * count as f32).round() as usize;
          if visible == 0 {
            continue;
          }
          item.text.chars().take(visible).collect::<String>()
        } else {
          item.text.clone()
        };
        // Same reasoning as the ticker: anchor the static and typer text by
        // its painted width, not the default-style width. The wave uses the
        // same box, so the item does not jump when it starts moving.
        let (width, height) = text.measure_with(
          &shown,
          size,
          card.w.max(1.0),
          false,
          style.weight,
          style.tracking,
        );
        let (x, y) = item
          .anchor
          .place((card.w, card.h), (width.max(1.0), height), offset);
        let rect = Rect::new(card.x + x, card.y + y, width.max(1.0), height);
        let wave_on = item.motion.wave > 0.0;
        let wave_amp = if wave_on {
          item.motion.wave * card_scale
        } else {
          0.0
        };
        let glitch_on = glitch_window(sample.elapsed, item.motion.glitch, item.motion.glitch_ms);
        let glitch_dx = if item.motion.glitch > 0.0 {
          (2.0 + 4.0 * item.motion.glitch) * card_scale
        } else {
          0.0
        };
        // The outline sits outside the fill bounds and the wave moves glyphs
        // vertically, so grow the layer by the largest excursion. Without this
        // the top and bottom of the wave would be clipped.
        let stroke_radius = if item.stroke.is_some() && !wave_on {
          item.stroke_width * card_scale
        } else {
          0.0
        };
        let pad = stroke_radius.max(wave_amp).max(glitch_dx);
        let bounds = if pad > 0.0 { rect.inset(-pad) } else { rect };
        let wave_phase = if item.motion.wave_ms > 0.0 {
          (sample.elapsed / (item.motion.wave_ms / 1000.0)) % 1.0
        } else {
          0.0
        };
        paint_transformed(
          canvas,
          bounds,
          rect.center(),
          angle,
          scale,
          opacity,
          |layer, _| {
            // Glitch is text-only, and these two coloured copies sit behind
            // the real run. The offsets are deterministic, so the effect does
            // not flicker differently on every repaint.
            if glitch_on {
              for (color, shift) in [
                (Rgb::solid(255, 64, 64), -glitch_dx),
                (Rgb::solid(64, 220, 255), glitch_dx),
              ] {
                let copy_style = Style {
                  color: color.with_alpha(70),
                  ..style
                };
                let copy_area = Area::new(rect.x + shift, rect.y, width.max(1.0)).clip(bounds);
                if wave_on {
                  draw_wave_text(
                    text,
                    layer,
                    WaveText {
                      run: &shown,
                      style: copy_style,
                      area: copy_area,
                      size,
                      weight: style.weight,
                      tracking: style.tracking,
                      amplitude: wave_amp,
                      phase: wave_phase,
                      step: item.motion.wave_step,
                    },
                  );
                } else {
                  text.draw_line(layer, &shown, copy_style, copy_area);
                }
              }
            }
            if wave_on {
              // Wave ignores `stroke`: eight outline passes per character is
              // not worth the pixels, and the moving fill already reads well.
              draw_wave_text(
                text,
                layer,
                WaveText {
                  run: &shown,
                  style,
                  area: Area::new(rect.x, rect.y, width.max(1.0)).clip(bounds),
                  size,
                  weight: style.weight,
                  tracking: style.tracking,
                  amplitude: wave_amp,
                  phase: wave_phase,
                  step: item.motion.wave_step,
                },
              );
              return;
            }
            if let Some(stroke) = item.stroke {
              // Eight copies on a 45-degree ring is enough for a clean
              // outline without a real glyph-stroking pass. The fill goes on
              // top afterwards, so only the outer half of each copy shows.
              let stroke_style = Style {
                color: stroke,
                ..style
              };
              let step = std::f32::consts::FRAC_PI_4;
              for index in 0..8 {
                let radians = step * index as f32;
                let (dx, dy) = (radians.cos() * stroke_radius, radians.sin() * stroke_radius);
                text.draw_line(
                  layer,
                  &shown,
                  stroke_style,
                  Area::new(rect.x + dx, rect.y + dy, width.max(1.0)),
                );
              }
            }
            text.draw_line(
              layer,
              &shown,
              style,
              Area::new(rect.x, rect.y, width.max(1.0)),
            );
          },
        );
      }
    }
  }
}
