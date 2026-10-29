//! Render one frame to a PNG without a window.
//!
//! This exists so a greeter theme can be checked on a normal desktop or in CI
//! before it is installed at the login screen. It uses the exact same view and
//! renderer as the window, so a screenshot is honest: if something is clipped
//! here, it is clipped at boot too.
//!
//! The one deliberate difference is timing. A screenshot cannot wait for a
//! crossfade, so it asks the background to load synchronously and then renders
//! at the end of the fade. Motion is rendered in its settled state for the
//! same reason.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::plugin::Plugin;
use crate::session::Session;
use crate::ui::background::BackgroundView;
use crate::ui::canvas::Canvas;
use crate::ui::drag::Drag;
use crate::ui::glass::GlassCache;
use crate::ui::motion::MotionFrame;
use crate::ui::render::{self, Scene};
use crate::ui::text::TextEngine;
use crate::ui::view::View;

/// Long enough for the 320 ms crossfade to finish. The exact value does not
/// matter as long as it is past the fade constant.
const SETTLE: Duration = Duration::from_secs(1);

#[derive(Debug, thiserror::Error)]
pub enum ScreenshotError {
  #[error("cannot create a {width}x{height} canvas")]
  Canvas { width: u32, height: u32 },
  #[error("cannot write {path}: {source}")]
  Write {
    path: PathBuf,
    #[source]
    source: png::EncodingError,
  },
}

/// Renders the login screen at `size` and writes it to `path`.
pub fn render(
  config: &Config,
  sessions: &[Session],
  plugins: &[Plugin],
  size: (u32, u32),
  dry_run: bool,
  warning: Option<String>,
  path: &Path,
) -> Result<(), ScreenshotError> {
  let (width, height) = size;
  let mut canvas = Canvas::new(width, height).ok_or(ScreenshotError::Canvas { width, height })?;
  let mut text = TextEngine::new(&config.ui.font);
  let mut background = BackgroundView::new(config);
  let mut glass = GlassCache::new();
  let drag = Drag::new(
    config.layout.save.then(|| config.layout.resolved_file()),
    Duration::ZERO,
  );

  let now = Instant::now();
  background.load_blocking((width, height), None, now);
  // Selecting the first frame and skipping the fade keeps screenshots stable
  // across runs and machines.
  let frame_now = now + SETTLE;

  // No worker: a screenshot must work without a greetd socket, and it never
  // accepts input. The view is built exactly as the live app would build it.
  let app = crate::app::App::new(
    config.clone(),
    sessions.to_vec(),
    plugins.to_vec(),
    None,
    warning,
    dry_run,
  );
  let view: View = app.view();
  let zoned = jiff::Zoned::now();
  let asset_warning = background.warning().map(str::to_string);
  let scene = Scene {
    view: &view,
    theme: app.theme(),
    plugins: app.plugins(),
    now: &zoned,
    started: frame_now,
    frame_now,
    scale: 1.0,
    motion: MotionFrame::settled(),
    monitor: None,
    background_color: config.background.color,
    asset_warning: asset_warning.as_deref(),
    drag: &drag,
  };
  let _ = render::paint(&mut canvas, &mut text, &mut background, &mut glass, &scene);

  canvas
    .save_png(path)
    .map_err(|source| ScreenshotError::Write {
      path: path.to_path_buf(),
      source,
    })
}
