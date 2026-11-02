//! Motion methods for the login app.

use super::*;

impl App {
  pub fn motion_frame(&mut self, now: Instant, scale: f32) -> MotionFrame {
    self.motion.frame(now, scale)
  }

  /// Starts the camera, post, and hit-stop triggers for one moment. The window
  /// calls this after it has already handled the matching app event, so this
  /// can never change what the auth worker sees.
  pub fn notify_motion(&mut self, moment: MotionMoment, now: Instant) {
    self.motion.notify_moment(moment, now);
  }

  /// True while the boot splash still owns the screen or is fading out.
  #[must_use]
  pub fn splash_active(&self, now: Instant) -> bool {
    self
      .splash
      .as_ref()
      .is_some_and(|splash| splash.active(now))
  }

  /// Rasterises the static boot card once per size, scale, or copy change.
  /// The window owns the text engine, so it is passed in rather than copied
  /// into the splash state.
  pub fn prepare_splash(
    &mut self,
    now: Instant,
    size: (u32, u32),
    scale: f32,
    background: crate::color::Rgb,
    text: &mut crate::ui::text::TextEngine,
  ) {
    let accent = self.theme.accent_on(background);
    if let Some(splash) = self.splash.as_mut() {
      splash.prepare(now, size, scale, background, accent, text);
    }
  }

  #[must_use]
  pub fn splash_art(&self) -> Option<&SplashArt> {
    self.splash.as_ref().and_then(SplashState::art)
  }

  /// True when the boot card is fully covering an opaque background. The GPU
  /// path uses this to keep the cached card as one texture and skip the hidden
  /// wallpaper upload.
  #[must_use]
  pub fn splash_opaque(&self, now: Instant, background: crate::color::Rgb) -> bool {
    self
      .splash
      .as_ref()
      .is_some_and(|splash| splash.opaque(now, background))
  }

  /// Offers one key to the splash. Returns true when the splash consumed it, in
  /// which case the hidden login form must not see it. Escape is never consumed.
  pub fn splash_key(&mut self, key: Key, now: Instant) -> bool {
    self
      .splash
      .as_mut()
      .is_some_and(|splash| splash.dismiss(key, now))
  }

  #[must_use]
  pub fn motion_active(&self) -> bool {
    self.motion_overlay_active() || self.motion.active()
  }

  /// True when a frame must land on the fast motion cadence. A breathing
  /// camera alone can run at half rate; a punch, glitch, or text animation
  /// cannot.
  #[must_use]
  pub fn motion_fast_active(&self) -> bool {
    let now = Instant::now();
    let splash = self
      .splash
      .as_ref()
      .is_some_and(|splash| splash.active(now));
    let pop_ms = self
      .theme
      .login
      .username
      .key_pop_ms
      .max(self.theme.login.password.key_pop_ms);
    let pop = self
      .typed_at
      .is_some_and(|at| now.saturating_duration_since(at).as_secs_f32() * 1000.0 < pop_ms);
    let flash = self.session_changed_at.is_some_and(|at| {
      now.saturating_duration_since(at).as_secs_f32() * 1000.0 < SESSION_FLASH_MS
    });
    splash || pop || flash || self.motion.fast_active()
  }

  /// True while the overlay pixels themselves can change. The window uses this
  /// to avoid a full text re-rasterisation for camera-only motion.
  #[must_use]
  pub fn motion_overlay_active(&self) -> bool {
    let now = Instant::now();
    if self
      .splash
      .as_ref()
      .is_some_and(|splash| splash.active(now))
    {
      return true;
    }
    let pop_ms = self
      .theme
      .login
      .username
      .key_pop_ms
      .max(self.theme.login.password.key_pop_ms);
    let pop = self
      .typed_at
      .is_some_and(|at| now.saturating_duration_since(at).as_secs_f32() * 1000.0 < pop_ms);
    let flash = self.session_changed_at.is_some_and(|at| {
      now.saturating_duration_since(at).as_secs_f32() * 1000.0 < SESSION_FLASH_MS
    });
    self.motion.overlay_active() || pop || flash
  }
}
