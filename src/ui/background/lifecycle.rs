//! Lifecycle for the wallpaper engine.

use super::*;

impl BackgroundView {
  #[must_use]
  pub fn new(config: &Config) -> Self {
    Self {
      config: config.background.clone(),
      placeholder: config.background.color,
      gradient: config.background.gradient,
      loaded: None,
      previous: None,
      fade_start: None,
      frame_start: Instant::now(),
      generation: 0,
      requested: None,
      accepted: None,
      loading: false,
      paused: false,
      paused_since: None,
      low_power: config.power.low_power || config.motion.reduced,
      warning: None,
    }
  }

  /// Applies a new config without throwing away a still-valid frame set.
  ///
  /// `keep_loaded` is the studio's promise that an edit did not touch the
  /// image path. When the kind and path are unchanged we keep the decoded
  /// frames and only update the cheap fields (colour, fps, idle pause). A
  /// different path, or a switch to/from a solid background, clears the frame
  /// set so the normal loader can replace it.
  pub fn reconfigure(&mut self, config: &Config, keep_loaded: bool) {
    let old = std::mem::replace(&mut self.config, config.background.clone());
    self.placeholder = config.background.color;
    self.gradient = config.background.gradient;
    self.low_power = config.power.low_power || config.motion.reduced;

    let image_kind = matches!(
      self.config.kind,
      BackgroundKind::Auto | BackgroundKind::Image | BackgroundKind::Animated
    );
    let reusable =
      keep_loaded && old.kind == self.config.kind && old.image == self.config.image && image_kind;
    if reusable {
      return;
    }

    self.loaded = None;
    self.previous = None;
    self.fade_start = None;
    self.accepted = None;
    self.requested = None;
    self.loading = false;
    // A fresh generation makes any loader result already in flight stale.
    self.generation = self.generation.wrapping_add(1);
  }

  /// Starts a load for this size and output. Returns `None` when the same
  /// load is already in flight, so a resize storm cannot spawn a thread per
  /// event.
  pub fn request(&mut self, size: (u32, u32), monitor: Option<&str>) -> Option<BackgroundRequest> {
    if size.0 == 0 || size.1 == 0 {
      return None;
    }
    let key = (size, monitor.map(str::to_string));
    if self.loading && self.requested.as_ref() == Some(&key) {
      return None;
    }
    // Already showing this exact size and output. Without this check every
    // redraw would start a fresh loader thread.
    if !self.loading && self.accepted.as_ref() == Some(&key) {
      return None;
    }
    self.generation = self.generation.wrapping_add(1);
    self.requested = Some(key);
    self.loading = true;
    Some(BackgroundRequest {
      generation: self.generation,
      config: self.config.clone(),
      size,
      monitor: monitor.map(str::to_string),
    })
  }

  /// Applies a loader result. Stale generations (from a window that was
  /// resized while a load was running) are ignored.
  pub fn accept(
    &mut self,
    generation: u64,
    result: Result<LoadedBackground, String>,
    now: Instant,
  ) -> bool {
    if generation != self.generation {
      return false;
    }
    self.loading = false;
    self.accepted = self.requested.take();
    match result {
      Ok(loaded) => {
        if let Some(warning) = &loaded.warning {
          warn!(%warning, "background warning");
        }
        self.warning = loaded.warning.clone();
        let had_previous = self.loaded.is_some();
        if let Some(current) = &self.loaded {
          match &current.content {
            Content::Stream(stream) => {
              self.previous = stream.current();
            }
            _ => {
              let (frame, _) = current.frame_at(now.duration_since(self.frame_start));
              self.previous = frame.cloned();
            }
          }
        }
        self.loaded = Some(loaded);
        self.frame_start = now;
        // A live stream must start frozen when low power is on.
        if self.low_power
          && let Some(Content::Stream(stream)) = self.loaded.as_ref().map(|l| &l.content)
        {
          stream.set_paused(true);
        }
        // Only crossfade when replacing something. The very first wallpaper
        // should appear fully opaque: a greeter must not depend on a later
        // animation tick to become visible (an unfocused window would sit on
        // the placeholder forever). Requiring a captured previous frame also
        // keeps the CPU and GPU paths identical: neither fades from a
        // placeholder that the GPU cannot sample.
        self.fade_start = if had_previous && self.previous.is_some() {
          Some(now)
        } else {
          None
        };
        true
      }
      Err(error) => {
        warn!(%error, "cannot load background, keeping the gradient");
        self.warning = Some(format!("Background: {error}"));
        false
      }
    }
  }

  /// A load failure that happened before the event loop could deliver it, as
  /// in the screenshot path or a thread spawn failure.
  pub fn fail(&mut self, message: impl Into<String>) {
    let message = message.into();
    self.loading = false;
    // Remember the failed key too. A corrupt file is not going to fix itself
    // while the greeter is on screen, and retrying on every redraw would spawn
    // a thread per frame.
    self.accepted = self.requested.take();
    // The window path logs failures in `accept`; this is the equivalent for a
    // synchronous load. Without it a `--screenshot` run would show the warning
    // banner but print nothing to stderr, which is easy to miss in CI.
    warn!(%message, "cannot load background, keeping the gradient");
    self.warning = Some(format!("Background: {message}"));
  }

  #[must_use]
  pub fn warning(&self) -> Option<&str> {
    self.warning.as_deref()
  }

  /// True when the current wallpaper has more than one frame, which tells the
  /// glass cache it may refresh on an interval instead of once.
  #[must_use]
  pub fn is_animated(&self) -> bool {
    self
      .loaded
      .as_ref()
      .is_some_and(LoadedBackground::is_animated)
  }

  /// Installs the frame callback on the current live stream. The window calls
  /// this after accepting a wallpaper so every published frame wakes the event
  /// loop instead of relying on a 16 ms poll.
  pub fn set_waker(&mut self, waker: Option<Arc<dyn Fn() + Send + Sync>>) {
    if let Some(Content::Stream(stream)) = self.loaded.as_ref().map(|loaded| &loaded.content) {
      stream.set_waker(waker);
    }
  }

  pub fn set_paused(&mut self, paused: bool) {
    if paused == self.paused {
      return;
    }
    if let Some(Content::Stream(stream)) = self.loaded.as_ref().map(|l| &l.content) {
      stream.set_paused(paused || self.low_power);
    }
    if paused {
      self.paused = true;
      self.paused_since = Some(Instant::now());
      return;
    }
    // Resume where the animation stopped. Shifting the start instant by the
    // paused duration avoids a jump back to frame zero (and avoids pretending
    // the whole pause was part of the animation).
    self.paused = false;
    if let Some(since) = self.paused_since.take() {
      let paused_for = Instant::now().saturating_duration_since(since);
      self.frame_start += paused_for;
      if let Some(fade) = self.fade_start {
        self.fade_start = Some(fade + paused_for);
      }
    }
  }

  /// The instant used to choose an animation frame.
  ///
  /// Low power freezes at the load instant rather than at a live clock, so a
  /// repaint caused by the ticking clock cannot walk the wallpaper forward one
  /// frame per second. Pausing freezes at the pause instant for the same
  /// reason. Without this, "freeze on a still frame" would look true at first
  /// and then drift as soon as the greeter repainted for any other reason.
  pub(super) fn animation_now(&self, now: Instant) -> Instant {
    if self.low_power {
      self.frame_start
    } else {
      self.paint_time(now)
    }
  }
}
