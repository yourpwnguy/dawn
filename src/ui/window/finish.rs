//! Finish handlers for the window.

use super::*;

impl Handler {
  pub(super) fn maybe_finish(&mut self, event_loop: &ActiveEventLoop) {
    let Some(finish) = self.app.finished() else {
      return;
    };
    // A `success` transition is the only reason a finished login waits. Auth
    // and the session launch are already committed by this point; this hold
    // only delays the process exit, and only for the capped duration. The
    // default `enter` trigger never reaches this branch.
    if finish == Finish::Started
      && (self.transition_holds(Instant::now()) || self.hitstop_active(Instant::now()))
    {
      self.request_redraw();
      return;
    }
    self.finish = Some(finish);
    self.app.audio().stop();
    event_loop.exit();
  }

  /// True while a success-triggered sequence is still worth painting. The
  /// `MAX_SUCCESS_HOLD` clamp is the lockout guard: even if a config asks for a
  /// ten-second slam, the greeter leaves for the session after two seconds.
  pub(super) fn transition_holds(&self, now: Instant) -> bool {
    self.transition.as_ref().is_some_and(|transition| {
      transition.trigger().accepts(TransitionTrigger::Success)
        && !transition.finished(now)
        && transition.elapsed(now) < MAX_SUCCESS_HOLD
    })
  }

  /// True while the last presented frame is deliberately held. The deadline is
  /// always in the future when this answers true, so callers can treat it as a
  /// reason to skip a repaint rather than a reason to exit.
  pub(super) fn hitstop_active(&self, now: Instant) -> bool {
    self.hitstop_until.is_some_and(|until| now < until)
  }

  /// Starts a hit-stop for one moment. A zero duration is a no-op. Success is
  /// clamped against `MAX_SUCCESS_HOLD`; the other moments never hold the exit.
  pub(super) fn start_hitstop(&mut self, duration: Duration, now: Instant, success: bool) {
    let duration = clamped_hitstop(duration, success);
    if duration.is_zero() {
      return;
    }
    let until = now + duration;
    self.hitstop_until = Some(
      self
        .hitstop_until
        .map_or(until, |current| current.max(until)),
    );
  }

  /// Applies the ENCORE triggers for a submit. The auth request has already
  /// been handed to the worker, so this only affects what the window paints.
  pub(super) fn on_submit(&mut self, now: Instant) {
    self.start_transition(TransitionTrigger::Enter, now);
    let hitstop = self.app.theme().motion.hitstop.submit;
    self.start_hitstop(hitstop, now, false);
    self.app.notify_motion(MotionMoment::Submit, now);
  }

  /// Starts the configured sequence for `moment`, if this table accepts that
  /// moment and nothing is already running. The auth request has already left
  /// the UI thread when this is called; nothing here talks to the worker.
  pub(super) fn start_transition(&mut self, moment: TransitionTrigger, now: Instant) {
    let config = self.app.theme().login.transition.clone();
    let Some(started) = Transition::start(&config, now) else {
      return;
    };
    if !started.trigger().accepts(moment) {
      return;
    }
    if self
      .transition
      .as_ref()
      .is_some_and(|active| !active.finished(now))
    {
      return;
    }
    self.transition = Some(started);
    self.ui_dirty = true;
    self.request_redraw();
  }

  /// Drops a finished sequence and asks for a normal frame. Returns true when
  /// something changed, so the caller can re-check a pending finish.
  pub(super) fn retire_transition(&mut self, now: Instant) -> bool {
    if self
      .transition
      .as_ref()
      .is_some_and(|transition| transition.finished(now))
    {
      self.transition = None;
      self.ui_dirty = true;
      self.request_redraw();
      return true;
    }
    false
  }

  pub(super) fn fail(&mut self, event_loop: &ActiveEventLoop, error: WindowError) {
    tracing::error!(%error, "window frontend failed");
    self.error = Some(error);
    event_loop.exit();
  }
}
