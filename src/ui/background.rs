//! The background engine: solid, gradient, still image, or animated wallpaper.
//!
//! Decoding is the expensive part and it never runs on the UI thread. The
//! window shows the configured gradient immediately, starts a loader thread,
//! and crossfades to the wallpaper when the frame set arrives. Frames are
//! decoded once, fitted to the output size, and cached as ready-to-blit
//! pixmaps. Animated playback is driven by the file's delays and capped by the
//! configured frame rate.
//!
//! A broken wallpaper is never fatal. The loader returns a readable error, the
//! view keeps its gradient, and the error is painted near the bottom of the
//! screen alongside config warnings.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tiny_skia::Pixmap;
use tracing::warn;

use crate::color::Rgb;
use crate::config::{BackgroundConfig, BackgroundKind, Config};
use crate::ui::canvas::Canvas;
use crate::ui::image::{self, DecodeLimits};
use crate::ui::stream::{FrameStream, LiveSource};
use crate::ui::video::{self, VideoStream};

const FADE: Duration = Duration::from_millis(320);
const LOAD_LIMITS: DecodeLimits = DecodeLimits {
  max_file_bytes: 64 * 1024 * 1024,
  max_dimension: 8192,
  max_pixels: 32 * 1024 * 1024,
  max_alloc: 256 * 1024 * 1024,
};

/// What the loader produced. `Frames` is used for both still and animated
/// images; a still image simply has one entry.
#[derive(Debug)]
pub enum Content {
  Solid(Rgb),
  Gradient(Rgb),
  Frames {
    /// Frames are shared so a renderer snapshot can hold one without copying
    /// a full-screen pixmap. The GPU path relies on that: it snapshots the
    /// background every frame but only uploads when the `Arc` actually
    /// changes.
    frames: Vec<Arc<Pixmap>>,
    delays: Vec<Duration>,
    cumulative: Vec<Duration>,
  },
  /// A live GIF, WebP, or video decoder. Only the current frame is resident,
  /// so long or full-HD animations do not need a predecoded frame cache.
  Stream(Arc<dyn LiveSource>),
}

/// What the current background is made of. The GPU shader uses this instead of
/// guessing whether a texture is bound, and the blur snapshot uses it to paint
/// the same placeholder the CPU path would.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotKind {
  Image,
  Solid(Rgb),
  Gradient(Rgb),
}

/// A borrowing-free view of one background frame. `current` and `previous`
/// are shared frame handles, so taking a snapshot at 30 or 60 fps does not
/// copy a single pixel. `opacity` is the crossfade weight of `current` over
/// `previous`, exactly like `BackgroundView::paint`.
#[derive(Debug, Clone)]
pub struct BackgroundSnapshot {
  pub current: Option<Arc<Pixmap>>,
  pub previous: Option<Arc<Pixmap>>,
  pub opacity: f32,
  pub kind: SnapshotKind,
  pub frame_id: u64,
  pub animated: bool,
}

#[derive(Debug)]
pub struct LoadedBackground {
  pub content: Content,
  pub warning: Option<String>,
}

pub struct BackgroundView {
  config: BackgroundConfig,
  placeholder: Rgb,
  gradient: bool,
  loaded: Option<LoadedBackground>,
  previous: Option<Arc<Pixmap>>,
  fade_start: Option<Instant>,
  frame_start: Instant,
  generation: u64,
  requested: Option<((u32, u32), Option<String>)>,
  accepted: Option<((u32, u32), Option<String>)>,
  loading: bool,
  paused: bool,
  paused_since: Option<Instant>,
  low_power: bool,
  warning: Option<String>,
}

mod helpers;
mod lifecycle;
mod loader;
mod paint;

use helpers::*;
use loader::*;

#[cfg(test)]
mod tests;
