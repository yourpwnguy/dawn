//! winit application events for the window.

use super::*;

impl ApplicationHandler<UserEvent> for Handler {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    if self.window.is_some() {
      return;
    }
    match self.init_window(event_loop) {
      Ok(()) => {
        self.start_background_load();
        // The boot cut is a one-shot montage intro. It is not tied to auth and
        // cannot hold the process open, because only a success trigger is
        // allowed to delay exit. `resumed` returns early once a window exists,
        // so this fires exactly once.
        let now = Instant::now();
        // A splash is its own entrance. Playing the boot cut over it would
        // cover the title card, so the montage waits for configs without one.
        if !self.app.splash_active(now) {
          self.start_transition(TransitionTrigger::Boot, now);
        }
        self.request_redraw();
      }
      Err(error) => self.fail(event_loop, error),
    }
  }

  fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
    match event {
      UserEvent::Auth(event) => {
        let failed = matches!(event, Event::Failed(_));
        let was_authenticated = self.app.authenticated();
        self.app.handle_auth(event);
        let now = Instant::now();
        if failed {
          // A rejection gets its own optional cut. The form is already back in
          // its typing state; this is decoration on the same frame, never a
          // hold on the auth path.
          self.start_transition(TransitionTrigger::Failure, now);
          self.start_hitstop(self.app.theme().motion.hitstop.failure, now, false);
          self.app.notify_motion(MotionMoment::Failure, now);
        }
        if !was_authenticated && self.app.authenticated() {
          if self.app.theme().particles.burst_on == crate::config::BurstTrigger::Success {
            self.burst_at = Some(now);
          }
          self.start_transition(TransitionTrigger::Success, now);
          self.start_hitstop(self.app.theme().motion.hitstop.success, now, true);
          self.app.notify_motion(MotionMoment::Success, now);
        }
        self.ui_dirty = true;
        self.request_redraw();
        self.maybe_finish(event_loop);
      }
      UserEvent::Background(generation, result) => {
        let accepted = self.background.accept(generation, result, Instant::now());
        if accepted {
          let proxy = self.proxy.clone();
          let pending = Arc::clone(&self.frame_pending);
          self.background.set_waker(Some(Arc::new(move || {
            if !pending.swap(true, Ordering::AcqRel) {
              let _ = proxy.send_event(UserEvent::Frame);
            }
          })));
        }
        // A failed load still changes the warning line, so repaint either way.
        self.ui_dirty = true;
        self.blur_source_at = None;
        self.request_redraw();
      }
      UserEvent::Media => {
        self.ui_dirty = true;
        self.request_redraw();
      }
      UserEvent::Frame => {
        self.frame_pending.store(false, Ordering::Release);
        self.request_redraw();
      }
      UserEvent::DevReload => self.dev_reload(),
    }
  }

  fn window_event(
    &mut self,
    event_loop: &ActiveEventLoop,
    _window_id: WindowId,
    event: WindowEvent,
  ) {
    match event {
      WindowEvent::CloseRequested => {
        self.app.quit();
        self.finish = Some(Finish::Quit);
        event_loop.exit();
      }
      WindowEvent::Destroyed => {
        self.finish.get_or_insert(Finish::Quit);
        event_loop.exit();
      }
      WindowEvent::Resized(_) => {
        self.update_monitor();
        self.start_background_load();
        self.ui_dirty = true;
        self.blur_source_at = None;
        self.decor.clear();
        self.request_redraw();
      }
      WindowEvent::ScaleFactorChanged { .. } => {
        self.update_monitor();
        self.start_background_load();
        self.ui_dirty = true;
        self.blur_source_at = None;
        self.decor.clear();
        self.request_redraw();
      }
      WindowEvent::Focused(true) => {
        self.last_input = Instant::now();
        self.ui_dirty = true;
        if self.paused {
          self.paused = false;
          self.background.set_paused(false);
          // Resume visibly: the frozen frame may be seconds behind, and an
          // animated wallpaper would otherwise wait up to one frame delay
          // before the next scheduled repaint catches up.
          self.request_redraw();
        }
      }
      WindowEvent::Focused(false) => {
        // A hidden greeter has no reason to animate a wallpaper. Freezing it
        // also means a compositor switching away does not burn CPU. We do not
        // pause on `Occluded`: X11 without a window manager reports it for a
        // fully mapped greeter, which would freeze the first wallpaper frame
        // forever.
        self.paused = true;
        self.ui_dirty = true;
        self.background.set_paused(true);
        if self.drag.end() {
          self.drag.save();
          self.refresh_cursor();
        }
      }
      WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
      WindowEvent::CursorMoved { position, .. } => {
        self.pointer_move((position.x as f32, position.y as f32));
      }
      WindowEvent::CursorLeft { .. } => self.pointer_leave(),
      WindowEvent::MouseInput {
        state,
        button: MouseButton::Left,
        ..
      } => match state {
        ElementState::Pressed => self.pointer_press(event_loop),
        ElementState::Released => self.pointer_release(),
      },
      WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
        self.handle_key(event_loop, event);
      }
      WindowEvent::RedrawRequested => self.redraw(),
      _ => {}
    }
  }

  fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
    // A drawing failure must stop the loop so main can try the terminal
    // fallback instead of leaving a frozen window on screen.
    if self.error.is_some() {
      event_loop.exit();
      return;
    }
    if self.app.finished().is_some() {
      self.maybe_finish(event_loop);
      if self.finish.is_some() {
        return;
      }
    }

    let now = Instant::now();
    // A held frame is intentional. Do not let animation deadlines queue a
    // stream of repaints that `redraw` would only throw away; wait for the
    // deadline, then request the frame that resumes the scene.
    if let Some(until) = self.hitstop_until {
      if now < until {
        self.schedule(event_loop);
        return;
      }
      self.hitstop_until = None;
      self.ui_dirty = true;
      self.request_redraw();
    }
    if !self.paused
      && !self.idle_pause.is_zero()
      && now.duration_since(self.last_input) >= self.idle_pause
      && self.background.is_animated()
    {
      self.paused = true;
      self.background.set_paused(true);
    }

    // One animation clock now owns splash, transition, input, motion, and
    // decor. Requesting a redraw from every `about_to_wait` spun the loop as
    // fast as winit could deliver events and ignored `ControlFlow`. Instead we
    // mark what changed, request only when the deadline is due, and let
    // `schedule` wake us exactly once for the next frame.
    let splash = self.app.splash_active(now);
    let splash_shader = self.splash_shader_active(now);
    // A splash that has just ended leaves its cached card in the GPU texture.
    // Force one normal frame so the login form replaces it even when nothing
    // else on screen changed.
    if self.splash_was_active && !splash {
      self.ui_dirty = true;
    }
    self.splash_was_active = splash;
    if !splash && self.splash_ui.is_some() {
      self.splash_ui = None;
      self.ui_dirty = true;
    }
    let transition_active = self
      .transition
      .as_ref()
      .is_some_and(|transition| !transition.finished(now));
    let motion_animating = self.app.motion_active() && !self.paused;
    let overlay_animating = self.app.motion_overlay_active() && !splash_shader;
    let input_animating =
      (self.drag.is_animating() || self.button_hover_animating(now)) && !self.paused;
    let burst = self.burst_active(now);
    let decor_due = self.app.theme().decor_motion_active()
      && !self.paused
      && !splash_shader
      && self
        .last_overlay_at
        .is_none_or(|at| now.duration_since(at) >= self.app.theme().motion.decor_frame());
    let animating =
      splash || transition_active || motion_animating || input_animating || burst || decor_due;
    if (splash && !splash_shader)
      || transition_active
      || overlay_animating
      || input_animating
      || burst
      || decor_due
    {
      self.ui_dirty = true;
    }
    if animating {
      let fast =
        splash || transition_active || input_animating || burst || self.app.motion_fast_active();
      let interval = if fast { MOTION_FRAME } else { BREATH_FRAME };
      let due = self.last_frame_at + interval;
      self.next_frame_at = Some(due);
      if now >= due {
        self.request_redraw();
        self.last_frame_at = now;
        self.next_frame_at = Some(now + interval);
      }
    } else {
      self.next_frame_at = None;
    }
    // A finished cut sequence needs one normal frame so the form, not the
    // last translucent slam, is what stays on screen.
    if self.retire_transition(now) {
      self.maybe_finish(event_loop);
      if self.finish.is_some() {
        return;
      }
    }
    // The clock keeps ticking even while the wallpaper is paused: an
    // unfocused greeter on a headless X server is still visible, and a frozen
    // clock looks broken.
    if self.app.tick(&jiff::Zoned::now()) {
      self.ui_dirty = true;
      self.request_redraw();
    }
    if !self.paused
      && (self.background.fading(now) || self.background.frame_id(now) != self.last_frame_id)
    {
      self.request_redraw();
    }
    self.schedule(event_loop);
  }
}
