use std::time::{Duration, Instant};

use super::{
  Scene, WidgetFrame, draw_media, ease_out_expo, glitch_window, parallax_offset, scanline_rows,
  scramble_run, scramble_text, split_last_char, ticker_flash, ticker_offset, widget_offset,
  widget_visible,
};
use crate::color::Rgb;
use crate::config::{Config, MediaLayout, MediaLook};
use crate::layout::Anchor;
use crate::plugin;
use crate::sysinfo::{BatterySnapshot, SystemSnapshot};
use crate::ui::background::BackgroundView;
use crate::ui::canvas::{Canvas, Rect};
use crate::ui::drag::Drag;
use crate::ui::glass::GlassCache;
use crate::ui::media::MediaSnapshot;
use crate::ui::motion::MotionFrame;
use crate::ui::text::TextEngine;
use crate::ui::theme::Theme;
use crate::ui::view::View;

/// Paints one media widget with a deliberate title and artist. Comparing
/// the pixels of two different titles is how we prove the renderer either
/// used them or ignored them completely.
fn media_pixels(
  authenticated: bool,
  show_before_login: bool,
  title: &str,
  artist: &str,
) -> Vec<u8> {
  let config = Config::default();
  let theme = Theme::from(&config);
  let view = View {
    authenticated,
    ..View::default()
  };
  let now = jiff::Zoned::now();
  let drag = Drag::empty();
  let frame_now = Instant::now();
  let scene = Scene {
    view: &view,
    theme: &theme,
    plugins: &[],
    now: &now,
    started: frame_now,
    frame_now,
    scale: 1.0,
    motion: MotionFrame::settled(),
    monitor: None,
    background_color: config.background.color,
    asset_warning: None,
    drag: &drag,
    cursor: None,
    screen: (480.0, 160.0),
    burst_at: None,
    hud: None,
    splash_art: None,
    post_gpu: false,
  };
  let snapshot = MediaSnapshot {
    available: true,
    title: title.to_string(),
    artist: artist.to_string(),
    playing: true,
    ..MediaSnapshot::default()
  };
  let mut canvas = Canvas::new(480, 160).unwrap();
  let mut text = TextEngine::new(&config.ui.font);
  let glass = GlassCache::new();
  draw_media(
    &mut canvas,
    &mut text,
    &glass,
    &scene,
    0,
    &snapshot,
    Anchor::TopLeft,
    (0, 0),
    Rect::new(0.0, 0.0, 480.0, 160.0),
    1.0,
    16.0,
    false,
    show_before_login,
    Rgb::solid(255, 255, 255),
    Rgb::solid(200, 200, 200),
    Rgb::solid(80, 120, 255),
    None,
    MediaLook::default(),
    0.0,
  );
  canvas.pixmap().data().to_vec()
}

/// Renders one media widget and returns only the transport rectangles. The
/// pixel test above deliberately uses the default look (controls off), so
/// this keeps the clickability check independent from the metadata privacy
/// check.
fn media_control_rects(controls: bool, angle: f32, available: bool) -> [Option<Rect>; 3] {
  let config = Config::default();
  let theme = Theme::from(&config);
  let view = View::default();
  let now = jiff::Zoned::now();
  let drag = Drag::empty();
  let frame_now = Instant::now();
  let scene = Scene {
    view: &view,
    theme: &theme,
    plugins: &[],
    now: &now,
    started: frame_now,
    frame_now,
    scale: 1.0,
    motion: MotionFrame::settled(),
    monitor: None,
    background_color: config.background.color,
    asset_warning: None,
    drag: &drag,
    cursor: None,
    screen: (480.0, 220.0),
    burst_at: None,
    hud: None,
    splash_art: None,
    post_gpu: false,
  };
  let snapshot = MediaSnapshot {
    available,
    title: "Song".to_string(),
    artist: "Artist".to_string(),
    playing: true,
    position: Duration::from_secs(30),
    length: Duration::from_secs(180),
    ..MediaSnapshot::default()
  };
  let look = MediaLook {
    controls,
    layout: MediaLayout::Card,
    ..MediaLook::default()
  };
  let mut canvas = Canvas::new(480, 220).unwrap();
  let mut text = TextEngine::new(&config.ui.font);
  let glass = GlassCache::new();
  draw_media(
    &mut canvas,
    &mut text,
    &glass,
    &scene,
    2,
    &snapshot,
    Anchor::TopLeft,
    (0, 0),
    Rect::new(0.0, 0.0, 480.0, 220.0),
    1.0,
    16.0,
    false,
    true,
    Rgb::solid(255, 255, 255),
    Rgb::solid(200, 200, 200),
    Rgb::solid(80, 120, 255),
    None,
    look,
    angle,
  )
  .controls
}

/// Renders the three new first-party widgets from a plain [`View`], which is
/// the same path the live window and `--screenshot` use. A fake battery and
/// system snapshot keep the test independent of the host hardware. Setting
/// `DAWN_WIDGET_SCREENSHOT` also writes the frame as a PNG for visual review;
/// a normal test run writes nothing.
#[test]
fn first_party_widgets_paint_from_the_view_snapshot() {
  let mut config = Config::default();
  config.clock.enabled = false;
  config.media.enabled = false;
  config.calendar.enabled = true;
  config.calendar.placement.anchor = Anchor::TopRight;
  config.calendar.placement.offset = (40, 40);
  config.battery.enabled = true;
  config.battery.placement.anchor = Anchor::BottomRight;
  config.battery.placement.offset = (40, 40);
  config.system.enabled = true;
  config.system.placement.anchor = Anchor::TopLeft;
  config.system.placement.offset = (40, 40);
  config.system.style = crate::config::SystemStyle::Sparkline;
  // The documented defaults are tuned for the light kinich wallpaper. On the
  // default dark preset the screenshot uses the theme's muted tone so the
  // labels stay readable.
  config.system.label_color = Rgb::solid(0xA9, 0xB4, 0xC8);
  config.calendar.muted_color = Rgb::solid(0xA9, 0xB4, 0xC8);

  let theme = Theme::from(&config);
  let battery = BatterySnapshot {
    present: true,
    percent: 73,
    charging: true,
    full: false,
  };
  let mut history = Vec::new();
  for index in 0..32 {
    history.push(0.45 + (index as f32 * 0.37).sin() * 0.35 + index as f32 * 0.01);
  }
  let system = SystemSnapshot {
    hostname: "dawn".to_string(),
    kernel: "6.12.4-dawn".to_string(),
    uptime: Duration::from_secs(3 * 86_400 + 4 * 3_600 + 12 * 60),
    load: [0.42, 0.31, 0.20],
    mem_used: 6_501_171,
    mem_total: 16_252_928,
    load_history: history,
  };
  let view = View {
    battery,
    system,
    ..View::default()
  };
  let builtins = plugin::builtins(&config);
  let now = jiff::Zoned::now();
  let drag = Drag::empty();
  let frame_now = Instant::now();
  let scene = Scene {
    view: &view,
    theme: &theme,
    plugins: &builtins.plugins,
    now: &now,
    started: frame_now,
    frame_now,
    scale: 1.0,
    motion: MotionFrame::settled(),
    monitor: None,
    background_color: config.background.color,
    asset_warning: None,
    drag: &drag,
    cursor: None,
    screen: (1280.0, 720.0),
    burst_at: None,
    hud: None,
    splash_art: None,
    post_gpu: false,
  };
  let mut canvas = Canvas::new(1280, 720).unwrap();
  let mut text = TextEngine::new(&config.ui.font);
  let mut background = BackgroundView::new(&config);
  background.load_blocking((1280, 720), None, frame_now);
  let mut glass = GlassCache::new();
  let regions = super::paint(&mut canvas, &mut text, &mut background, &mut glass, &scene);
  for id in ["calendar", "battery", "system"] {
    assert!(
      regions.iter().any(|region| region.id == id),
      "missing hit region for {id}"
    );
  }
  if let Some(path) = std::env::var_os("DAWN_WIDGET_SCREENSHOT") {
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path.parent() {
      std::fs::create_dir_all(parent).unwrap();
    }
    canvas.save_png(&path).unwrap();
  }
}

#[test]
fn scanline_rows_cover_the_height_and_clamp_a_zero_gap() {
  assert_eq!(scanline_rows(0, 10), 10);
  assert_eq!(scanline_rows(3, 9), 3);
  assert_eq!(scanline_rows(3, 10), 4);
  assert_eq!(scanline_rows(2, 720), 360);
}

#[test]
fn media_controls_expose_click_targets_only_when_they_can_work() {
  let controls = media_control_rects(true, 0.0, true);
  assert!(
    controls.iter().all(Option::is_some),
    "a visible, unrotated widget with controls should expose all three buttons"
  );
  let [previous, play, next] = controls.map(Option::unwrap);
  assert!(
    previous.x < play.x && play.x < next.x,
    "buttons should follow the drawn previous, play, next order"
  );

  // Controls off, a rotated widget, and the No media state all keep the
  // picture but must not offer a click target.
  assert!(
    media_control_rects(false, 0.0, true)
      .iter()
      .all(Option::is_none)
  );
  assert!(
    media_control_rects(true, 25.0, true)
      .iter()
      .all(Option::is_none)
  );
  assert!(
    media_control_rects(true, 0.0, false)
      .iter()
      .all(Option::is_none)
  );
}

#[test]
fn front_widget_offsets_and_extent_follow_the_card_scale() {
  // The card is 100x80 at 0.5 scale, so a front widget anchors inside that
  // shrunken card and its configured offset shrinks with it. This is the
  // exact math that keeps the kinich clock pill inside the scaled bar.
  let card = WidgetFrame::card(Rect::new(10.0, 20.0, 100.0, 80.0), 0.5);
  assert_eq!(card.origin, (10.0, 20.0));
  assert_eq!(card.extent, (100.0, 80.0));
  assert_eq!(card.scale, 0.5);
  assert_eq!(widget_offset(&card, (48.0, 14.0), (0.0, 0.0)), (24, 7));
  // Adding the drag before rounding matches decor and keeps a card-space
  // widget moving with the same pixel grid as its configured offset.
  assert_eq!(widget_offset(&card, (48.0, 14.0), (10.0, -4.0)), (29, 5));
  // A motion sample joins the configured offset in logical pixels before
  // the scale multiply, exactly like a decor item.
  assert_eq!(widget_offset(&card, (48.5, 14.4), (0.0, 0.0)), (24, 7));

  // The behind layer keeps the screen-space frame and the old rounding.
  let screen = WidgetFrame::screen((1920.0, 1080.0), 1.0);
  assert_eq!(screen.origin, (0.0, 0.0));
  assert_eq!(screen.extent, (1920.0, 1080.0));
  assert_eq!(widget_offset(&screen, (48.0, 14.0), (0.0, 0.0)), (48, 14));
  // A fractional sample rounds the same way the integer manifest offset
  // used to, so an unconfigured widget keeps its exact pixels.
  assert_eq!(widget_offset(&screen, (48.25, 14.75), (0.0, 0.0)), (48, 15));
}

#[test]
fn media_metadata_follows_the_privacy_setting() {
  // Shared machines can keep metadata off the login pixels. Single-user
  // machines can show the last track before login, which is the whole point
  // of the media widget in a desktop test or a `--dry-run` session.
  let first = media_pixels(false, false, "Secret Song", "Private Artist");
  let second = media_pixels(false, false, "Another Track", "Somebody Else");
  assert_eq!(
    first, second,
    "pre-auth media rendering ignored show_before_login = false"
  );

  let first = media_pixels(false, true, "Secret Song", "Private Artist");
  let second = media_pixels(false, true, "Another Track", "Somebody Else");
  assert_ne!(
    first, second,
    "pre-auth media rendering ignored show_before_login = true"
  );

  let first = media_pixels(true, false, "Secret Song", "Private Artist");
  let second = media_pixels(true, false, "Another Track", "Somebody Else");
  assert_ne!(
    first, second,
    "authenticated media rendering ignored the track metadata"
  );
}

#[test]
fn widget_visibility_prefers_hide_and_treats_an_empty_show_list_as_always() {
  use crate::config::WidgetState;
  use crate::ui::view::Line;

  let idle = View::default();
  // Empty show_on means always, even with a hide list that does not match.
  assert!(widget_visible(&[], &[], &idle));
  assert!(widget_visible(&[], &[WidgetState::Busy], &idle));

  // A state in hide_on wins even when show_on also lists it.
  assert!(!widget_visible(
    &[WidgetState::Typing],
    &[WidgetState::Typing],
    &View {
      username: "ada".to_string(),
      ..View::default()
    }
  ));

  // Typed content is the typing state.
  let typing = View {
    password: "secret".to_string(),
    ..View::default()
  };
  assert!(widget_visible(&[WidgetState::Typing], &[], &typing));
  assert!(!widget_visible(&[WidgetState::Idle], &[], &typing));

  // Busy outranks typing when a request is in flight.
  let busy = View {
    busy: true,
    username: "ada".to_string(),
    ..View::default()
  };
  assert!(widget_visible(&[WidgetState::Busy], &[], &busy));
  assert!(!widget_visible(&[WidgetState::Typing], &[], &busy));

  // An error status is the error state.
  let error = View {
    status: Some(Line::error("denied")),
    ..View::default()
  };
  assert!(widget_visible(&[WidgetState::Error], &[], &error));
  assert!(!widget_visible(&[WidgetState::Idle], &[], &error));

  // Authentication outranks an empty form.
  let authenticated = View {
    authenticated: true,
    ..View::default()
  };
  assert!(widget_visible(
    &[WidgetState::Authenticated],
    &[],
    &authenticated
  ));
  assert!(!widget_visible(&[WidgetState::Idle], &[], &authenticated));
}

#[test]
fn scramble_run_reveals_from_the_head_and_is_deterministic() {
  let run = "hello world";
  // A screenshot and a settled effect both show the final text.
  assert_eq!(scramble_text(run, 0.0, 1000.0, 3), run);
  assert_eq!(scramble_run(run, 1.0, 3), run);

  let half = scramble_run(run, 0.5, 3);
  let original: Vec<char> = run.chars().collect();
  let scrambled: Vec<char> = half.chars().collect();
  assert_eq!(scrambled.len(), original.len());
  for (index, (before, after)) in original.iter().zip(&scrambled).enumerate() {
    if before.is_whitespace() {
      assert_eq!(before, after, "whitespace at {index} must survive");
    } else if index < original.len() / 2 {
      assert_eq!(before, after, "head character at {index} should stay");
    } else {
      assert_ne!(before, after, "tail character at {index} should scramble");
    }
  }
  // The same tick produces the same pixels; a new tick is free to reshuffle.
  assert_eq!(scramble_run(run, 0.5, 3), half);
}

#[test]
fn glitch_window_is_open_only_at_the_start_of_a_period() {
  assert!(glitch_window(0.0, 1.0, 1000.0));
  assert!(glitch_window(0.05, 1.0, 1000.0));
  assert!(!glitch_window(0.08, 1.0, 1000.0));
  assert!(!glitch_window(0.5, 1.0, 1000.0));
  // The next period opens again.
  assert!(glitch_window(1.0, 1.0, 1000.0));
  assert!(!glitch_window(0.0, 0.0, 1000.0));
}

#[test]
fn parallax_follows_the_pointer_and_needs_strength() {
  let mut config = Config::default();
  config.motion.parallax = 1.0;
  let theme = Theme::from(&config);
  let view = View::default();
  let now = jiff::Zoned::now();
  let drag = Drag::empty();
  let frame_now = Instant::now();
  let mut scene = Scene {
    view: &view,
    theme: &theme,
    plugins: &[],
    now: &now,
    started: frame_now,
    frame_now,
    scale: 1.0,
    motion: MotionFrame::settled(),
    monitor: None,
    background_color: config.background.color,
    asset_warning: None,
    drag: &drag,
    cursor: None,
    screen: (100.0, 100.0),
    burst_at: None,
    hud: None,
    splash_art: None,
    post_gpu: false,
  };
  // No pointer means no offset, which is what a screenshot needs.
  assert_eq!(parallax_offset(&scene, 1.0, 1.0), (0.0, 0.0));
  scene.cursor = Some((0.0, 50.0));
  let left = parallax_offset(&scene, 1.0, 1.0);
  assert!(left.0 > 0.0);
  scene.cursor = Some((100.0, 50.0));
  let right = parallax_offset(&scene, 1.0, 1.0);
  assert!(right.0 < 0.0);
  // Depth zero removes the strength even while the pointer is inside.
  assert_eq!(parallax_offset(&scene, 0.0, 1.0), (0.0, 0.0));
}

#[test]
fn key_pop_splits_runs_at_character_boundaries() {
  assert_eq!(split_last_char(""), None);
  assert_eq!(split_last_char("a"), Some(("", "a")));
  assert_eq!(split_last_char("ada"), Some(("ad", "a")));
  // Multibyte glyphs must come back whole, never as a broken byte slice.
  assert_eq!(split_last_char("café"), Some(("caf", "é")));
  assert_eq!(split_last_char("✦✦"), Some(("✦", "✦")));
  assert_eq!(split_last_char("◆"), Some(("", "◆")));
}

#[test]
fn the_key_pop_eases_from_overshoot_to_rest() {
  assert_eq!(ease_out_expo(0.0), 0.0);
  assert_eq!(ease_out_expo(1.0), 1.0);
  // The overshoot decays monotonically and is nearly settled by half time.
  let early = ease_out_expo(0.1);
  let middle = ease_out_expo(0.5);
  assert!(early < middle);
  assert!(middle > 0.9);
  // At t = 0 the scale is the configured peak, at t = 1 it is exactly 1.
  let peak = 1.0 + (1.18_f32 - 1.0) * (1.0 - ease_out_expo(0.0));
  let rest = 1.0 + (1.18_f32 - 1.0) * (1.0 - ease_out_expo(1.0));
  assert!((peak - 1.18).abs() < 0.0001);
  assert!((rest - 1.0).abs() < 0.0001);
}

#[test]
fn ticker_flash_is_one_short_beat() {
  let now = Instant::now();
  assert!(!ticker_flash(now, None));
  assert!(ticker_flash(now, Some(now)));
  // 80 ms is the whole beat. A later frame goes back to the configured
  // crimson, which is what makes it a beat rather than a highlight.
  assert!(ticker_flash(now + Duration::from_millis(79), Some(now)));
  assert!(!ticker_flash(now + Duration::from_millis(80), Some(now)));
}

#[test]
fn ticker_offsets_wrap_at_the_measured_run_width() {
  // At rest the first copy starts at the strip origin.
  assert_eq!(ticker_offset(300.0, 60.0, 0.0), 0.0);
  // One second at 60 px/s advances 60 px.
  assert!((ticker_offset(300.0, 60.0, 1.0) - 60.0).abs() < 0.001);
  // At exactly one run width the offset wraps back to zero, so the seam is
  // invisible: the second copy is now where the first one started.
  assert!((ticker_offset(300.0, 60.0, 5.0)).abs() < 0.001);
  // A fractional number of loops keeps the remainder.
  assert!((ticker_offset(300.0, 60.0, 6.0) - 60.0).abs() < 0.001);
  // Degenerate measurements cannot divide by zero.
  assert_eq!(ticker_offset(0.0, 60.0, 1.0), 0.0);
  assert_eq!(ticker_offset(f32::INFINITY, 60.0, 1.0), 0.0);
  // A stopped or negative speed never scrolls backwards.
  assert_eq!(ticker_offset(300.0, 0.0, 10.0), 0.0);
  assert_eq!(ticker_offset(300.0, -60.0, 1.0), 0.0);
}
