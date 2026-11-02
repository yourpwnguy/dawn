//! Redraw handlers for the window.

use super::*;

impl Handler {
  pub(super) fn redraw(&mut self) {
    // Hit-stop holds the last presented frame. Returning here keeps the pixels
    // exactly where they were while the event loop stays free to deliver auth
    // and input. `about_to_wait` wakes us at the deadline.
    let now = Instant::now();
    if self.hitstop_active(now) {
      return;
    }
    if self.hitstop_until.is_some_and(|until| now >= until) {
      self.hitstop_until = None;
      self.ui_dirty = true;
    }
    let Some(window) = self.window.clone() else {
      return;
    };
    let Some(mut presenter) = self.presenter.take() else {
      return;
    };
    let size = window.inner_size();
    if size.width == 0 || size.height == 0 {
      // A zero-sized surface happens during resize. Skipping the frame is
      // correct; winit will ask again once the window has a size.
      self.presenter = Some(presenter);
      return;
    }
    let before = self.last_frame_id;
    let result = match &mut presenter {
      Presenter::Gpu(renderer) => self.redraw_gpu(renderer, &window, (size.width, size.height)),
      Presenter::Cpu(surface) => self.redraw_cpu(surface, &window, (size.width, size.height)),
    };
    self.presenter = Some(presenter);
    if let Err(error) = result {
      tracing::error!(%error, "cannot present the frame");
      self.error = Some(WindowError::Graphics(error));
    }
    self.redraw_frames = self.redraw_frames.saturating_add(1);
    if self.last_frame_id != before {
      self.wallpaper_frames = self.wallpaper_frames.saturating_add(1);
    }
    self.log_cadence(Instant::now());
  }

  /// Emits one debug line per second with the distinct redraw and wallpaper
  /// frame rates. This is the number that matters for a video wallpaper: a
  /// decor animation can drive 60 redraws while the video is only presented
  /// 30 times.
  fn log_cadence(&mut self, now: Instant) {
    let elapsed = now.saturating_duration_since(self.frame_window);
    if elapsed < Duration::from_secs(1) {
      return;
    }
    let seconds = elapsed.as_secs_f32();
    tracing::debug!(
      redraw_fps = self.redraw_frames as f32 / seconds,
      wallpaper_fps = self.wallpaper_frames as f32 / seconds,
      "presentation cadence"
    );
    self.frame_window = now;
    self.redraw_frames = 0;
    self.wallpaper_frames = 0;
  }

  /// Builds the per-frame view and starts the motion clock. Both presentation
  /// paths call this once per redraw so they cannot disagree about boot timing
  /// or widget values.
  pub(super) fn frame_state(&mut self, window: &Window, need_view: bool) -> FrameState {
    let scale = window.scale_factor() as f32;
    let frame_now = Instant::now();
    let hud = match self.dev.as_mut() {
      Some(dev) => {
        dev.fps.tick(frame_now);
        dev.hud()
      }
      None => None,
    };
    let reduced = self.app.theme().motion.reduced || self.app.theme().low_power;
    self.drag.advance(frame_now, reduced);
    let motion = self.app.motion_frame(frame_now, scale);
    let background_color = self
      .background
      .config()
      .color_for_monitor(self.monitor.as_deref());
    let view = if need_view {
      self.app.view()
    } else {
      View::default()
    };
    // Beat bursts restart from the most recent beat instant instead of keeping
    // a spawn table. This is the same one-shot emitter the success trigger uses,
    // so the count cap and the low-power freeze apply unchanged.
    let theme = self.app.theme();
    if !reduced
      && theme.particles.enabled
      && theme.particles.burst_on_beat
      && theme.motion.beat.bpm > 0.0
    {
      let age_ms = motion.beat_phase * motion.beat_ms;
      let at = frame_now
        .checked_sub(Duration::from_secs_f32(age_ms / 1000.0))
        .unwrap_or(frame_now);
      self.burst_at = Some(self.burst_at.map_or(at, |old| old.max(at)));
    }
    let zoned = jiff::Zoned::now();
    let warning = need_view
      .then(|| self.background.warning().map(str::to_string))
      .flatten();
    let size = window.inner_size();
    FrameState {
      view,
      zoned,
      warning,
      started: self.started,
      frame_now,
      scale,
      motion,
      background_color,
      cursor: self.pointer_inside.then_some(self.cursor),
      screen: (size.width as f32, size.height as f32),
      burst_at: self.burst_at,
      hud,
    }
  }

  /// The original full CPU path. It is both the no-GPU fallback and the path
  /// `--screenshot` exercises, so any visual change must keep working here.
  pub(super) fn redraw_cpu(
    &mut self,
    surface: &mut softbuffer::Surface<Arc<Window>, Arc<Window>>,
    window: &Window,
    size: (u32, u32),
  ) -> Result<(), String> {
    let (Some(width), Some(height)) = (NonZeroU32::new(size.0), NonZeroU32::new(size.1)) else {
      return Ok(());
    };
    let reusable = self
      .frame_canvas
      .as_ref()
      .is_some_and(|canvas| canvas.width() == width.get() && canvas.height() == height.get());
    if !reusable {
      self.frame_canvas = Canvas::new(width.get(), height.get());
    }
    let state = self.frame_state(window, true);
    self.app.prepare_splash(
      state.frame_now,
      (width.get(), height.get()),
      state.scale,
      state.background_color,
      &mut self.text,
    );
    let scene = state.scene(&self.app, self.monitor.as_deref(), &self.drag, false);
    let transition_frame = self
      .transition
      .as_ref()
      .and_then(|transition| transition.frame(state.frame_now));
    let Some(canvas) = self.frame_canvas.as_mut() else {
      return Err("cannot allocate the frame canvas".to_string());
    };
    self.regions = render::paint(
      canvas,
      &mut self.text,
      &mut self.background,
      &mut self.glass,
      &scene,
    );
    // The cut sequence is a post-pass over the finished frame: opaque during
    // the cuts, translucent during the release. The CPU path has nowhere else
    // to put a wallpaper-covering flash.
    if let Some(frame) = &transition_frame {
      transition::paint(canvas, &mut self.text, frame, state.scale);
    }
    self.last_overlay_at = Some(Instant::now());
    self.last_frame_id = self.background.frame_id(state.frame_now);
    // A rebuilt region list can put a different widget under a stationary
    // pointer (a dragged card, a changed field height). Re-evaluate hover so
    // the button transition does not stay stuck on its old target.
    if self.pointer_inside {
      self.drag.hover(&self.regions, Some(self.cursor));
    }

    surface
      .resize(width, height)
      .map_err(|error| error.to_string())?;
    let mut buffer = surface.buffer_mut().map_err(|error| error.to_string())?;
    canvas.to_argb(&mut buffer[..]);
    buffer.present().map_err(|error| error.to_string())
  }

  /// GPU compositor path: upload the wallpaper, rasterise the transparent UI
  /// overlay only when something in it changed, and present one triangle.
  pub(super) fn redraw_gpu(
    &mut self,
    renderer: &mut gpu::GpuRenderer,
    window: &Window,
    size: (u32, u32),
  ) -> Result<(), String> {
    if size.0 == 0 || size.1 == 0 {
      return Ok(());
    }
    let resized = renderer.resize(size)?;
    // A resize recreates the UI texture. Forget the cached splash identity so
    // the next covering frame uploads the card into the new texture.
    if resized {
      self.splash_ui = None;
    }
    let now = Instant::now();
    let needs_glass = {
      let theme = self.app.theme();
      theme.blur.enabled || theme.noise
    };
    let transition_frame = self
      .transition
      .as_ref()
      .and_then(|transition| transition.frame(now));
    // An opaque transition frame is a full-screen flash. There is no reason to
    // rasterise the login overlay underneath it, so skip that work and keep
    // the hit regions empty for the duration. Translucent release frames need
    // the real UI because they reveal it.
    let transition_covers = transition_frame
      .as_ref()
      .is_some_and(|frame| frame.fill.a == 255);
    // A camera-only frame does not need a View, and building one clones the
    // username, status, battery, and system snapshot. Text shaping is much
    // more expensive than the GPU transform, so only build it when the overlay
    // can actually change (or when a glass panel needs background pixels).
    let need_view = self.ui_dirty
      || resized
      || transition_frame.is_some()
      || self.burst_active(now)
      || needs_glass
      // The shader needs the splash title and phase even on frames where the
      // UI overlay itself is clean.
      || self.app.splash_active(now);
    let state = self.frame_state(window, need_view);
    // An opaque title card hides the wallpaper, so there is no reason to pay
    // for a full-screen video upload until the card starts fading.
    let splash_opaque = state
      .view
      .splash
      .as_ref()
      .is_some_and(|splash| splash.opaque(state.background_color));
    self.app.prepare_splash(
      state.frame_now,
      (size.0, size.1),
      state.scale,
      state.background_color,
      &mut self.text,
    );
    let splash_art = self.app.splash_art();
    let splash_shader = splash_opaque && splash_art.is_some();
    // The shader owns the vignette now, but an opaque title card must not be
    // darkened by it. The fade still gets the same vignette as the CPU path.
    let mut post = state.motion.post;
    if splash_opaque {
      post.vignette = 0.0;
    }
    // The camera and the shader-side post effects are compositor state, not
    // overlay pixels, so they are uploaded even when the UI itself is clean.
    renderer.set_effects(state.motion.camera, post);
    // Bake the still ornament into one texture once the card has settled. The
    // live overlay then only carries the moving decor and the form, which is
    // what keeps the video at source rate on a fullscreen integrated GPU.
    let decor_enabled = !splash_opaque
      && state.motion.settled
      && state.motion.card_dx.abs() <= 0.01
      && state.motion.card_dy.abs() <= 0.01
      && state.motion.opacity >= 0.999
      && state.view.splash.is_none()
      && self.transition.is_none()
      && self.drag.offset("login") == (0.0, 0.0)
      && self.app.theme().login.layout.is_full_bleed();
    let decor_cached = if decor_enabled {
      let scene = state.scene(&self.app, self.monitor.as_deref(), &self.drag, true);
      self
        .decor
        .prepare(
          &scene,
          &mut self.text,
          &self.glass,
          (size.0, size.1),
          state.scale,
          true,
        )
        .is_some()
    } else {
      self.decor.clear();
      false
    };
    renderer.set_decor(
      if decor_cached { self.decor.art() } else { None },
      self.decor.id(),
    )?;
    let snapshot = self.background.snapshot(state.frame_now);
    let mut ui_dirty =
      self.ui_dirty || resized || transition_frame.is_some() || self.burst_active(state.frame_now);
    let theme = self.app.theme();

    // Glass panels are the one thing that still needs CPU background pixels.
    // Keep the same refresh cadence as the CPU blur cache instead of repainting
    // the wallpaper on every video frame. An opaque splash has no visible
    // glass, so skip the whole cache while it is up.
    if !splash_opaque && (theme.blur.enabled || theme.noise) {
      let needs_canvas = self
        .blur_canvas
        .as_ref()
        .is_none_or(|canvas| canvas.width() != size.0 || canvas.height() != size.1);
      if needs_canvas {
        self.blur_canvas = Canvas::new(size.0, size.1);
        self.blur_source_at = None;
        ui_dirty = true;
      }
      if theme.blur.enabled {
        let due = self
          .blur_source_at
          .is_none_or(|at| state.frame_now.duration_since(at) >= theme.blur.refresh);
        if due
          && (self.glass.cached_frame() != snapshot.frame_id || !self.glass.has_blur())
          && let Some(canvas) = self.blur_canvas.as_mut()
        {
          self.background.paint_snapshot(canvas, state.frame_now);
          self.blur_source_at = Some(state.frame_now);
        }
      }
      if let Some(canvas) = self.blur_canvas.as_ref() {
        // The GPU renderer owns the blur pipeline, so the cache asks it first.
        // A failure returns `None` and the cache falls back to the CPU worker
        // exactly as before.
        let changed = self.glass.prepare_with(
          canvas,
          snapshot.frame_id,
          state.frame_now,
          theme,
          state.scale,
          snapshot.animated,
          |source, blur, scale| renderer.blur(source, blur, scale).ok(),
        );
        ui_dirty |= changed;
      }
    }

    if splash_shader {
      // The static card is one texture. Upload it when the title, size, or
      // palette changes, then let the shader animate the two-pixel underline
      // from uniforms. That removes a full-screen upload per splash frame.
      if let Some(art) = splash_art {
        if self.splash_ui != Some(art.id()) {
          renderer.set_ui(art.pixmap())?;
          self.splash_ui = Some(art.id());
        }
        let underline = state
          .view
          .splash
          .as_ref()
          .and_then(|view| art.underline(view));
        renderer.set_splash(underline.map(|line| gpu::SplashOverlay {
          rect: line.rect,
          color: line.color,
        }));
        self.regions.clear();
        self.last_overlay_at = Some(Instant::now());
        self.ui_dirty = false;
      }
    } else {
      self.splash_ui = None;
      renderer.set_splash(None);
      if ui_dirty {
        let scene = state.scene(&self.app, self.monitor.as_deref(), &self.drag, true);
        let needs_canvas = self
          .ui_canvas
          .as_ref()
          .is_none_or(|canvas| canvas.width() != size.0 || canvas.height() != size.1);
        if needs_canvas {
          self.ui_canvas = Canvas::new(size.0, size.1);
        }
        let Some(canvas) = self.ui_canvas.as_mut() else {
          return Err("cannot allocate the UI overlay".to_string());
        };
        canvas.clear_transparent();
        if transition_covers {
          // The cut covers every pixel, so the overlay is complete without the
          // scanlines or the form. Clearing the regions also makes a stray
          // click during the flash harmless.
          self.regions.clear();
        } else {
          // The GPU path keeps the wallpaper in its own texture, so the CRT
          // overlay goes on the transparent UI layer first. The form then
          // paints over it, which keeps the same wallpaper-under-text
          // ordering as the CPU path. A covering splash hides the scanlines
          // too.
          if !splash_opaque {
            render::draw_scanlines(canvas, &scene);
          }
          self.regions =
            render::paint_overlay(canvas, &mut self.text, &self.glass, &scene, decor_cached);
        }
        if let Some(frame) = &transition_frame {
          transition::paint(canvas, &mut self.text, frame, state.scale);
        }
        self.last_overlay_at = Some(Instant::now());
        renderer.set_ui(canvas.pixmap())?;
        if self.pointer_inside {
          self.drag.hover(&self.regions, Some(self.cursor));
        }
        self.ui_dirty = false;
      }
    }

    if !splash_opaque {
      renderer.set_background(&snapshot)?;
    }
    renderer.present()?;
    self.last_frame_id = snapshot.frame_id;
    Ok(())
  }
}
