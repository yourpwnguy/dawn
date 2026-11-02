//! Schedule handlers for the window.

use super::*;

impl Handler {
  pub(super) fn button_hover_animating(&self, now: Instant) -> bool {
    let button = &self.app.theme().login.button;
    if !button.show || !button.has_motion() {
      return false;
    }
    let duration = Duration::from_millis(button.hover_ms.max(0.0) as u64);
    let looping =
      button.effect != crate::config::ButtonEffect::None && self.drag.is_hovered("login.button");
    looping
      || self
        .drag
        .hover_transition_active("login.button", now, duration)
  }

  /// True when the GPU path can animate the splash underline from shader
  /// uniforms without re-rasterising the UI overlay. The CPU path keeps the
  /// normal cached-bitmap paint.
  pub(super) fn splash_shader_active(&self, now: Instant) -> bool {
    if !matches!(self.presenter, Some(Presenter::Gpu(_))) {
      return false;
    }
    let background = self
      .background
      .config()
      .color_for_monitor(self.monitor.as_deref());
    self.app.splash_art().is_some() && self.app.splash_opaque(now, background)
  }

  /// True while the success burst is still painting. The burst has no mutable
  /// particles: it is a pure function of age, so "active" is just a time
  /// window.
  pub(super) fn burst_active(&self, now: Instant) -> bool {
    self.burst_at.is_some_and(|at| {
      now.saturating_duration_since(at).as_secs_f32() < crate::config::BURST_LIFETIME
    })
  }

  /// Chooses the next wake-up: the earliest of the clock tick, a wallpaper
  /// frame, and a motion frame. When everything is static this is a plain
  /// one-second wait.
  pub(super) fn schedule(&self, event_loop: &ActiveEventLoop) {
    let now = Instant::now();
    let mut next = now + CLOCK_TICK;
    if let Some(until) = self.hitstop_until {
      next = next.min(until);
    }
    if let Some(deadline) = self.background.next_deadline(now) {
      next = next.min(deadline);
    }
    if let Some(due) = self.next_frame_at {
      next = next.min(due);
    }
    event_loop.set_control_flow(ControlFlow::WaitUntil(next));
  }
}
