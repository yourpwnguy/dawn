//! Dev hot-reload handler for the window.

use super::*;

impl Handler {
  pub(super) fn dev_reload(&mut self) {
    let Some(dev) = self.dev.as_ref() else {
      return;
    };
    let source = dev.source.clone();
    let host_gpu = dev.host_gpu.clone();
    let previous = dev.state.last_good.clone();

    match crate::dev::prepare_reload(&source, &host_gpu, &previous) {
      Ok(reload) => {
        let crate::dev::Reload {
          config,
          plugins,
          warning,
          background,
          rebuild_text,
          cost_ms,
        } = reload;
        let swap_started = Instant::now();
        if rebuild_text {
          self.text = TextEngine::new(&config.ui.font);
        }
        if background.is_reload() {
          // `!decode` means the image path is unchanged, so the loaded frame
          // set stays in place and only the cheap background fields change.
          self.background.reconfigure(&config, !background.decode());
        }
        self.app.reload(config.clone(), plugins, warning);
        let total_ms = cost_ms + swap_started.elapsed().as_secs_f32() * 1000.0;
        if let Some(dev) = self.dev.as_mut() {
          dev.state.success(config, total_ms);
        }
        if background.is_reload() {
          self.start_background_load();
        }
        self.ui_dirty = true;
        self.blur_source_at = None;
        self.decor.clear();
        self.request_redraw();
        tracing::info!(cost_ms = total_ms, "dev reload");
      }
      Err(error) => {
        let logged = self
          .dev
          .as_mut()
          .is_some_and(|dev| dev.state.failure(error.clone()));
        if logged {
          tracing::warn!(
            error = %error,
            "dev reload failed, keeping the last good theme"
          );
        }
        // A failed reload still changes the HUD error line, so repaint.
        self.ui_dirty = true;
        self.request_redraw();
      }
    }
  }
}
