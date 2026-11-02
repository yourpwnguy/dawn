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
