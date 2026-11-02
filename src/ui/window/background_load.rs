//! Background Load handlers for the window.

use super::*;

impl Handler {
  pub(super) fn start_background_load(&mut self) {
    let Some(window) = &self.window else {
      return;
    };
    let size = window.inner_size();
    if size.width == 0 || size.height == 0 {
      return;
    }
    let Some(request) = self
      .background
      .request((size.width, size.height), self.monitor.as_deref())
    else {
      return;
    };
    let proxy = self.proxy.clone();
    let spawned = std::thread::Builder::new()
      .name("dawn-background".to_string())
      .spawn(move || {
        let (generation, result) = request.load();
        let _ = proxy.send_event(UserEvent::Background(generation, result));
      });
    if let Err(error) = spawned {
      self
        .background
        .fail(format!("cannot start the loader thread: {error}"));
      self.ui_dirty = true;
      self.request_redraw();
    }
  }
}
