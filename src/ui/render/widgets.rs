//! Widgets pass for the renderer.

use super::*;

/// A stable id for a widget's drag offset. Built-ins use their kind so
/// disabling the clock does not hand the media widget the clock's saved spot.
pub(super) fn widget_id(plugin: &Plugin, index: usize, widget: &Widget) -> String {
  if plugin.builtin {
    match widget {
      Widget::Clock { .. } => return "clock".to_string(),
      Widget::Media { .. } => return "media".to_string(),
      Widget::Calendar { .. } => return "calendar".to_string(),
      Widget::Battery { .. } => return "battery".to_string(),
      Widget::System { .. } => return "system".to_string(),
      _ => {}
    }
  }
  format!("{}:{index}", plugin.id)
}

/// True when at least one widget asked for the front layer and is visible on
/// this output. The rotated card uses this to avoid allocating a second
/// full-card layer when no widget will land between decor and form.
pub(super) fn has_front_widgets(scene: &Scene<'_>) -> bool {
  scene.plugins.iter().any(|plugin| {
    plugin.widgets.iter().any(|widget| {
      widget_layer(scene, plugin, widget) == WidgetLayer::Front
        && widget.placement().visible_on(scene.monitor)
    })
  })
}

/// Resolves the paint layer for one widget. The built-in clock and media use
/// their `[clock]`/`[media]` layer so a theme can put them inside a HUD bar.
/// Every plugin widget carries its own manifest layer, which defaults to
/// `Behind`; the form still paints after both layers, so a plugin can never
/// cover an input.
pub(super) fn widget_layer(scene: &Scene<'_>, plugin: &Plugin, widget: &Widget) -> WidgetLayer {
  if !plugin.builtin {
    return widget.layer();
  }
  match widget {
    Widget::Clock { .. } => scene.theme.clock_layer,
    Widget::Media { .. } => scene.theme.media_layer,
    // The calendar, battery, and system widgets carry their own configured
    // layer because they are optional first-party extras rather than part of
    // the historical clock/media theme slots.
    Widget::Calendar { spec, .. } => spec.layer,
    Widget::Battery { spec, .. } => spec.layer,
    Widget::System { spec, .. } => spec.layer,
    _ => WidgetLayer::Behind,
  }
}

/// The coordinate frame a widget is placed in.
///
/// Behind widgets keep the historical screen-space frame: the screen's
/// top-left is the origin and the output scale sizes the offset and the font.
/// Front widgets on a built-in clock or media slot join the card frame the
/// same way decor does: the card's top-left is the origin, the card extent is
/// the anchor space, and `card_scale` scales both the offset and the size
/// base. That is what makes a HUD pill shrink with a hero card on a smaller
/// display instead of overflowing a decor bar that already shrank.
#[derive(Debug, Clone, Copy)]
pub(super) struct WidgetFrame {
  pub(super) origin: (f32, f32),
  pub(super) extent: (f32, f32),
  pub(super) scale: f32,
  pub(super) card_space: bool,
}

impl WidgetFrame {
  pub(super) fn screen(screen: (f32, f32), scale: f32) -> Self {
    Self {
      origin: (0.0, 0.0),
      extent: screen,
      scale,
      card_space: false,
    }
  }

  pub(super) fn card(card: Rect, card_scale: f32) -> Self {
    Self {
      origin: (card.x, card.y),
      extent: (card.w, card.h),
      scale: card_scale,
      card_space: true,
    }
  }

  /// The rectangle the widget anchors inside. Its top-left is already an
  /// absolute screen coordinate, so drawing code adds it to the local anchor
  /// result instead of threading a separate origin.
  pub(super) fn viewport(self) -> Rect {
    Rect::new(self.origin.0, self.origin.1, self.extent.0, self.extent.1)
  }
}

/// Resolves a manifest offset plus the live drag offset in the frame's units.
///
/// `configured` already includes any motion sample, because the sample is in
/// logical pixels and has to join the offset before the scale multiplication.
/// Card-space widgets round after adding the drag, matching decor, so a
/// subpixel drag cannot drift by a pixel every frame. Screen-space widgets
/// keep the old two-rounding rule, which is what leaves the behind layer
/// byte-for-byte unchanged when no motion is configured.
pub(super) fn widget_offset(
  frame: &WidgetFrame,
  configured: (f32, f32),
  drag: (f32, f32),
) -> (i32, i32) {
  if frame.card_space {
    (
      ((configured.0 + drag.0) * frame.scale).round() as i32,
      ((configured.1 + drag.1) * frame.scale).round() as i32,
    )
  } else {
    (
      (configured.0 * frame.scale).round() as i32 + (drag.0 * frame.scale).round() as i32,
      (configured.1 * frame.scale).round() as i32 + (drag.1 * frame.scale).round() as i32,
    )
  }
}

/// The current login state for widget visibility rules.
///
/// The order matters when more than one flag is set. A request in flight is
/// the most useful thing to react to, an error is next, and typing is only
/// reported when nothing louder is happening. That keeps `show_on` rules
/// predictable without asking a config author to think about combinations.
pub(super) fn current_widget_state(view: &View) -> WidgetState {
  if view.busy {
    WidgetState::Busy
  } else if view
    .status
    .as_ref()
    .is_some_and(|line| line.tone == Tone::Error)
  {
    WidgetState::Error
  } else if view.authenticated {
    WidgetState::Authenticated
  } else if !view.username.is_empty() || !view.password.is_empty() {
    WidgetState::Typing
  } else {
    WidgetState::Idle
  }
}

/// Whether a state-gated widget should paint this frame. `hide_on` wins over
/// `show_on`; an empty `show_on` means the widget is visible in every state.
pub(super) fn widget_visible(
  show_on: &[WidgetState],
  hide_on: &[WidgetState],
  view: &View,
) -> bool {
  let state = current_widget_state(view);
  if hide_on.contains(&state) {
    return false;
  }
  show_on.is_empty() || show_on.contains(&state)
}

/// Seconds of widget motion for this frame. Reduced motion and low-power mode
/// pin it to zero, which is the neutral pose. The clock also uses this for its
/// colon blink, so a screenshot (`started == frame_now`) always captures the
/// lit phase.
pub(super) fn widget_elapsed(scene: &Scene<'_>) -> f32 {
  if scene.theme.motion.reduced || scene.theme.low_power {
    return 0.0;
  }
  scene
    .frame_now
    .saturating_duration_since(scene.started)
    .as_secs_f32()
}

/// Samples a built-in widget's motion. Reduced motion and low-power mode pin
/// the sample to `elapsed = 0`, which is the neutral pose, so the widget stays
/// still instead of disappearing or jumping.
pub(super) fn widget_sample(look: Option<&WidgetLook>, scene: &Scene<'_>) -> Option<DecorSample> {
  let look = look?;
  Some(look.motion.sample(widget_elapsed(scene)))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_plugins(
  canvas: &mut Canvas,
  text: &mut TextEngine,
  glass: &GlassCache,
  scene: &Scene<'_>,
  frame: WidgetFrame,
  primary: Rgb,
  muted: Rgb,
  accent: Rgb,
  layer: WidgetLayer,
  regions: &mut Vec<HitRegion>,
) {
  let mut next_media = 0usize;
  for plugin in scene.plugins {
    for (index, widget) in plugin.widgets.iter().enumerate() {
      // The window spawns one MediaHandle per media widget in app.media_states()
      // order, which is this same plugin/widget order. The counter has to move
      // before the visible_on check so a widget hidden on this output does not
      // shift every later media widget onto the wrong player.
      let media_slot = if matches!(widget, Widget::Media { .. }) {
        let slot = next_media;
        next_media += 1;
        Some(slot)
      } else {
        None
      };
      if !widget.placement().visible_on(scene.monitor) {
        continue;
      }
      // Each widget is painted exactly once: the behind pass skips front
      // widgets and the front pass skips everything else. The hit region is
      // recorded in the same arm that paints, so both layers keep identical
      // ids and geometry.
      if widget_layer(scene, plugin, widget) != layer {
        continue;
      }
      let placement = widget.placement();
      // Only the built-in clock and media have a resolved look today. Plugin
      // widgets keep the historical fixed scrim and a static transform, so a
      // third-party manifest cannot opt into theme state rules yet.
      let look = if plugin.builtin {
        match widget {
          Widget::Clock { .. } => Some(&scene.theme.clock_look),
          Widget::Media { .. } => Some(&scene.theme.media_look),
          Widget::Calendar { spec, .. } => Some(&spec.look),
          Widget::Battery { spec, .. } => Some(&spec.look),
          Widget::System { spec, .. } => Some(&spec.look),
          _ => None,
        }
      } else {
        None
      };
      if let Some(look) = look
        && !widget_visible(&look.show_on, &look.hide_on, scene.view)
      {
        continue;
      }
      let sample = widget_sample(look, scene);
      // Blink gates the whole widget, plate included. That is what makes a
      // two-state radar blip possible without a second decor item.
      if sample.is_some_and(|sample| !sample.blink_on) {
        continue;
      }
      // Parallax is logical here; `widget_offset` applies the frame scale
      // afterwards, exactly like the configured offset and the motion sample.
      let parallax = look.map_or((0.0, 0.0), |look| {
        parallax_offset(scene, look.motion.parallax_depth, 1.0)
      });
      let configured = (
        placement.offset.0 as f32 + sample.map_or(0.0, |sample| sample.offset.0) + parallax.0,
        placement.offset.1 as f32 + sample.map_or(0.0, |sample| sample.offset.1) + parallax.1,
      );
      let id = widget_id(plugin, index, widget);
      let drag = scene.drag.offset(&id);
      let offset = widget_offset(&frame, configured, drag);
      let rotation = placement.rotation;
      let angle = rotation + sample.map_or(0.0, |sample| sample.angle);
      let (scale, opacity) = match (look, sample) {
        (Some(look), Some(sample)) => (sample.scale, sample.opacity * look.plate.opacity),
        _ => (1.0, 1.0),
      };
      let plate = look.map(|look| &look.plate);
      let draggable = widget.draggable();
      match widget {
        Widget::Text {
          text: content,
          size,
          color,
          max_width,
          ..
        } => {
          let size = *size * frame.scale;
          let limit = max_width.map_or((frame.extent.0 - 16.0 * frame.scale).max(1.0), |width| {
            width * frame.scale
          });
          let (width, height) = text.measure(content, size, limit, max_width.is_some());
          let (x, y) = placement
            .anchor
            .place(frame.extent, (width, height), offset);
          let (x, y) = (frame.origin.0 + x, frame.origin.1 + y);
          let rect = Rect::new(x, y, width, height);
          paint_rotated(canvas, rect, rect.center(), rotation, |layer| {
            text.draw(
              layer,
              content,
              Style::new(size, *color),
              Area::new(x, y, limit),
            );
          });
          regions.push(HitRegion::rotated(id.clone(), rect, draggable, rotation));
          draw_drag_highlight(canvas, rect, &id, scene, accent, frame.scale);
        }
        Widget::Clock {
          spec, draggable, ..
        } => {
          let color = spec.color.unwrap_or(primary);
          let date_color = spec.date_color.unwrap_or(muted);
          if let Some(bounds) = clock::bounds(
            text,
            spec,
            placement.anchor,
            offset,
            frame.viewport(),
            frame.scale,
            scene.now,
          ) {
            paint_transformed(
              canvas,
              bounds,
              bounds.center(),
              angle,
              scale,
              opacity,
              |layer, _| {
                // The plate is drawn before the digits so the whole widget,
                // including its future progress bar, sits on one surface.
                paint_widget_backdrop(layer, bounds, scene.theme, frame.scale, plate, glass);
                clock::draw(
                  layer,
                  text,
                  spec,
                  placement.anchor,
                  offset,
                  frame.viewport(),
                  frame.scale,
                  color,
                  date_color,
                  scene.now,
                  widget_elapsed(scene),
                  plate,
                  scene.theme,
                );
              },
            );
            regions.push(
              HitRegion::rotated(id.clone(), bounds, *draggable, angle)
                .with_drag_scale(frame.scale),
            );
            draw_drag_highlight(canvas, bounds, &id, scene, accent, frame.scale);
          }
        }
        Widget::Calendar {
          spec, draggable, ..
        } => {
          if let Some(bounds) = calendar::bounds(
            spec,
            placement.anchor,
            offset,
            frame.viewport(),
            frame.scale,
            scene.now,
          ) {
            paint_transformed(
              canvas,
              bounds,
              bounds.center(),
              angle,
              scale,
              opacity,
              |layer, _| {
                paint_widget_backdrop(layer, bounds, scene.theme, frame.scale, plate, glass);
                calendar::draw(
                  layer,
                  text,
                  spec,
                  placement,
                  frame.viewport(),
                  frame.scale,
                  scene.now,
                );
              },
            );
            regions.push(
              HitRegion::rotated(id.clone(), bounds, *draggable, angle)
                .with_drag_scale(frame.scale),
            );
            draw_drag_highlight(canvas, bounds, &id, scene, accent, frame.scale);
          }
        }
        Widget::Battery {
          spec, draggable, ..
        } => {
          if let Some(bounds) = battery::bounds(
            spec,
            &scene.view.battery,
            placement.anchor,
            offset,
            frame.viewport(),
            frame.scale,
          ) {
            paint_transformed(
              canvas,
              bounds,
              bounds.center(),
              angle,
              scale,
              opacity,
              |layer, _| {
                paint_widget_backdrop(layer, bounds, scene.theme, frame.scale, plate, glass);
                battery::draw(
                  layer,
                  text,
                  spec,
                  &scene.view.battery,
                  placement,
                  frame.viewport(),
                  frame.scale,
                );
              },
            );
            regions.push(
              HitRegion::rotated(id.clone(), bounds, *draggable, angle)
                .with_drag_scale(frame.scale),
            );
            draw_drag_highlight(canvas, bounds, &id, scene, accent, frame.scale);
          }
        }
        Widget::System {
          spec, draggable, ..
        } => {
          if let Some(bounds) = system::bounds(
            spec,
            placement.anchor,
            offset,
            frame.viewport(),
            frame.scale,
          ) {
            paint_transformed(
              canvas,
              bounds,
              bounds.center(),
              angle,
              scale,
              opacity,
              |layer, _| {
                paint_widget_backdrop(layer, bounds, scene.theme, frame.scale, plate, glass);
                system::draw(
                  layer,
                  text,
                  spec,
                  &scene.view.system,
                  placement,
                  frame.viewport(),
                  frame.scale,
                );
              },
            );
            regions.push(
              HitRegion::rotated(id.clone(), bounds, *draggable, angle)
                .with_drag_scale(frame.scale),
            );
            draw_drag_highlight(canvas, bounds, &id, scene, accent, frame.scale);
          }
        }
        Widget::Image { image, width, .. } => {
          let width = *width * frame.scale;
          let height = if image.width() == 0 {
            0.0
          } else {
            width * image.height() as f32 / image.width() as f32
          };
          let (x, y) = placement
            .anchor
            .place(frame.extent, (width, height), offset);
          let (x, y) = (frame.origin.0 + x, frame.origin.1 + y);
          let rect = Rect::new(x, y, width, height);
          paint_rotated(canvas, rect, rect.center(), rotation, |layer| {
            layer.image_in(image, rect);
          });
          regions.push(HitRegion::rotated(id.clone(), rect, draggable, rotation));
          draw_drag_highlight(canvas, rect, &id, scene, accent, frame.scale);
        }
        Widget::Media {
          state,
          size,
          show_art,
          look: media_look,
          color,
          show_before_login,
          draggable,
          ..
        } => {
          let slot = media_slot.unwrap_or(0);
          let snapshot = state.snapshot();
          let bounds = media_bounds(
            text,
            &snapshot,
            placement.anchor,
            offset,
            frame.viewport(),
            frame.scale,
            *size,
            *show_art,
            *show_before_login,
            scene.view.authenticated,
            *media_look,
          );
          let mut painted = None;
          paint_transformed(
            canvas,
            bounds,
            bounds.center(),
            angle,
            scale,
            opacity,
            |layer, _| {
              painted = Some(draw_media(
                layer,
                text,
                glass,
                scene,
                slot,
                &snapshot,
                placement.anchor,
                offset,
                frame.viewport(),
                frame.scale,
                *size,
                *show_art,
                *show_before_login,
                color.unwrap_or(primary),
                muted,
                accent,
                plate,
                *media_look,
                angle,
              ));
            },
          );
          // `paint_transformed` skips the closure entirely at zero opacity, so
          // the fallback keeps the widget's own drag region even when it faded
          // out this frame. Controls stay unclickable in that case.
          let draw = painted.unwrap_or(MediaDraw {
            bounds,
            controls: [None; 3],
          });
          regions.push(
            HitRegion::rotated(id.clone(), draw.bounds, *draggable, angle)
              .with_drag_scale(frame.scale),
          );
          // Pushed after the widget region, in the same order the glyphs were
          // drawn. Reverse hit testing then sees a control before the plate.
          for (action, rect) in ["prev", "play", "next"].into_iter().zip(draw.controls) {
            if let Some(rect) = rect {
              regions.push(HitRegion::new(
                format!("media:{slot}:{action}"),
                rect,
                false,
              ));
            }
          }
          draw_drag_highlight(canvas, draw.bounds, &id, scene, accent, frame.scale);
        }
      }
    }
  }
}
