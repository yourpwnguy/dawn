//! Input handlers for the window.

use super::*;

impl Handler {
  pub(super) fn handle_key(&mut self, event_loop: &ActiveEventLoop, event: KeyEvent) {
    // Once a login has been committed, the window may still be painting a
    // success transition. Ignore input during that hold so an Escape can never
    // cancel a session that greetd has already accepted.
    if self.app.finished().is_some() {
      return;
    }
    // Studio shortcuts are intercepted before the login key map. Escape is
    // deliberately absent: it stays the normal "clear the field" key.
    if self.dev.is_some() && event.state == ElementState::Pressed {
      match &event.logical_key {
        WinitKey::Named(NamedKey::F1) => {
          if let Some(dev) = self.dev.as_mut()
            && dev.hud_enabled
          {
            dev.hud_visible = !dev.hud_visible;
            self.ui_dirty = true;
            self.request_redraw();
          }
          return;
        }
        WinitKey::Named(NamedKey::F5) => {
          self.dev_reload();
          return;
        }
        WinitKey::Character(value)
          if self.modifiers.control_key() && value.as_str().eq_ignore_ascii_case("q") =>
        {
          self.app.quit();
          self.finish = Some(Finish::Quit);
          event_loop.exit();
          return;
        }
        _ => {}
      }
    }
    let Some(key) = translate_key(&event, self.modifiers) else {
      return;
    };
    self.last_input = Instant::now();
    if self.paused {
      self.paused = false;
      self.background.set_paused(false);
    }
    // The splash hides the form, so it owns every key except Escape. Escape
    // keeps its normal app meaning (clear/cancel) even though there is nothing
    // visible to clear yet.
    let now = Instant::now();
    if self.app.splash_active(now) {
      let consumed = self.app.splash_key(key, now);
      if consumed || key == Key::Escape {
        if !consumed {
          self.app.key(key);
        }
        self.ui_dirty = true;
        self.request_redraw();
      }
      return;
    }
    let submit_before = self.app.submit_epoch();
    self.app.key(key);
    if self.app.submit_epoch() != submit_before {
      self.on_submit(Instant::now());
    }
    self.ui_dirty = true;
    self.request_redraw();
    self.maybe_finish(event_loop);
  }

  pub(super) fn refresh_cursor(&self) {
    let Some(window) = &self.window else {
      return;
    };
    let icon = if self.drag.is_dragging() {
      CursorIcon::Grabbing
    } else if self.drag.hover_is_draggable() {
      CursorIcon::Grab
    } else {
      CursorIcon::Default
    };
    window.set_cursor(icon);
  }

  pub(super) fn pointer_move(&mut self, cursor: (f32, f32)) {
    self.cursor = cursor;
    self.pointer_inside = true;
    self.last_input = Instant::now();
    let Some(window) = &self.window else {
      return;
    };
    let size = window.inner_size();
    if self.drag.is_dragging() {
      if self
        .drag
        .move_to(cursor, (size.width as f32, size.height as f32))
      {
        self.ui_dirty = true;
        self.request_redraw();
      }
    } else if self.drag.hover(&self.regions, Some(cursor)) {
      self.ui_dirty = true;
      self.refresh_cursor();
    }
    // Parallax follows the pointer even when nothing else changed, so a static
    // login screen still needs a fresh overlay frame on every move. Hover
    // already asked for one above, but the guard keeps this path cheap when
    // the effect is off.
    if self.app.theme().motion.parallax > 0.0 {
      self.ui_dirty = true;
      self.request_redraw();
    }
  }

  pub(super) fn pointer_press(&mut self, event_loop: &ActiveEventLoop) {
    let Some(window) = &self.window else {
      return;
    };
    if self.app.finished().is_some() {
      return;
    }
    let scale = window.scale_factor() as f32;
    self.last_input = Instant::now();
    self.ui_dirty = true;
    if self.drag.begin(&self.regions, self.cursor, scale) {
      self.refresh_cursor();
      self.request_redraw();
      return;
    }
    // A non-draggable field still responds to a click by taking focus.
    let id = self
      .regions
      .iter()
      .rev()
      .find(|region| region.contains(self.cursor))
      .map(|region| region.id.as_str());
    // A click on the submit button is the pointer equivalent of Enter. It
    // goes through the same key policy, so an empty username still moves focus
    // to the first field instead of starting an auth request.
    if id == Some("login.button") {
      let submit_before = self.app.submit_epoch();
      self.app.key(Key::Enter);
      if self.app.submit_epoch() != submit_before {
        self.on_submit(Instant::now());
      }
      self.request_redraw();
      self.maybe_finish(event_loop);
      return;
    }
    // Media transport buttons are ordinary hit regions. We parse the id rather
    // than comparing strings so a malformed id cannot be mistaken for a field,
    // and an index that no longer has a live handle simply does nothing.
    if let Some((index, control)) = id.and_then(media_control_id) {
      if let Some(handle) = self._media.get(index) {
        // A dropped worker is the only reason this can fail, and the next
        // snapshot wake-up will repaint anyway. The click still belongs to the
        // widget, so we return instead of falling through to field focus.
        let _ = handle.control(control);
        self.ui_dirty = true;
        self.request_redraw();
      }
      return;
    }
    let focused = match id {
      Some("login.username") => self.app.focus_field(Focus::Username),
      Some("login.password") => self.app.focus_field(Focus::Password),
      _ => false,
    };
    if focused {
      self.request_redraw();
    }
  }

  pub(super) fn pointer_release(&mut self) {
    if self.drag.end() {
      self.drag.save();
      self.refresh_cursor();
      self.ui_dirty = true;
      self.request_redraw();
    }
  }

  pub(super) fn pointer_leave(&mut self) {
    self.pointer_inside = false;
    if self.drag.hover(&self.regions, None) {
      self.ui_dirty = true;
      self.refresh_cursor();
    }
    if self.drag.end() {
      self.drag.save();
      self.refresh_cursor();
      self.ui_dirty = true;
      self.request_redraw();
    }
  }
}
