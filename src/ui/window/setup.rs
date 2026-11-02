//! Setup handlers for the window.

use super::*;

impl Handler {
  pub(super) fn init_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), WindowError> {
    let (width, height) = self.initial_size;
    let mut attributes = Window::default_attributes()
      .with_title(self.window_title())
      .with_inner_size(LogicalSize::new(width, height));
    if self.fullscreen {
      attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
    }

    let window = Arc::new(
      event_loop
        .create_window(attributes)
        .map_err(|error| WindowError::Graphics(error.to_string()))?,
    );
    let size = window.inner_size();
    let presenter = if !self.gpu.enabled {
      tracing::info!("GPU compositor disabled in [gpu]");
      Self::cpu_presenter(&window)?
    } else if self.gpu.fallback == GpuFallback::Cpu {
      tracing::info!("GPU compositor disabled by gpu.fallback = \"cpu\"");
      Self::cpu_presenter(&window)?
    } else {
      match gpu::GpuRenderer::new(
        Arc::clone(&window),
        event_loop.owned_display_handle(),
        (size.width, size.height),
        &self.gpu,
      ) {
        Ok(renderer) => {
          tracing::info!("using GPU compositor");
          Presenter::Gpu(Box::new(renderer))
        }
        Err(error) => {
          tracing::warn!(%error, "GPU compositor unavailable");
          self.gpu_presenter_after_failure(&window, event_loop, size, error)?
        }
      }
    };

    self.monitor = window.current_monitor().and_then(|monitor| monitor.name());
    self.window = Some(window);
    self.presenter = Some(presenter);
    self.ui_dirty = true;
    Ok(())
  }

  pub(super) fn cpu_presenter(window: &Arc<Window>) -> Result<Presenter, WindowError> {
    let context = softbuffer::Context::new(Arc::clone(window))
      .map_err(|error| WindowError::Graphics(error.to_string()))?;
    let surface = softbuffer::Surface::new(&context, Arc::clone(window))
      .map_err(|error| WindowError::Graphics(error.to_string()))?;
    Ok(Presenter::Cpu(surface))
  }

  /// Applies `[gpu].fallback` after the configured adapter failed.
  ///
  /// The software retry rebuilds a copy with `backend = "software"`,
  /// `device = "cpu"`, and `adapter = "auto"` on purpose. A name or index that
  /// only made sense for the adapter that just failed should not stop the last
  /// chance from trying lavapipe or llvmpipe. `chain` then degrades to
  /// softbuffer; `software` and `none` return an error so main can reach the
  /// terminal fallback.
  pub(super) fn gpu_presenter_after_failure(
    &self,
    window: &Arc<Window>,
    event_loop: &ActiveEventLoop,
    size: winit::dpi::PhysicalSize<u32>,
    error: String,
  ) -> Result<Presenter, WindowError> {
    if matches!(
      self.gpu.fallback,
      GpuFallback::Chain | GpuFallback::Software
    ) {
      let mut fallback = self.gpu.clone();
      fallback.backend = GpuBackend::Software;
      fallback.device = GpuDevice::Cpu;
      fallback.adapter = "auto".to_string();
      match gpu::GpuRenderer::new(
        Arc::clone(window),
        event_loop.owned_display_handle(),
        (size.width, size.height),
        &fallback,
      ) {
        Ok(renderer) => {
          tracing::info!("using software GPU compositor");
          return Ok(Presenter::Gpu(Box::new(renderer)));
        }
        Err(software_error) => {
          if self.gpu.fallback == GpuFallback::Chain {
            tracing::warn!(
              %software_error,
              "software GPU compositor unavailable, using softbuffer"
            );
            return Self::cpu_presenter(window);
          }
          return Err(WindowError::Graphics(format!(
            "GPU compositor failed ({error}); software retry failed ({software_error})"
          )));
        }
      }
    }
    if self.gpu.fallback == GpuFallback::Cpu {
      // `cpu` is handled before the first attempt; keep this defensive branch
      // so a future call site cannot accidentally try a GPU here.
      tracing::info!("GPU compositor disabled by gpu.fallback = \"cpu\"");
      return Self::cpu_presenter(window);
    }
    Err(WindowError::Graphics(error))
  }

  /// Refreshes the cached output name. `current_monitor` is a round trip on
  /// some backends, so it is not called once per frame.
  pub(super) fn update_monitor(&mut self) {
    self.monitor = self
      .window
      .as_ref()
      .and_then(|window| window.current_monitor())
      .and_then(|monitor| monitor.name());
  }

  pub(super) fn request_redraw(&self) {
    if let Some(window) = &self.window {
      window.request_redraw();
    }
  }

  /// Normal runs keep the short historical title. The studio names its source
  /// so two dev windows are distinguishable in a task switcher.
  pub(super) fn window_title(&self) -> String {
    match &self.dev {
      Some(dev) => format!("dawn dev {}", dev.source.label),
      None => "dawn".to_string(),
    }
  }
}
