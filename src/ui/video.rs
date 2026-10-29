//! MP4 (and other FFmpeg-decodable) live wallpapers.
//!
//! Writing an H.264 decoder is not a good use of this project's time, and
//! linking a second media stack into a greeter is a dependency risk. So dawn
//! asks the system `ffmpeg` for raw RGBA frames on a pipe and treats them like
//! the GIF stream: one frame resident, decoded on a background thread, never
//! blocking the login form.
//!
//! The trade-off is explicit: video wallpapers require `ffmpeg` at runtime. If
//! it is missing, the loader returns a readable warning and the greeter falls
//! back to its gradient. GIF and WebP wallpapers do not need it.
//!
//! Pause works by simply not reading the pipe. FFmpeg blocks on a full pipe, so
//! an unfocused greeter costs no decode work. Stop kills the child; the read
//! unblocks with EOF and the thread exits.

use std::fmt;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use tiny_skia::{IntSize, Pixmap};
use tracing::warn;

use crate::color::Rgb;
use crate::layout::Fit;
use crate::ui::image::{DecodeLimits, clamp_target};
use crate::ui::stream::{FrameWaker, LiveSource, TICK};

const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_VIDEO_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_STDERR: usize = 4096;

/// Extensions we hand to ffmpeg. The list is intentionally about containers,
/// not codecs: ffmpeg decides which decoder to use.
#[must_use]
pub fn is_video_path(path: &Path) -> bool {
  path
    .extension()
    .and_then(|extension| extension.to_str())
    .is_some_and(|extension| {
      matches!(
        extension.to_ascii_lowercase().as_str(),
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" | "mpg" | "mpeg"
      )
    })
}

/// A live video wallpaper backed by an ffmpeg child process.
pub struct VideoStream {
  current: Arc<Mutex<Option<Arc<Pixmap>>>>,
  sequence: Arc<AtomicU64>,
  stop: Arc<AtomicBool>,
  paused: Arc<AtomicBool>,
  alive: Arc<AtomicBool>,
  child: Arc<Mutex<Option<Child>>>,
  waker: Arc<Mutex<Option<FrameWaker>>>,
}

impl VideoStream {
  /// Starts ffmpeg and waits for the first frame. A missing ffmpeg, an
  /// unreadable file, or a codec ffmpeg cannot decode becomes an error here so
  /// the background can keep its placeholder.
  pub fn load(
    path: &Path,
    target: (u32, u32),
    fit: Fit,
    background: Rgb,
    fps: u32,
    memory_limit: u64,
    _limits: DecodeLimits,
  ) -> Result<Self, String> {
    validate_file(path)?;
    let (width, height) = clamp_target(target, None, memory_limit).0;
    let frame_bytes = frame_bytes(width, height)?;

    let mut child = Command::new("ffmpeg")
      .args(ffmpeg_args(path, width, height, fit, background, fps))
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .spawn()
      .map_err(|error| {
        format!(
          "cannot start ffmpeg for {}: {error} (install ffmpeg to use video wallpapers)",
          path.display()
        )
      })?;
    let mut stdout = match child.stdout.take() {
      Some(stdout) => stdout,
      None => {
        let _ = child.kill();
        return Err("ffmpeg did not expose a stdout pipe".to_string());
      }
    };
    let stderr_text = Arc::new(Mutex::new(String::new()));
    if let Some(stderr) = child.stderr.take() {
      let sink = Arc::clone(&stderr_text);
      let _ = thread::Builder::new()
        .name("dawn-video-stderr".to_string())
        .spawn(move || {
          let mut data = Vec::with_capacity(MAX_STDERR);
          let _ = stderr.take(MAX_STDERR as u64).read_to_end(&mut data);
          if let Ok(mut sink) = sink.lock() {
            *sink = String::from_utf8_lossy(&data).trim().to_string();
          }
        });
    }

    let current = Arc::new(Mutex::new(None));
    let sequence = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let alive = Arc::new(AtomicBool::new(true));
    let waker = Arc::new(Mutex::new(None));
    let child = Arc::new(Mutex::new(Some(child)));
    let (ready_tx, ready_rx) = mpsc::channel();

    let worker = {
      let current = Arc::clone(&current);
      let sequence = Arc::clone(&sequence);
      let stop = Arc::clone(&stop);
      let paused = Arc::clone(&paused);
      let alive = Arc::clone(&alive);
      let waker = Arc::clone(&waker);
      thread::Builder::new()
        .name("dawn-video".to_string())
        .spawn(move || {
          read_frames(
            &mut stdout,
            width,
            height,
            frame_bytes,
            &stderr_text,
            &current,
            &sequence,
            &stop,
            &paused,
            &waker,
            &ready_tx,
          );
          alive.store(false, Ordering::Release);
        })
    };
    if let Err(error) = worker {
      kill_child(&child);
      return Err(format!("cannot start the video thread: {error}"));
    }

    match ready_rx.recv_timeout(FIRST_FRAME_TIMEOUT) {
      Ok(Ok(())) => Ok(Self {
        current,
        sequence,
        stop,
        paused,
        alive,
        child,
        waker,
      }),
      Ok(Err(error)) => {
        stop.store(true, Ordering::Relaxed);
        kill_child(&child);
        Err(error)
      }
      Err(_) => {
        stop.store(true, Ordering::Relaxed);
        kill_child(&child);
        Err(format!(
          "timed out waiting for the first video frame of {}",
          path.display()
        ))
      }
    }
  }

  /// Decodes one frame and stops. Used by `background.kind = "image"` so a
  /// video file still produces a sensible still wallpaper instead of a
  /// gradient and a cryptic warning.
  pub fn first_frame(
    path: &Path,
    target: (u32, u32),
    fit: Fit,
    background: Rgb,
    fps: u32,
    memory_limit: u64,
  ) -> Result<Pixmap, String> {
    validate_file(path)?;
    let (width, height) = clamp_target(target, None, memory_limit).0;
    let frame_bytes = frame_bytes(width, height)?;
    let mut args = base_args(path, width, height, fit, background, fps);
    // `-frames:v 1` stops ffmpeg after one decoded frame. Put it before the
    // output format so it applies to the rawvideo muxer.
    let output = Command::new("ffmpeg")
      .args(&mut args)
      .arg("-frames:v")
      .arg("1")
      .arg("-f")
      .arg("rawvideo")
      .arg("pipe:1")
      .stdin(Stdio::null())
      .stderr(Stdio::piped())
      .output()
      .map_err(|error| {
        format!(
          "cannot start ffmpeg for {}: {error} (install ffmpeg to use video wallpapers)",
          path.display()
        )
      })?;
    if !output.status.success() {
      let stderr = String::from_utf8_lossy(&output.stderr);
      let detail = stderr.trim();
      return Err(if detail.is_empty() {
        format!("ffmpeg could not decode {}", path.display())
      } else {
        format!("ffmpeg could not decode {}: {detail}", path.display())
      });
    }
    if output.stdout.len() < frame_bytes {
      return Err(format!(
        "ffmpeg produced {} bytes for a {width}x{height} frame (wanted {frame_bytes})",
        output.stdout.len()
      ));
    }
    pixmap_from(output.stdout[..frame_bytes].to_vec(), width, height)
  }
}

impl LiveSource for VideoStream {
  fn current(&self) -> Option<Arc<Pixmap>> {
    lock(&self.current).clone()
  }

  fn sequence(&self) -> u64 {
    self.sequence.load(Ordering::Acquire)
  }

  fn is_running(&self) -> bool {
    self.alive.load(Ordering::Acquire)
  }

  fn set_paused(&self, paused: bool) {
    self.paused.store(paused, Ordering::Relaxed);
  }

  fn set_waker(&self, waker: Option<FrameWaker>) {
    if let Ok(mut slot) = self.waker.lock() {
      *slot = waker;
    }
  }
}

impl Drop for VideoStream {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Relaxed);
    // Killing the child is what unblocks a thread sitting in read_exact.
    kill_child(&self.child);
  }
}

impl fmt::Debug for VideoStream {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "VideoStream(sequence={})", self.sequence())
  }
}

#[allow(clippy::too_many_arguments)]
fn read_frames(
  stdout: &mut impl Read,
  width: u32,
  height: u32,
  frame_bytes: usize,
  stderr_text: &Arc<Mutex<String>>,
  current: &Arc<Mutex<Option<Arc<Pixmap>>>>,
  sequence: &Arc<AtomicU64>,
  stop: &AtomicBool,
  paused: &AtomicBool,
  waker: &Arc<Mutex<Option<FrameWaker>>>,
  ready_tx: &mpsc::Sender<Result<(), String>>,
) {
  let mut ready = false;
  loop {
    if stop.load(Ordering::Relaxed) {
      return;
    }
    while paused.load(Ordering::Relaxed) {
      if stop.load(Ordering::Relaxed) {
        return;
      }
      thread::sleep(TICK);
    }
    let mut buffer = vec![0u8; frame_bytes];
    if let Err(error) = stdout.read_exact(&mut buffer) {
      if stop.load(Ordering::Relaxed) {
        return;
      }
      let stderr = lock(stderr_text).clone();
      let message = if stderr.is_empty() {
        format!("ffmpeg stopped producing video frames: {error}")
      } else {
        format!("ffmpeg stopped producing video frames: {stderr}")
      };
      if !ready {
        let _ = ready_tx.send(Err(message.clone()));
      }
      warn!(%message, "video wallpaper stopped");
      return;
    }
    let pixmap = match pixmap_from(buffer, width, height) {
      Ok(pixmap) => pixmap,
      Err(error) => {
        if !ready {
          let _ = ready_tx.send(Err(error.clone()));
        }
        warn!(%error, "video wallpaper frame is invalid");
        return;
      }
    };
    *lock(current) = Some(Arc::new(pixmap));
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
    // ffmpeg is driven with `-re`, so it paces frames from the source
    // timestamps. Sleeping here as well would add decode time to every frame
    // and make a 60 fps clip drift below 60 fps. The blocking read is the only
    // pacing we need, and it also gives pause free back-pressure.
  }
}

fn validate_file(path: &Path) -> Result<(), String> {
  if !is_video_path(path) {
    return Err(format!(
      "{} is not a supported video file (use mp4, m4v, mov, webm, mkv, avi, mpg, or mpeg)",
      path.display()
    ));
  }
  let metadata =
    std::fs::metadata(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
  if !metadata.is_file() {
    return Err(format!("{} is not a regular file", path.display()));
  }
  if metadata.len() > MAX_VIDEO_BYTES {
    return Err(format!(
      "{} is larger than the {} GiB video limit",
      path.display(),
      MAX_VIDEO_BYTES / 1024 / 1024 / 1024
    ));
  }
  Ok(())
}

fn frame_bytes(width: u32, height: u32) -> Result<usize, String> {
  let pixels = u64::from(width)
    .checked_mul(u64::from(height))
    .ok_or_else(|| "video frame dimensions overflow".to_string())?;
  let bytes = pixels
    .checked_mul(4)
    .ok_or_else(|| "video frame size overflow".to_string())?;
  usize::try_from(bytes).map_err(|_| "video frame is too large for this machine".to_string())
}

fn pixmap_from(data: Vec<u8>, width: u32, height: u32) -> Result<Pixmap, String> {
  let size = IntSize::from_wh(width, height)
    .ok_or_else(|| format!("invalid video frame size {width}x{height}"))?;
  Pixmap::from_vec(data, size)
    .ok_or_else(|| format!("video frame buffer does not match {width}x{height} RGBA"))
}

fn base_args(
  path: &Path,
  width: u32,
  height: u32,
  fit: Fit,
  background: Rgb,
  fps: u32,
) -> Vec<std::ffi::OsString> {
  let filter = video_filter(width, height, fit, background, fps);
  vec![
    "-nostdin".into(),
    "-hide_banner".into(),
    "-loglevel".into(),
    "error".into(),
    "-i".into(),
    path.as_os_str().to_os_string(),
    "-an".into(),
    "-sn".into(),
    "-dn".into(),
    "-vf".into(),
    filter.into(),
    "-pix_fmt".into(),
    "rgba".into(),
  ]
}

fn ffmpeg_args(
  path: &Path,
  width: u32,
  height: u32,
  fit: Fit,
  background: Rgb,
  fps: u32,
) -> Vec<std::ffi::OsString> {
  let mut args: Vec<std::ffi::OsString> = vec![
    "-nostdin".into(),
    "-hide_banner".into(),
    "-loglevel".into(),
    "error".into(),
    // Input rate control: ffmpeg sleeps between frames according to the
    // source timestamps, including variable frame rate clips. That gives us
    // the source's exact timing and keeps the pipe one frame deep.
    "-re".into(),
    "-stream_loop".into(),
    "-1".into(),
  ];
  args.extend(base_args(path, width, height, fit, background, fps));
  // Do not let ffmpeg resample the output behind our back. With a positive
  // `background.fps` the filter chain already caps the rate; with the default
  // of 0 we want every source frame, in source order.
  args.push("-fps_mode".into());
  args.push("passthrough".into());
  args.push("-f".into());
  args.push("rawvideo".into());
  args.push("pipe:1".into());
  args
}

/// The ffmpeg filter chain. Kept pure so the fit modes can be tested without
/// spawning a process.
#[must_use]
pub fn video_filter(width: u32, height: u32, fit: Fit, background: Rgb, fps: u32) -> String {
  let scale = match fit {
    Fit::Cover => {
      format!("scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height}")
    }
    Fit::Contain => format!(
      "scale={width}:{height}:force_original_aspect_ratio=decrease,\
       pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=0x{:02x}{:02x}{:02x}",
      background.r, background.g, background.b
    ),
    Fit::Fill => format!("scale={width}:{height}"),
  };
  if fps == 0 {
    format!("{scale},format=rgba")
  } else {
    format!("{scale},fps={},format=rgba", fps.max(1))
  }
}

fn kill_child(slot: &Arc<Mutex<Option<Child>>>) {
  let mut slot = lock(slot);
  if let Some(mut child) = slot.take() {
    let _ = child.kill();
    let _ = child.wait();
  }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  match mutex.lock() {
    Ok(guard) => guard,
    Err(poisoned) => poisoned.into_inner(),
  }
}

#[cfg(test)]
mod tests {
  use super::{VideoStream, ffmpeg_args, is_video_path, video_filter};
  use crate::color::Rgb;
  use crate::layout::Fit;
  use crate::ui::image::DecodeLimits;
  use crate::ui::stream::LiveSource;
  use std::path::{Path, PathBuf};
  use std::process::{Command, Stdio};
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};
  use std::thread;
  use std::time::{Duration, Instant};

  /// The sample clip checked into `examples/wallpapers`. Tests skip instead of
  /// failing on a machine without ffmpeg, because video is an optional feature
  /// and a build host should not need a media stack just to run `cargo test`.
  fn sample_video() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/wallpapers/input.mp4")
  }

  fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
      .arg("-version")
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .status()
      .is_ok_and(|status| status.success())
  }

  #[test]
  fn rejects_a_file_that_is_not_video() {
    let error = VideoStream::load(
      Path::new("wallpaper.gif"),
      (64, 36),
      Fit::Cover,
      Rgb::solid(0, 0, 0),
      30,
      64 * 1024 * 1024,
      DecodeLimits::default(),
    )
    .expect_err("a GIF is not a video");
    assert!(error.contains("not a supported video file"), "{error}");
  }

  #[test]
  fn decodes_a_real_video_through_ffmpeg() {
    if !ffmpeg_available() {
      eprintln!("skipping video decoding test: ffmpeg is not installed");
      return;
    }
    let stream = VideoStream::load(
      &sample_video(),
      (96, 54),
      Fit::Fill,
      Rgb::solid(0, 0, 0),
      10,
      64 * 1024 * 1024,
      DecodeLimits::default(),
    )
    .expect("load the sample clip");
    let first = stream.current().expect("the first frame is ready on load");
    assert_eq!((first.width(), first.height()), (96, 54));
    assert!(stream.is_running(), "the decoder thread should be alive");

    // Frames are paced on the worker thread, so poll with a deadline instead
    // of sleeping for a fixed time. A slower machine gets the full budget and
    // a fast one finishes as soon as the second frame lands.
    let deadline = Instant::now() + Duration::from_secs(3);
    while stream.sequence() < 2 && Instant::now() < deadline {
      thread::sleep(Duration::from_millis(20));
    }
    assert!(
      stream.sequence() >= 2,
      "expected at least two frames, saw {}",
      stream.sequence()
    );
  }

  #[test]
  fn native_playback_follows_source_timing_and_wakes_the_window() {
    if !ffmpeg_available() {
      eprintln!("skipping native timing test: ffmpeg is not installed");
      return;
    }
    let stream = VideoStream::load(
      &sample_video(),
      (96, 54),
      Fit::Fill,
      Rgb::solid(0, 0, 0),
      0,
      64 * 1024 * 1024,
      DecodeLimits::default(),
    )
    .expect("load the sample clip at its native rate");
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    stream.set_waker(Some(Arc::new(move || {
      counter.fetch_add(1, Ordering::Relaxed);
    })));
    let before = stream.sequence();
    thread::sleep(Duration::from_millis(300));
    let frames = stream.sequence().saturating_sub(before);
    // The sample is 25 fps, so 300 ms is about seven frames. Without `-re`
    // ffmpeg would dump the whole clip into the pipe and this would be in the
    // hundreds; with the old double pacing it would be closer to ten or fewer.
    assert!(
      (3..=20).contains(&frames),
      "native playback produced {frames} frames in 300 ms"
    );
    assert!(
      wakes.load(Ordering::Relaxed) >= 2,
      "the frame waker did not fire"
    );
  }

  #[test]
  fn ffmpeg_args_pace_from_the_source_and_do_not_resample() {
    let args = ffmpeg_args(
      Path::new("wall.mp4"),
      64,
      36,
      Fit::Fill,
      Rgb::solid(0, 0, 0),
      0,
    );
    let text: Vec<String> = args
      .iter()
      .map(|value| value.to_string_lossy().into_owned())
      .collect();
    assert!(text.iter().any(|value| value == "-re"), "{text:?}");
    assert!(text.iter().any(|value| value == "-fps_mode"), "{text:?}");
    assert!(text.iter().any(|value| value == "passthrough"), "{text:?}");
    assert!(
      !text.iter().any(|value| value.contains("fps=")),
      "native playback must not force an output rate: {text:?}"
    );
  }

  #[test]
  fn extracts_a_first_frame_for_still_mode() {
    if !ffmpeg_available() {
      eprintln!("skipping first-frame test: ffmpeg is not installed");
      return;
    }
    let frame = VideoStream::first_frame(
      &sample_video(),
      (64, 36),
      Fit::Fill,
      Rgb::solid(0, 0, 0),
      10,
      64 * 1024 * 1024,
    )
    .expect("decode one frame");
    assert_eq!((frame.width(), frame.height()), (64, 36));
  }

  #[test]
  fn detects_video_extensions_case_insensitively() {
    assert!(is_video_path(Path::new("wall.mp4")));
    assert!(is_video_path(Path::new("wall.MP4")));
    assert!(is_video_path(Path::new("wall.webm")));
    assert!(!is_video_path(Path::new("wall.gif")));
    assert!(!is_video_path(Path::new("wall")));
  }

  #[test]
  fn fit_modes_produce_the_expected_filters() {
    let black = Rgb::solid(1, 2, 3);
    assert!(video_filter(1920, 1080, Fit::Cover, black, 30).contains("crop=1920:1080"));
    assert!(video_filter(1920, 1080, Fit::Contain, black, 30).contains("pad=1920:1080"));
    assert!(video_filter(1920, 1080, Fit::Contain, black, 30).contains("color=0x010203"));
    assert_eq!(
      video_filter(640, 360, Fit::Fill, black, 24),
      "scale=640:360,fps=24,format=rgba"
    );
    assert_eq!(
      video_filter(640, 360, Fit::Fill, black, 0),
      "scale=640:360,format=rgba"
    );
  }
}
