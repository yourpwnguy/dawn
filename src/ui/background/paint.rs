//! Paint for the wallpaper engine.

use super::*;

impl BackgroundView {
  pub fn paint(&mut self, canvas: &mut Canvas, now: Instant) {
    let now = self.animation_now(now);
    match &self.loaded {
      None => self.paint_placeholder(canvas),
      Some(loaded) => {
        if let Some(previous) = &self.previous {
          canvas.image_cover(previous);
        } else {
          self.paint_placeholder(canvas);
        }
        let opacity = match self.fade_start {
          Some(start) if !self.low_power => {
            (now.duration_since(start).as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0)
          }
          _ => 1.0,
        };
        if opacity >= 1.0 {
          self.fade_start = None;
        }
        let elapsed = now.duration_since(self.frame_start);
        match &loaded.content {
          Content::Solid(color) => canvas.clear(*color),
          Content::Gradient(color) => draw_gradient(canvas, *color),
          Content::Frames { .. } => {
            let (frame, _) = loaded.frame_at(elapsed);
            if let Some(frame) = frame {
              canvas.image_cover_alpha(frame, opacity);
            }
          }
          Content::Stream(stream) => {
            if self.low_power || self.paused {
              stream.set_paused(true);
            }
            if let Some(frame) = stream.current() {
              canvas.image_cover_alpha(&frame, opacity);
            }
          }
        }
      }
    }
  }

  /// A borrowing-free view of the frame that [`Self::paint`] would draw right
  /// now. The GPU renderer uses it to upload only the current and previous
  /// frames, and to get the crossfade weight and placeholder kind without
  /// painting a full-screen CPU pixmap.
  ///
  /// `frame_id` follows the same pause and low-power rules as
  /// [`Self::frame_id`], so an idle greeter keeps a stable snapshot.
  #[must_use]
  pub fn snapshot(&self, now: Instant) -> BackgroundSnapshot {
    let now = self.animation_now(now);
    let frame_id = self.frame_id(now);
    let placeholder = if self.gradient {
      SnapshotKind::Gradient(self.placeholder)
    } else {
      SnapshotKind::Solid(self.placeholder)
    };
    let Some(loaded) = &self.loaded else {
      return BackgroundSnapshot {
        current: None,
        previous: None,
        opacity: 1.0,
        kind: placeholder,
        frame_id,
        animated: false,
      };
    };
    let elapsed = now.duration_since(self.frame_start);
    let (current, mut kind) = match &loaded.content {
      Content::Solid(color) => (None, SnapshotKind::Solid(*color)),
      Content::Gradient(color) => (None, SnapshotKind::Gradient(*color)),
      Content::Frames { .. } => {
        let (frame, _) = loaded.frame_at(elapsed);
        (frame.cloned(), SnapshotKind::Image)
      }
      Content::Stream(stream) => (stream.current(), SnapshotKind::Image),
    };
    // A live source can be between its load and the first published frame.
    // Falling back to the placeholder is what `paint` does too.
    if kind == SnapshotKind::Image && current.is_none() {
      kind = placeholder;
    }
    let opacity = match self.fade_start {
      Some(start) if !self.low_power => {
        (now.duration_since(start).as_secs_f32() / FADE.as_secs_f32()).clamp(0.0, 1.0)
      }
      _ => 1.0,
    };
    BackgroundSnapshot {
      current,
      // `paint` keeps drawing the old frame for the whole transition, but the
      // GPU only needs it while the new frame is still fading in.
      previous: if opacity < 1.0 {
        self.previous.clone()
      } else {
        None
      },
      opacity,
      kind,
      frame_id,
      animated: loaded.is_animated(),
    }
  }

  /// Paints a snapshot for a CPU consumer such as the glass blur cache. This
  /// has no side effects: unlike [`Self::paint`] it never clears the fade
  /// state, so the GPU crossfade and the CPU fallback cannot fight over it.
  pub fn paint_snapshot(&self, canvas: &mut Canvas, now: Instant) {
    let snapshot = self.snapshot(now);
    if let Some(previous) = &snapshot.previous {
      canvas.image_cover(previous);
    }
    match snapshot.kind {
      SnapshotKind::Solid(color) => canvas.clear(color),
      SnapshotKind::Gradient(color) => draw_gradient(canvas, color),
      SnapshotKind::Image => {
        if snapshot.previous.is_none() {
          self.paint_placeholder(canvas);
        }
        if let Some(current) = &snapshot.current {
          canvas.image_cover_alpha(current, snapshot.opacity);
        }
      }
    }
  }

  pub(super) fn paint_placeholder(&self, canvas: &mut Canvas) {
    if self.gradient {
      draw_gradient(canvas, self.placeholder);
    } else {
      canvas.clear(self.placeholder);
    }
  }

  /// The next instant the window should wake for a wallpaper frame or fade.
  #[must_use]
  pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
    if self.low_power || self.paused {
      return None;
    }
    if let Some(start) = self.fade_start {
      let end = start + FADE;
      if now < end {
        return Some(now + Duration::from_millis(16));
      }
    }
    let loaded = self.loaded.as_ref()?;
    if let Content::Stream(_) = &loaded.content {
      // The stream wakes the window when a frame is published, so there is
      // nothing to poll. Keeping a 16 ms deadline here would burn a wakeup per
      // frame even when the video is slower than 60 fps.
      return None;
    }
    let Content::Frames { delays, .. } = &loaded.content else {
      return None;
    };
    if delays.len() <= 1 {
      return None;
    }
    let elapsed = now.duration_since(self.frame_start);
    let total: Duration = delays.iter().copied().sum();
    if total.is_zero() {
      return None;
    }
    let mut position = wrap(elapsed, total);
    for delay in delays {
      if position < *delay {
        return Some(now + (*delay - position));
      }
      position = position.saturating_sub(*delay);
    }
    Some(now + *delays.first().unwrap_or(&Duration::from_millis(33)))
  }

  /// The instant the next paint should treat as "now". While paused this is
  /// the moment the pause began, so repaints driven by the clock tick (the
  /// clock keeps ticking on an unfocused but visible greeter) render the
  /// frozen frame instead of silently advancing the animation.
  #[must_use]
  pub fn paint_time(&self, now: Instant) -> Instant {
    if self.paused {
      self.paused_since.unwrap_or(now)
    } else {
      now
    }
  }

  /// A stable identifier for the current frame, used by the blur cache and
  /// the window's change detection. Frozen while paused and in low-power mode,
  /// matching `paint`, so a repaint neither re-blurs nor signals a frame change
  /// when the wallpaper is not supposed to move.
  #[must_use]
  pub fn frame_id(&self, now: Instant) -> u64 {
    // An unloaded wallpaper shows the placeholder, and that is not the same
    // identity as the wallpaper that will load a moment later. The generation
    // does not change across the transition (the request already bumped it and
    // a still image keeps frame index 0 forever), so the blur cache computed
    // from the placeholder would stick around after the real pixels arrive.
    // Sentinel `u64::MAX` means "next change wakes the caches"; glass itself
    // uses it as its not-prepared marker, so the two sides agree.
    if self.loaded.is_none() {
      return u64::MAX;
    }
    if let Some(Content::Stream(stream)) = self.loaded.as_ref().map(|l| &l.content) {
      return self
        .generation
        .wrapping_mul(1_000_003)
        .wrapping_add(stream.sequence());
    }
    let now = self.animation_now(now);
    let index = self.loaded.as_ref().map_or(0, |loaded| {
      loaded.frame_at(now.duration_since(self.frame_start)).1
    });
    self
      .generation
      .wrapping_mul(1024)
      .wrapping_add(index as u64)
  }

  /// True while the crossfade is running.
  #[must_use]
  pub fn fading(&self, now: Instant) -> bool {
    self
      .fade_start
      .is_some_and(|start| now.duration_since(start) < FADE)
  }

  /// Blocks until a wallpaper is ready. Only the screenshot path uses this;
  /// the live window must always start with the placeholder.
  pub fn load_blocking(&mut self, size: (u32, u32), monitor: Option<&str>, now: Instant) -> bool {
    self.generation = self.generation.wrapping_add(1);
    let generation = self.generation;
    match LoadedBackground::load(&self.config, size, monitor) {
      Ok(loaded) => self.accept(generation, Ok(loaded), now),
      Err(error) => {
        self.fail(error);
        false
      }
    }
  }

  /// The configured background, for screenshots and diagnostics.
  #[must_use]
  pub fn config(&self) -> &BackgroundConfig {
    &self.config
  }
}
