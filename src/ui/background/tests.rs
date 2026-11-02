#[cfg(test)]
fn cumulative(delays: &[Duration]) -> Vec<Duration> {
  let mut total = Duration::ZERO;
  delays
    .iter()
    .map(|delay| {
      total += *delay;
      total
    })
    .collect()
}

use super::{BackgroundView, Content, LoadedBackground};
use crate::config::{BackgroundConfig, BackgroundKind, Config};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn config(kind: BackgroundKind) -> BackgroundConfig {
  let mut config = BackgroundConfig {
    kind,
    ..Config::default().background
  };
  config.color = crate::color::Rgb::solid(10, 20, 30);
  config
}

#[test]
fn solid_and_gradient_do_not_need_files() {
  let solid = LoadedBackground::load(&config(BackgroundKind::Solid), (320, 200), None).unwrap();
  assert!(matches!(solid.content, Content::Solid(_)));
  let gradient =
    LoadedBackground::load(&config(BackgroundKind::Gradient), (320, 200), None).unwrap();
  assert!(matches!(gradient.content, Content::Gradient(_)));
}

#[test]
fn missing_animation_still_yields_a_placeholder_view() {
  let mut config = Config::default();
  config.background = BackgroundConfig {
    kind: BackgroundKind::Image,
    ..config.background
  };
  let mut view = BackgroundView::new(&config);
  let now = Instant::now();
  assert!(!view.load_blocking((320, 200), None, now));
  assert!(view.warning().is_some());
}

#[test]
fn cumulative_delays_are_monotonic() {
  let values = cumulative(&[Duration::from_millis(10), Duration::from_millis(20)]);
  assert_eq!(
    values,
    [Duration::from_millis(10), Duration::from_millis(30)]
  );
}

#[test]
fn an_unloaded_wallpaper_is_not_the_loaded_identity() {
  // The frame id gates the glass blur cache. Before the loader returns, the
  // window blurs the navy placeholder; the id stays there forever (a still
  // background keeps frame index 0 and the generation was already bumped by
  // the request), so the cache never re-blurred and the plates stayed navy
  // on screen. Unloaded must return the sentinel; loaded must move off it.
  let mut view = BackgroundView::new(&Config {
    background: config(BackgroundKind::Gradient),
    ..Config::default()
  });
  assert_eq!(
    view.frame_id(Instant::now()),
    u64::MAX,
    "unloaded id must be the sentinel"
  );
  let request = view.request((320, 200), None).unwrap();
  let (generation, result) = request.load();
  assert!(view.accept(generation, result, Instant::now()));
  assert_ne!(
    view.frame_id(Instant::now()),
    u64::MAX,
    "loaded background kept the placeholder id, so the blur cache would never refresh"
  );
}

#[test]
fn a_stale_generation_is_ignored() {
  let base = Config {
    background: config(BackgroundKind::Gradient),
    ..Config::default()
  };
  let mut view = BackgroundView::new(&base);
  let request = view.request((320, 200), None).unwrap();
  let (generation, result) = request.load();
  assert!(view.accept(generation, result, Instant::now()));
  assert!(!view.accept(
    generation + 10,
    Ok(LoadedBackground::load(&config(BackgroundKind::Gradient), (320, 200), None).unwrap()),
    Instant::now()
  ));
}

#[test]
fn pausing_freezes_the_frame_and_resume_continues_from_it() {
  // Two synthetic frames with 50 ms delays, injected directly because the
  // loader path is covered elsewhere. While paused, the frame id must not
  // move no matter how much wall-clock passes; on resume it continues from
  // the frozen point instead of jumping to the live position.
  let mut view = BackgroundView::new(&Config {
    background: config(BackgroundKind::Animated),
    ..Config::default()
  });
  let frame = tiny_skia::Pixmap::new(4, 4).unwrap();
  view.loaded = Some(LoadedBackground {
    content: Content::Frames {
      cumulative: cumulative(&[Duration::from_millis(50), Duration::from_millis(100)]),
      delays: vec![Duration::from_millis(50), Duration::from_millis(50)],
      frames: vec![Arc::new(frame.clone()), Arc::new(frame)],
    },
    warning: None,
  });

  view.set_paused(true);
  let frozen = view.frame_id(Instant::now());
  std::thread::sleep(Duration::from_millis(120));
  assert_eq!(
    view.frame_id(Instant::now()),
    frozen,
    "paused frame id moved"
  );

  view.set_paused(false);
  assert_eq!(
    view.frame_id(Instant::now()),
    frozen,
    "resume jumped the animation"
  );
  std::thread::sleep(Duration::from_millis(120));
  assert_ne!(
    view.frame_id(Instant::now()),
    frozen,
    "resume did not advance"
  );
}

#[test]
fn paint_snapshot_matches_paint_for_every_background_kind() {
  // The GPU path paints the wallpaper from `snapshot`/`paint_snapshot` while
  // the CPU and screenshot paths use `paint`. They must agree pixel for
  // pixel, or the live window would drift from `--screenshot`.
  use crate::ui::canvas::Canvas;
  use tiny_skia::Color;

  let fill = |red: u8, green: u8, blue: u8| {
    let mut pixmap = tiny_skia::Pixmap::new(4, 4).unwrap();
    pixmap.fill(Color::from_rgba8(red, green, blue, 255));
    Arc::new(pixmap)
  };

  let compare = |view: &mut BackgroundView, now: Instant| {
    let mut painted = Canvas::new(4, 4).unwrap();
    let mut snapshot = Canvas::new(4, 4).unwrap();
    view.paint(&mut painted, now);
    view.paint_snapshot(&mut snapshot, now);
    assert_eq!(
      painted.pixmap().data(),
      snapshot.pixmap().data(),
      "snapshot painting diverged from paint"
    );
  };

  // Placeholder, solid, and gradient need no frame set at all.
  for kind in [
    BackgroundKind::Solid,
    BackgroundKind::Gradient,
    BackgroundKind::Auto,
  ] {
    let mut view = BackgroundView::new(&Config {
      background: config(kind),
      ..Config::default()
    });
    compare(&mut view, Instant::now());
  }

  let mut view = BackgroundView::new(&Config {
    background: config(BackgroundKind::Animated),
    ..Config::default()
  });
  view.loaded = Some(LoadedBackground {
    content: Content::Frames {
      cumulative: cumulative(&[Duration::from_millis(50), Duration::from_millis(100)]),
      delays: vec![Duration::from_millis(50), Duration::from_millis(50)],
      frames: vec![fill(220, 40, 40), fill(40, 180, 80)],
    },
    warning: None,
  });
  view.frame_start = Instant::now();
  let start = view.frame_start;
  compare(&mut view, start);

  // A half-finished crossfade must also match: this is the path that mixes
  // the previous wallpaper with the new one.
  view.previous = Some(fill(20, 20, 200));
  view.fade_start = Some(start - Duration::from_millis(160));
  compare(&mut view, start);
}

#[test]
fn low_power_freezes_on_one_frame() {
  // Low-power mode is documented as freezing the wallpaper on a still
  // frame. The clock keeps repainting, so the frame id must stay at the
  // load instant even while wall-clock time moves well past a frame delay.
  let mut config = Config {
    background: config(BackgroundKind::Animated),
    ..Config::default()
  };
  config.power.low_power = true;
  let mut view = BackgroundView::new(&config);
  let frame = tiny_skia::Pixmap::new(4, 4).unwrap();
  view.loaded = Some(LoadedBackground {
    content: Content::Frames {
      cumulative: cumulative(&[Duration::from_millis(50), Duration::from_millis(100)]),
      delays: vec![Duration::from_millis(50), Duration::from_millis(50)],
      frames: vec![Arc::new(frame.clone()), Arc::new(frame)],
    },
    warning: None,
  });

  let frozen = view.frame_id(Instant::now());
  std::thread::sleep(Duration::from_millis(120));
  assert_eq!(
    view.frame_id(Instant::now()),
    frozen,
    "low-power frame id moved"
  );
  assert!(view.next_deadline(Instant::now()).is_none());
}
