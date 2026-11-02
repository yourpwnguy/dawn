//! Surface code for the GPU compositor.

use super::*;

impl GpuRenderer {
  pub fn resize(&mut self, size: (u32, u32)) -> Result<bool, String> {
    let size = (size.0.max(1), size.1.max(1));
    if size.0 > self.max_dimension || size.1 > self.max_dimension {
      return Err(format!(
        "window {}x{} exceeds the GPU texture limit of {}",
        size.0, size.1, self.max_dimension
      ));
    }
    let changed = size != (self.config.width, self.config.height);
    if !changed && !self.reconfigure {
      return Ok(false);
    }
    self.config.width = size.0;
    self.config.height = size.1;
    self.surface.configure(&self.device, &self.config);
    self.reconfigure = false;
    if changed {
      self.ui = TextureSlot::new(&self.device, size, "dawn-ui");
      self.decor = TextureSlot::new(&self.device, size, "dawn-decor");
      self.globals.screen = [size.0 as f32, size.1 as f32];
      self.ui_uploaded = false;
      self.decor_key = None;
      self.rebuild_bind_group();
    }
    Ok(changed)
  }

  /// Uploads the full-screen transparent UI overlay. This is only called when
  /// the UI changed, not on wallpaper frames.
  pub fn set_ui(&mut self, pixmap: &Pixmap) -> Result<(), String> {
    let size = (pixmap.width(), pixmap.height());
    if size.0 == 0 || size.1 == 0 {
      return Ok(());
    }
    if size != self.ui.size {
      self.ui = TextureSlot::new(&self.device, size, "dawn-ui");
      self.rebuild_bind_group();
    }
    self.upload(&self.ui.texture, pixmap);
    self.ui_uploaded = true;
    Ok(())
  }

  /// Uploads the static decor texture when its identity changed, or turns it
  /// off. The live UI overlay is composited over this texture by the shader,
  /// which keeps a full-screen still image out of the per-frame CPU paint.
  pub fn set_decor(&mut self, pixmap: Option<&Pixmap>, id: u64) -> Result<(), String> {
    let Some(pixmap) = pixmap else {
      self.globals.decor_on = 0;
      self.decor_key = None;
      return Ok(());
    };
    let size = pixmap_size(pixmap);
    if self.decor.size != size {
      self.decor = TextureSlot::new(&self.device, size, "dawn-decor");
      self.rebuild_bind_group();
    }
    if self.decor_key != Some(id) {
      self.upload(&self.decor.texture, pixmap);
      self.decor_key = Some(id);
    }
    self.globals.decor_on = 1;
    Ok(())
  }

  /// Stores the camera and post values for the next present. This is pure
  /// uniform state, so it costs nothing on frames where the UI overlay is not
  /// re-uploaded. The software path has no equivalent: it ignores both.
  pub fn set_effects(&mut self, camera: CameraFrame, post: PostFrame) {
    let scale = if camera.scale.is_finite() {
      camera.scale.clamp(0.01, 4.0)
    } else {
      1.0
    };
    let dx = if camera.dx.is_finite() {
      camera.dx
    } else {
      0.0
    };
    let dy = if camera.dy.is_finite() {
      camera.dy
    } else {
      0.0
    };
    self.globals.camera = [scale, dx, dy, 0.0];
    self.globals.post = [
      post.chromatic.clamp(0.0, 1.0),
      post.vignette.clamp(0.0, 1.0),
      post.glitch.clamp(0.0, 1.0),
      if post.glitch_seed.is_finite() {
        post.glitch_seed.max(0.0)
      } else {
        0.0
      },
    ];
  }

  /// Sets the shader-drawn splash underline. `None` hides it. This costs one
  /// uniform upload, not another full-screen texture.
  pub fn set_splash(&mut self, splash: Option<SplashOverlay>) {
    match splash {
      Some(splash) => {
        self.globals.splash = splash.rect;
        self.globals.splash_color = splash.color;
        self.globals.splash_on = 1;
      }
      None => self.globals.splash_on = 0,
    }
  }

  /// Updates the wallpaper uniforms and, when the frame actually changed,
  /// uploads the new current and/or previous frame.
  pub fn set_background(&mut self, snapshot: &BackgroundSnapshot) -> Result<(), String> {
    let mut mode = MODE_IMAGE;
    let mut top = [0.0, 0.0, 0.0, 1.0];
    let mut bottom = [0.0, 0.0, 0.0, 1.0];
    let mut has_prev = 0;

    match snapshot.kind {
      SnapshotKind::Solid(color) => {
        mode = MODE_SOLID;
        top = color_array(color);
      }
      SnapshotKind::Gradient(color) => {
        mode = MODE_GRADIENT;
        top = color_array(color);
        bottom = color_array(shade(color, 0.72));
      }
      SnapshotKind::Image => {
        let Some(current) = &snapshot.current else {
          // Defensive: an image snapshot without a frame means "nothing has
          // arrived yet", which is the placeholder's job.
          mode = MODE_SOLID;
          top = [0.0, 0.0, 0.0, 1.0];
          self.globals = Globals {
            top,
            bottom,
            mode,
            has_prev: 0,
            ..self.globals
          };
          return Ok(());
        };
        let key = FrameKey::of(snapshot.frame_id, current);
        if self.bg_key != Some(key) || self.background.size != pixmap_size(current) {
          self.ensure_background_size(pixmap_size(current));
          self.upload(&self.background.texture, current);
          self.bg_key = Some(key);
        }
        self.globals.bg_size = [current.width() as f32, current.height() as f32];

        if let Some(previous) = &snapshot.previous {
          let key = FrameKey::pointer(previous);
          if self.prev_key != Some(key) || self.previous.size != pixmap_size(previous) {
            self.ensure_previous_size(pixmap_size(previous));
            self.upload(&self.previous.texture, previous);
            self.prev_key = Some(key);
          }
          self.globals.prev_size = [previous.width() as f32, previous.height() as f32];
          has_prev = 1;
        }
      }
    }

    self.globals.top = top;
    self.globals.bottom = bottom;
    self.globals.mode = mode;
    self.globals.has_prev = has_prev;
    self.globals.opacity = snapshot.opacity.clamp(0.0, 1.0);
    if has_prev == 0 {
      // Drop the old key so a future crossfade that happens to reuse an
      // allocation address still gets a fresh upload.
      self.prev_key = None;
    }
    Ok(())
  }
}
