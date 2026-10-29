//! Streaming playback for animated GIF and WebP wallpapers.
//!
//! A predecoded frame cache is fine for a short, small animation, but a
//! 1920x1080 GIF with a couple hundred frames needs gigabytes. That is exactly
//! the file people mean when they say "high quality GIF". This module keeps
//! only the current frame plus the one being decoded, so memory stays flat and
//! playback keeps the source resolution.
//!
//! The decoder runs on its own thread. It publishes a frame and then sleeps
//! for that frame's delay, re-opening the file when it reaches the end. The UI
//! never waits for a decode: it paints whatever frame is current and asks for
//! another repaint shortly after. If the decoder falls behind, the last frame
//! simply stays on screen for a moment, which is much better than blocking the
//! login form.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use tiny_skia::Pixmap;
use tracing::warn;

use crate::color::Rgb;
use crate::layout::Fit;
use crate::ui::image::{self, AnimationFormat, DecodeLimits};

/// The gap between checks for stop and pause while a frame is displayed.
pub(crate) const TICK: Duration = Duration::from_millis(10);
/// How long the first frame may take before the loader gives up and falls back.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(5);

/// A callback invoked when a decoder publishes a frame. The window wraps an
/// `EventLoopProxy`, but the stream itself only sees a plain closure.
pub type FrameWaker = Arc<dyn Fn() + Send + Sync>;

/// What the background needs from any live wallpaper. GIF, WebP, and video
/// all provide the same four operations, so the renderer does not care which
/// decoder produced the frame.
pub trait LiveSource: Send + Sync {
  fn current(&self) -> Option<Arc<Pixmap>>;
  fn sequence(&self) -> u64;
  fn is_running(&self) -> bool;
  fn set_paused(&self, paused: bool);
  /// Installs a callback invoked after a new frame is published. The window
  /// turns this into an event-loop wakeup, so a 60 fps video is presented on
  /// the next frame instead of waiting for a polling tick.
  fn set_waker(&self, _waker: Option<FrameWaker>) {}
}

impl fmt::Debug for dyn LiveSource + '_ {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "LiveSource(sequence={})", self.sequence())
  }
}

/// A live animation source. Dropping it stops the decoder thread.
pub struct FrameStream {
  current: Arc<Mutex<Option<Arc<Pixmap>>>>,
  sequence: Arc<AtomicU64>,
  stop: Arc<AtomicBool>,
  paused: Arc<AtomicBool>,
  alive: Arc<AtomicBool>,
  waker: Arc<Mutex<Option<FrameWaker>>>,
}

impl FrameStream {
  /// Starts decoding and waits for the first frame. A corrupt file or a
  /// decoder that never produces a frame becomes an error here, so the
  /// background can fall back to its gradient instead of showing a blank
  /// window forever.
  #[allow(clippy::too_many_arguments)]
  pub(crate) fn load(
    path: &Path,
    format: AnimationFormat,
    target: (u32, u32),
    fit: Fit,
    background: Rgb,
    fps: u32,
    memory_limit: u64,
    limits: DecodeLimits,
  ) -> Result<Self, String> {
    let current = Arc::new(Mutex::new(None));
    let sequence = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let alive = Arc::new(AtomicBool::new(true));
    let waker = Arc::new(Mutex::new(None));
    let (ready_tx, ready_rx) = mpsc::channel();
    let display = path.to_path_buf();
    let worker_path = path.to_path_buf();

    let worker = {
      let current = Arc::clone(&current);
      let sequence = Arc::clone(&sequence);
      let stop = Arc::clone(&stop);
      let paused = Arc::clone(&paused);
      let alive = Arc::clone(&alive);
      let waker = Arc::clone(&waker);
      thread::Builder::new()
        .name("dawn-animation".to_string())
        .spawn(move || {
          run(
            worker_path,
            format,
            target,
            fit,
            background,
            fps,
            memory_limit,
            limits,
            current,
            sequence,
            stop,
            paused,
            waker,
            ready_tx,
          );
          alive.store(false, Ordering::Release);
        })
    };
    if let Err(error) = worker {
      return Err(format!("cannot start the animation thread: {error}"));
    }

    match ready_rx.recv_timeout(FIRST_FRAME_TIMEOUT) {
      Ok(Ok(())) => Ok(Self {
        current,
        sequence,
        stop,
        paused,
        alive,
        waker,
      }),
      Ok(Err(error)) => {
        stop.store(true, Ordering::Relaxed);
        Err(error)
      }
      Err(_) => {
        stop.store(true, Ordering::Relaxed);
        Err(format!(
          "timed out waiting for the first frame of {}",
          display.display()
        ))
      }
    }
  }

  /// The frame to paint, or `None` before the first frame is ready.
  #[must_use]
  pub fn current(&self) -> Option<Arc<Pixmap>> {
    match self.current.lock() {
      Ok(slot) => slot.clone(),
      Err(poisoned) => poisoned.into_inner().clone(),
    }
  }

  /// Increments once per published frame. The window uses this to decide when
  /// a redraw is actually needed.
  #[must_use]
  pub fn sequence(&self) -> u64 {
    self.sequence.load(Ordering::Acquire)
  }

  /// False once the decoder has stopped, so the window stops polling for
  /// frames that will never arrive. The last frame stays on screen.
  #[must_use]
  pub fn is_running(&self) -> bool {
    self.alive.load(Ordering::Acquire)
  }

  /// Stops or resumes the decoder. While paused the current frame stays on
  /// screen and the worker sleeps, so an unfocused greeter costs nothing.
  pub fn set_paused(&self, paused: bool) {
    self.paused.store(paused, Ordering::Relaxed);
  }

  pub fn set_waker(&self, waker: Option<FrameWaker>) {
    if let Ok(mut slot) = self.waker.lock() {
      *slot = waker;
    }
  }
}

impl LiveSource for FrameStream {
  fn current(&self) -> Option<Arc<Pixmap>> {
    Self::current(self)
  }

  fn sequence(&self) -> u64 {
    Self::sequence(self)
  }

  fn is_running(&self) -> bool {
    Self::is_running(self)
  }

  fn set_paused(&self, paused: bool) {
    Self::set_paused(self, paused);
  }

  fn set_waker(&self, waker: Option<FrameWaker>) {
    Self::set_waker(self, waker);
  }
}

impl Drop for FrameStream {
  fn drop(&mut self) {
    // The worker checks this between frames and inside every sleep tick. We do
    // not join: dropping a background must never stall a resize or shutdown.
    self.stop.store(true, Ordering::Relaxed);
  }
}

impl fmt::Debug for FrameStream {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    // Pixmap is not Debug, so show the useful part only.
    write!(f, "FrameStream(sequence={})", self.sequence())
  }
}

#[allow(clippy::too_many_arguments)]
fn run(
  path: PathBuf,
  format: AnimationFormat,
  target: (u32, u32),
  fit: Fit,
  background: Rgb,
  fps: u32,
  memory_limit: u64,
  limits: DecodeLimits,
  current: Arc<Mutex<Option<Arc<Pixmap>>>>,
  sequence: Arc<AtomicU64>,
  stop: Arc<AtomicBool>,
  paused: Arc<AtomicBool>,
  waker: Arc<Mutex<Option<FrameWaker>>>,
  ready_tx: mpsc::Sender<Result<(), String>>,
) {
  let mut ready = false;
  loop {
    if stop.load(Ordering::Relaxed) {
      return;
    }
    let result = image::decode_animation(
      &path,
      format,
      target,
      fit,
      background,
      limits,
      fps,
      memory_limit,
      |pixmap, delay| {
        if stop.load(Ordering::Relaxed) {
          return false;
        }
        let mut slot = match current.lock() {
          Ok(slot) => slot,
          Err(poisoned) => poisoned.into_inner(),
        };
        *slot = Some(Arc::new(pixmap));
        drop(slot);
        sequence.fetch_add(1, Ordering::Release);
        if let Ok(slot) = waker.lock()
          && let Some(wake) = slot.as_ref()
        {
          wake();
        }
        if !ready {
          ready = true;
          let _ = ready_tx.send(Ok(()));
        }
        wait(delay, &stop, &paused)
      },
    );
    match result {
      Ok(_) => {}
      Err(error) => {
        if !ready {
          let _ = ready_tx.send(Err(error.clone()));
        }
        warn!(%error, "animation stopped");
        return;
      }
    }
  }
}

/// Sleeps for one frame delay while staying responsive to pause and stop.
/// Paused time does not count against the delay, so resuming shows the next
/// frame at the same point in the loop rather than jumping ahead.
pub(crate) fn wait(delay: Duration, stop: &AtomicBool, paused: &AtomicBool) -> bool {
  let mut remaining = delay;
  while !remaining.is_zero() {
    if stop.load(Ordering::Relaxed) {
      return false;
    }
    if paused.load(Ordering::Relaxed) {
      thread::sleep(TICK);
      continue;
    }
    let step = remaining.min(TICK);
    thread::sleep(step);
    remaining = remaining.saturating_sub(step);
  }
  true
}
