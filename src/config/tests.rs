use super::{BackgroundKind, Config, ThemePreset, load};
use crate::layout::{Anchor, Fit};
use std::path::PathBuf;

fn temp_config(name: &str, contents: &str) -> PathBuf {
  let dir = std::env::temp_dir().join(format!("dawn-config-{}-{}", std::process::id(), name));
  std::fs::create_dir_all(&dir).unwrap();
  let path = dir.join("config.toml");
  std::fs::write(&path, contents).unwrap();
  path
}

#[test]
fn no_file_means_pure_defaults() {
  let loaded = load(Some(&PathBuf::from("/nonexistent/dawn/config.toml")));
  assert!(loaded.issues.iter().any(|issue| issue.key == "config"));
  assert_eq!(loaded.config.ui.font_size, 17.0);
  assert_eq!(loaded.config.theme.preset, ThemePreset::GlassDark);
  assert!(loaded.config.theme.blur.enabled);
  assert!(loaded.config.theme.blur.size > 0.0);
}

#[test]
fn encore_keys_parse_without_issues_and_reach_the_config() {
  let path = temp_config(
    "encore",
    r##"
[login.splash]
enabled = true
ms = 1800
title = "ENCORE"
sub = "the stage is warm"
fade_ms = 250
dismiss_on = "enter"
bg = "#0e0b10"
accent = "#d8a84b"

[[login.decor]]
kind = "text"
text = "beat"
pulse_on_beat = true

[login.decor.motion]
pulse = 0.1

[ticker]
enabled = true
items = ["ONE"]
pulse_on_beat = true

[particles]
enabled = true
burst_on_beat = true

[motion.beat]
bpm = 136
offset_ms = 120

[motion.camera]
punch = 0.08
punch_ms = 150
shake = 10
shake_ms = 220
breathing = 0.01
breath_ms = 5000

[motion.hitstop]
submit_ms = 120
success_ms = 200
failure_ms = 80

[motion.post]
speed_lines = 0.6
speed_lines_on = "both"
letterbox = 0.08
letterbox_animate_ms = 500
vignette = 0.4
chromatic = 0.5
glitch_ms = 180
glitch_on = "both"
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  let config = loaded.config;
  assert!(config.login.splash.enabled);
  assert_eq!(config.login.splash.title, "ENCORE");
  assert_eq!(config.login.splash.dismiss_on, super::SplashDismiss::Enter);
  assert!((config.motion.beat.bpm - 136.0).abs() < 1e-6);
  assert_eq!(config.motion.beat.offset.as_millis(), 120);
  assert!(config.login.decor.iter().any(|item| matches!(
    item,
    super::DecorSpec::Text {
      pulse_on_beat: true,
      ..
    }
  )));
  assert!(config.ticker.pulse_on_beat);
  assert!(config.particles.burst_on_beat);
  assert!((config.motion.camera.punch - 0.08).abs() < 1e-6);
  assert_eq!(config.motion.hitstop.submit.as_millis(), 120);
  assert!((config.motion.post.speed_lines - 0.6).abs() < 1e-6);
  assert_eq!(config.motion.post.glitch_ms, 180.0);
}

#[test]
fn valid_values_are_applied() {
  let path = temp_config(
    "valid",
    r##"
[ui]
font_size = 22
card_width = 420.5
greeting = "Hello there"

[clock]
time_format = "%H:%M:%S"
date_format = ""
seconds = true
hour12 = true
style = "analog"
anchor = "top-left"
offset = [10, 20]

[background]
kind = "image"
fit = "contain"
fps = 24
memory_limit_mb = 64
idle_pause_secs = 10

[background.monitors.DP-1]
fit = "fill"

[theme]
accent = "#ff8800"
preset = "minimal"
corner_radius = 9

[media]
enabled = false
anchor = "bottom-right"
size = 18

[motion]
boot_ms = 100
decor_fps = 24
[power]
low_power = true
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  assert_eq!(loaded.config.ui.font_size, 22.0);
  assert_eq!(loaded.config.ui.card_width, 420.5);
  assert_eq!(loaded.config.clock.date_format, "");
  assert!(loaded.config.clock.seconds);
  assert!(loaded.config.clock.hour12);
  assert_eq!(loaded.config.clock.style, super::ClockStyle::Analog);
  assert_eq!(loaded.config.clock.placement.anchor, Anchor::TopLeft);
  assert_eq!(loaded.config.background.kind, BackgroundKind::Image);
  assert_eq!(loaded.config.background.fit, Fit::Contain);
  assert_eq!(loaded.config.background.fps, 24);
  assert_eq!(loaded.config.background.memory_limit, 64 * 1024 * 1024);
  assert_eq!(
    loaded.config.background.for_monitor(Some("DP-1")).fit,
    Fit::Fill
  );
  assert_eq!(loaded.config.theme.preset, ThemePreset::Minimal);
  assert_eq!(loaded.config.theme.corner_radius, 9.0);
  assert_eq!(
    loaded.config.theme.accent,
    crate::color::Rgb::solid(255, 136, 0)
  );
  assert!(!loaded.config.media.enabled);
  assert_eq!(loaded.config.media.placement.anchor, Anchor::BottomRight);
  assert!(loaded.config.power.low_power);
  assert_eq!(loaded.config.motion.boot.as_millis(), 100);
  assert_eq!(loaded.config.motion.decor_fps, 24);
}

#[test]
fn login_prefill_accepts_auto_none_and_a_name() {
  let path = temp_config(
    "prefill",
    r#"
[login]
prefill = "alice"
"#,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  assert_eq!(
    loaded.config.login.prefill,
    super::UsernamePrefill::Name("alice".to_string())
  );

  let path = temp_config(
    "prefill-none",
    r#"
[login]
prefill = "none"
"#,
  );
  let loaded = load(Some(&path));
  assert_eq!(loaded.config.login.prefill, super::UsernamePrefill::None);

  let path = temp_config(
    "prefill-auto",
    r#"
[login]
prefill = "auto"
"#,
  );
  let loaded = load(Some(&path));
  assert_eq!(loaded.config.login.prefill, super::UsernamePrefill::Auto);
}

#[test]
fn a_bad_login_prefill_falls_back_to_auto() {
  let path = temp_config(
    "prefill-bad",
    r#"
[login]
prefill = "bad name"
"#,
  );
  let loaded = load(Some(&path));
  assert_eq!(loaded.config.login.prefill, super::UsernamePrefill::Auto);
  assert!(
    loaded
      .issues
      .iter()
      .any(|issue| issue.key == "login.prefill")
  );
}

#[test]
fn login_form_and_layout_are_configurable() {
  let path = temp_config(
    "login",
    r##"
[ui]
greeting = "Old greeting"
card_width = 410

[login]
anchor = "top-left"
offset = [20, 30]
width = 500
padding = 40
gap = 8
radius = 12
draggable = true
greeting = "Hello"
picker_title = "Pick a desktop"
busy_text = "Working"
dry_run_badge = "TEST"
show_hints = false
show_session = false
show_picker = false
no_sessions = "Nothing to start"

[login.field]
height = 40
radius = 8
background = "#112233"
caret_width = 3

[login.username]
placeholder = "User"
label = "Account"
text = "#ffffff"

[login.password]
mask = "*"
placeholder_color = "#778899"

[login.session]
label = "Desktop"
empty = "No desktop"
change_hint = "Press Tab"

[login.hints]
username = "Enter!"
password = "Sign in!"
picker = "Choose!"
busy = "Please wait"

[layout]
save = true
file = "state/layout.toml"
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  let login = &loaded.config.login;
  assert_eq!(login.placement.anchor, Anchor::TopLeft);
  assert_eq!(login.placement.offset, (20, 30));
  assert_eq!(login.width, 500.0);
  assert_eq!(login.padding, 40.0);
  assert_eq!(login.gap, 8.0);
  assert_eq!(login.radius, 12.0);
  assert!(login.draggable);
  assert_eq!(login.greeting, "Hello");
  assert_eq!(login.picker_title, "Pick a desktop");
  assert_eq!(login.busy_text, "Working");
  assert_eq!(login.dry_run_badge, "TEST");
  assert!(!login.show_hints);
  assert!(!login.session.show);
  assert!(!login.show_picker);
  assert_eq!(login.no_sessions, "Nothing to start");
  assert_eq!(login.field.height, 40.0);
  assert_eq!(login.field.radius, 8.0);
  assert_eq!(login.field.caret_width, 3.0);
  assert_eq!(login.username.label, "Account");
  assert_eq!(login.username.placeholder, "User");
  assert_eq!(login.username.height, 40.0);
  assert_eq!(login.password.mask, "*");
  assert_eq!(login.session.label, "Desktop");
  assert_eq!(login.hints.username, "Enter!");
  assert!(loaded.config.layout.save);
  let expected = path.parent().unwrap().join("state").join("layout.toml");
  assert_eq!(
    loaded.config.layout.file.as_deref(),
    Some(expected.as_path())
  );
}

#[test]
fn hero_layout_button_and_decor_are_configurable() {
  let path = temp_config(
    "hero",
    r##"
[login]
layout = "hero"
width = 1160
height = 663
padding = 0
panel = "#ff4655e6"
panel_fade = [0.38, 0.62]
border = "#ffffff33"
tab = "Login"
form_offset = [610, 246]
form_width = 495
show_session = false

[login.username]
label = "Account"
label_color = "#22ff88"
underline = "#ff0000"
underline_size = 2.5
placeholder = "User Name"
icon = "user"
icon_size = 20
icon_gap = 12
background = "#ffffff73"
background_end = "#ffffff33"
bracket = "#0f1923"
bracket_gap = 12

[login.button]
show = true
text = "GET STARTED"
gap = 42
height = 48
radius = 0
font_size = 19
bold = true
tracking = 0.08
background = "#ff4655"
text_color = "#0f1923"

[[login.decor]]
kind = "image"
image = "logo.png"
width = 174
anchor = "top-left"
offset = [430, 42]

[[login.decor]]
kind = "text"
text = "Footer"
size = 12
color = "#ffffff"
anchor = "bottom-right"
offset = [20, 20]

[login.decor.motion]
spin = 8.0
sway = 4.0
sway_ms = 5000
float = [14, -10]
float_ms = 7000
pulse = 0.06
pulse_ms = 2200
fade = 0.25
fade_ms = 3200
phase = 0.25
"##,
  );
  // The decor validator checks that image paths exist and are real files.
  let png = [
    137u8, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
    0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31,
    0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
  ];
  std::fs::write(path.parent().unwrap().join("logo.png"), png).unwrap();

  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  let login = &loaded.config.login;
  assert_eq!(login.layout, super::LoginLayout::Hero);
  assert_eq!(login.height, 663.0);
  assert_eq!(login.panel_fade, (0.38, 0.62));
  assert_eq!(login.tab, "Login");
  assert_eq!(login.form_offset, (610, 246));
  assert_eq!(login.form_width, 495.0);
  assert!(login.button.show);
  assert_eq!(login.button.text, "GET STARTED");
  assert!(login.button.bold);
  assert_eq!(login.button.tracking, 0.08);
  assert_eq!(login.username.icon, "user");
  assert_eq!(login.username.icon_size, 20.0);
  assert_eq!(login.username.bracket_gap, 12.0);
  assert_eq!(login.username.label, "Account");
  assert_eq!(
    login.username.label_color,
    Some(crate::color::Rgb::solid(0x22, 0xff, 0x88))
  );
  assert_eq!(
    login.username.underline,
    Some(crate::color::Rgb::solid(0xff, 0, 0))
  );
  assert_eq!(login.username.underline_size, 2.5);
  assert_eq!(
    login.username.background_end,
    Some(crate::color::Rgb::solid(0xff, 0xff, 0xff).with_alpha(0x33))
  );
  assert_eq!(login.decor.len(), 2);
  match &login.decor[0] {
    super::DecorSpec::Image { width, offset, .. } => {
      assert_eq!(*width, 174.0);
      assert_eq!(*offset, (430, 42));
    }
    other => panic!("expected an image decor, got {other:?}"),
  }
  match &login.decor[1] {
    super::DecorSpec::Text {
      text,
      size,
      anchor,
      motion,
      ..
    } => {
      assert_eq!(text, "Footer");
      assert_eq!(*size, 12.0);
      assert_eq!(*anchor, Anchor::BottomRight);
      assert_eq!(motion.spin, 8.0);
      assert_eq!(motion.sway, 4.0);
      assert_eq!(motion.sway_ms, 5000.0);
      assert_eq!(motion.float, (14, -10));
      assert_eq!(motion.float_ms, 7000.0);
      assert_eq!(motion.pulse, 0.06);
      assert_eq!(motion.pulse_ms, 2200.0);
      assert_eq!(motion.fade, 0.25);
      assert_eq!(motion.fade_ms, 3200.0);
      assert_eq!(motion.phase, 0.25);
      assert!(motion.active());
    }
    other => panic!("expected a text decor, got {other:?}"),
  }
}

#[test]
fn decor_motion_is_neutral_at_zero_and_uses_its_phase() {
  let motion = super::DecorMotion {
    spin: 90.0,
    sway: 10.0,
    sway_ms: 4000.0,
    float: (20, -10),
    float_ms: 8000.0,
    pulse: 0.1,
    pulse_ms: 2000.0,
    fade: 0.4,
    fade_ms: 4000.0,
    phase: 0.0,
    typer_ms: 0.0,
    blink_ms: 0.0,
    ticker_ms: 0.0,
    ticker_width: 0,
    ticker_gap: 64,
    ..super::DecorMotion::default()
  };
  let start = motion.sample(0.0);
  assert_eq!(start.offset, (0.0, 0.0));
  assert_eq!(start.angle, 0.0);
  assert_eq!(start.scale, 1.0);
  assert_eq!(start.opacity, 1.0);
  // 90 deg/s of spin plus a 10 degree sway at the top of its 4 s cycle.
  assert!((motion.sample(1.0).angle - 100.0).abs() < 0.01);
  // Pulse peaks a quarter of the way through its 2 s period.
  assert!((motion.sample(0.5).scale - 1.1).abs() < 0.001);
  // Fade reaches its deepest halfway through its 4 s period.
  assert!((motion.sample(2.0).opacity - 0.6).abs() < 0.001);
  // A phase offset moves the cycle forward without moving the start pose.
  let staggered = super::DecorMotion {
    phase: 0.25,
    ..motion
  };
  let shifted = staggered.sample(0.0);
  assert_eq!(shifted.offset, (0.0, 0.0));
  assert_eq!(shifted.angle, 0.0);
  assert!((shifted.scale - 1.0).abs() < 0.001);
  assert!((shifted.opacity - 1.0).abs() < 0.001);
  assert!(!super::DecorMotion::default().active());
}

#[test]
fn decor_typer_reveal_is_linear_and_off_by_default() {
  let motion = super::DecorMotion {
    typer_ms: 1000.0,
    ..super::DecorMotion::default()
  };
  assert_eq!(motion.sample(0.0).reveal, 0.0);
  assert!((motion.sample(0.5).reveal - 0.5).abs() < 0.001);
  assert_eq!(motion.sample(1.0).reveal, 1.0);
  // Beyond the duration the text stays complete instead of restarting.
  assert_eq!(motion.sample(4.0).reveal, 1.0);
  // With the typer off, expect a full reveal at any age.
  assert_eq!(super::DecorMotion::default().sample(99.0).reveal, 1.0);
}

#[test]
fn decor_blink_is_a_square_wave() {
  let motion = super::DecorMotion {
    blink_ms: 1000.0,
    ..super::DecorMotion::default()
  };
  assert!(motion.sample(0.0).blink_on);
  // The second half of the first period is dark.
  assert!(!motion.sample(1.1).blink_on);
  // The first half of the second period is visible again.
  assert!(motion.sample(2.3).blink_on);
  assert!(super::DecorMotion::default().sample(1.1).blink_on);
}

#[test]
fn decor_ticker_loop_fraction_wraps_within_one_tile() {
  let motion = super::DecorMotion {
    ticker_ms: 1000.0,
    ticker_width: 300,
    ..super::DecorMotion::default()
  };
  assert_eq!(motion.sample(0.0).loop_frac, 0.0);
  assert!((motion.sample(0.25).loop_frac - 0.25).abs() < 0.001);
  let wrapped = motion.sample(1.25).loop_frac;
  assert!((wrapped - 0.25).abs() < 0.001);
  assert!((0.0..1.0).contains(&wrapped));
  assert_eq!(motion.sample(2.0).loop_frac, 0.0);
  assert_eq!(super::DecorMotion::default().sample(1.0).loop_frac, 0.0);
}

#[test]
fn ticker_session_first_moves_the_live_session_and_keeps_defaults_stable() {
  let ticker = super::TickerConfig {
    enabled: true,
    items: vec!["A".to_string(), "B".to_string()],
    separator: " | ".to_string(),
    session_first: false,
    ..super::TickerConfig::default()
  };
  // Off is the historical run exactly.
  assert_eq!(ticker.run(), "A | B | ");
  assert_eq!(ticker.run_with(Some("GNOME")), "A | B | ");

  let first = super::TickerConfig {
    session_first: true,
    ..ticker
  };
  assert_eq!(first.run_with(Some("GNOME")), "GNOME | A | B | ");
  // The poster voice is uppercase without touching the actual session id.
  assert_eq!(first.run_with(Some("gnome")), "GNOME | A | B | ");
  // A missing session falls back to the configured list without an empty
  // leading tile.
  assert_eq!(first.run_with(Some("")), "A | B | ");
  assert_eq!(first.run_with(None), "A | B | ");
  // A configured duplicate does not stutter at the seam.
  let duplicate = super::TickerConfig {
    items: vec!["GNOME".to_string(), "B".to_string()],
    ..first
  };
  assert_eq!(duplicate.run_with(Some("GNOME")), "GNOME | B | ");
}

#[test]
fn field_key_pop_defaults_off_and_accepts_a_theme_value() {
  let defaults = super::FieldConfig::default_username();
  assert_eq!(defaults.key_pop_ms, 0.0);
  assert_eq!(defaults.key_pop_scale, 1.18);
  assert!(defaults.key_pop_color.is_none());
  // The pop defaults are inert: the byte-identical rule depends on the zero.
  assert_eq!(super::FieldConfig::default_password().key_pop_ms, 0.0);
}

#[test]
fn burst_and_streak_keys_parse_with_off_defaults() {
  assert_eq!(
    super::BurstTrigger::parse("success"),
    Some(super::BurstTrigger::Success)
  );
  assert_eq!(
    super::BurstTrigger::parse("off"),
    Some(super::BurstTrigger::None)
  );
  assert_eq!(super::BurstTrigger::parse("enter"), None);
  assert_eq!(
    super::ParticleSprite::parse("streak"),
    Some(super::ParticleSprite::Streak)
  );
  let config = super::ParticlesConfig::default();
  assert_eq!(config.burst_on, super::BurstTrigger::None);
  assert_eq!(config.burst_count, 24);
}

#[test]
fn the_stage_call_keys_resolve_together() {
  let path = temp_config(
    "stage-call",
    r##"
[login.field]
label_size = 9
label_tracking = 0.3
key_pop_ms = 140
key_pop_scale = 1.2
key_pop_color = "#ff7a3c"

[login.transition]
enabled = true
trigger = "boot"

[clock]
label = "WANGSHENG PRESENTS"
label_color = "#d8a84b"
label_size = 11
label_tracking = 0.3

[particles]
enabled = true
burst_on = "success"
burst_sprite = "streak"
burst_count = 40

[ticker]
enabled = true
session_first = true
items = ["ONE"]
"##,
  );
  let loaded = load(Some(&path));
  assert!(
    loaded.issues.is_empty(),
    "unexpected config issues: {:?}",
    loaded.issues
  );
  let config = loaded.config;
  assert_eq!(config.login.field.label_size, 9.0);
  assert_eq!(config.login.field.label_tracking, 0.3);
  assert_eq!(config.login.field.key_pop_ms, 140.0);
  assert_eq!(config.login.field.key_pop_scale, 1.2);
  assert!(config.login.field.key_pop_color.is_some());
  assert_eq!(
    config.login.transition.trigger,
    super::TransitionTrigger::Boot
  );
  assert_eq!(config.clock.label, "WANGSHENG PRESENTS");
  assert_eq!(config.clock.label_tracking, 0.3);
  assert_eq!(config.particles.burst_on, super::BurstTrigger::Success);
  assert_eq!(config.particles.burst_sprite, super::ParticleSprite::Streak);
  assert_eq!(config.particles.burst_count, 40);
  assert!(config.ticker.session_first);
}

#[test]
fn poster_is_a_distinct_full_bleed_layout() {
  assert!(!super::LoginLayout::Card.is_full_bleed());
  assert!(super::LoginLayout::Hero.is_full_bleed());
  assert!(super::LoginLayout::Poster.is_full_bleed());
  let path = temp_config(
    "poster",
    r##"
[login]
layout = "poster"
width = 1920
height = 1080
surface = "none"
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  assert_eq!(loaded.config.login.layout, super::LoginLayout::Poster);
}

#[test]
fn enter_choreography_starts_hidden_and_eases_in() {
  let fade = super::DecorMotion {
    enter: super::EnterStyle::Fade,
    ..super::DecorMotion::default()
  };
  // The entrance is deliberately not neutral at zero: it starts hidden.
  assert_eq!(fade.sample(0.0).opacity, 0.0);
  assert!((fade.sample(0.6).opacity - 1.0).abs() < 0.001);

  let delayed = super::DecorMotion {
    enter_delay_ms: 300.0,
    ..fade
  };
  assert_eq!(delayed.sample(0.0).opacity, 0.0);
  assert_eq!(delayed.sample(0.3).opacity, 0.0);
  let midway = delayed.sample(0.6).opacity;
  assert!(midway > 0.0 && midway < 1.0);
  assert!((delayed.sample(0.9).opacity - 1.0).abs() < 0.001);

  let slide = super::DecorMotion {
    enter: super::EnterStyle::Slide,
    ..super::DecorMotion::default()
  };
  assert!((slide.sample(0.0).offset.0 + 28.0).abs() < 0.001);
  assert!(slide.sample(0.6).offset.0.abs() < 0.001);

  let scale = super::DecorMotion {
    enter: super::EnterStyle::Scale,
    ..super::DecorMotion::default()
  };
  let opening = scale.sample(0.0).scale;
  assert!(opening < 1.0);
  assert!(opening >= 0.9);
  assert!((scale.sample(0.6).scale - 1.0).abs() < 0.001);
}

#[test]
fn sample_carries_elapsed_and_new_motions_are_active() {
  let sample = super::DecorMotion::default().sample(2.5);
  assert_eq!(sample.elapsed, 2.5);
  // Non-finite input has no usable age; the neutral sample reports zero.
  assert_eq!(super::DecorMotion::default().sample(f32::NAN).elapsed, 0.0);
  assert_eq!(super::DecorMotion::default().sample(-1.0).elapsed, 0.0);

  let cases = [
    super::DecorMotion {
      wave: 4.0,
      ..super::DecorMotion::default()
    },
    super::DecorMotion {
      scramble_ms: 800.0,
      ..super::DecorMotion::default()
    },
    super::DecorMotion {
      glitch: 0.5,
      ..super::DecorMotion::default()
    },
    super::DecorMotion {
      parallax_depth: 0.5,
      ..super::DecorMotion::default()
    },
    super::DecorMotion {
      enter: super::EnterStyle::Fade,
      ..super::DecorMotion::default()
    },
  ];
  for motion in cases {
    assert!(motion.active(), "{motion:?} should request animation");
  }
}

#[test]
fn decor_motion_active_tracks_the_text_motions() {
  let typer = super::DecorMotion {
    typer_ms: 1.0,
    ..super::DecorMotion::default()
  };
  assert!(typer.active());
  let blink = super::DecorMotion {
    blink_ms: 1.0,
    ..super::DecorMotion::default()
  };
  assert!(blink.active());
  let ticker = super::DecorMotion {
    ticker_ms: 1.0,
    ticker_width: 100,
    ..super::DecorMotion::default()
  };
  assert!(ticker.active());
  // A ticker with no width cannot draw itself, so it must not request
  // animation frames that would only repaint the same pixels.
  let nowhere = super::DecorMotion {
    ticker_ms: 1.0,
    ..super::DecorMotion::default()
  };
  assert!(!nowhere.active());
}

#[test]
fn text_decor_motions_and_block_caret_are_configurable() {
  let path = temp_config(
    "text-motions",
    r##"
[[login.decor]]
kind = "text"
text = "HUD"
[login.decor.motion]
typer_ms = 1200
blink_ms = 900
ticker_ms = 3000
ticker_width = 300
ticker_gap = 80

[login.username]
caret_style = "block"
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  match &loaded.config.login.decor[0] {
    super::DecorSpec::Text { motion, .. } => {
      assert_eq!(motion.typer_ms, 1200.0);
      assert_eq!(motion.blink_ms, 900.0);
      assert_eq!(motion.ticker_ms, 3000.0);
      assert_eq!(motion.ticker_width, 300);
      assert_eq!(motion.ticker_gap, 80);
    }
    other => panic!("expected a text decor, got {other:?}"),
  }
  assert_eq!(
    loaded.config.login.username.caret_style,
    super::CaretStyle::Block
  );
}

#[test]
fn the_motion_pack_keys_parse_with_their_ranges() {
  let path = temp_config(
    "motion-pack",
    r##"
[motion]
parallax = 1.5

[[login.decor]]
kind = "text"
text = "MOTION"
[login.decor.motion]
parallax_depth = 0.8
enter = "slide"
enter_ms = 1200
enter_delay_ms = 250
enter_from = "bottom"
wave = 12
wave_ms = 3000
wave_step = 0.2
scramble_ms = 1800
glitch = 0.6
glitch_ms = 1500
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  assert_eq!(loaded.config.motion.parallax, 1.5);
  let motion = match &loaded.config.login.decor[0] {
    super::DecorSpec::Text { motion, .. } => *motion,
    other => panic!("expected a text decor, got {other:?}"),
  };
  assert_eq!(motion.parallax_depth, 0.8);
  assert_eq!(motion.enter, super::EnterStyle::Slide);
  assert_eq!(motion.enter_ms, 1200.0);
  assert_eq!(motion.enter_delay_ms, 250.0);
  assert_eq!(motion.enter_from, super::EnterFrom::Bottom);
  assert_eq!(motion.wave, 12.0);
  assert_eq!(motion.wave_ms, 3000.0);
  assert_eq!(motion.wave_step, 0.2);
  assert_eq!(motion.scramble_ms, 1800.0);
  assert_eq!(motion.glitch, 0.6);
  assert_eq!(motion.glitch_ms, 1500.0);
}

#[test]
fn bad_motion_pack_values_fall_back_with_issues() {
  let path = temp_config(
    "bad-motion-pack",
    r##"
[motion]
parallax = 9

[[login.decor]]
kind = "text"
text = "MOTION"
[login.decor.motion]
parallax_depth = 2
enter = "wobble"
enter_ms = 99999
enter_delay_ms = -1
enter_from = "diagonal"
wave = 99
wave_ms = 10
wave_step = 5
scramble_ms = -1
glitch = 2
glitch_ms = 1
"##,
  );
  let loaded = load(Some(&path));
  assert_eq!(loaded.config.motion.parallax, 0.0);
  let motion = match &loaded.config.login.decor[0] {
    super::DecorSpec::Text { motion, .. } => *motion,
    other => panic!("expected a text decor, got {other:?}"),
  };
  let fallback = super::DecorMotion::default();
  assert_eq!(motion.parallax_depth, fallback.parallax_depth);
  assert_eq!(motion.enter, fallback.enter);
  assert_eq!(motion.enter_ms, fallback.enter_ms);
  assert_eq!(motion.enter_delay_ms, fallback.enter_delay_ms);
  assert_eq!(motion.enter_from, fallback.enter_from);
  assert_eq!(motion.wave, fallback.wave);
  assert_eq!(motion.wave_ms, fallback.wave_ms);
  assert_eq!(motion.wave_step, fallback.wave_step);
  assert_eq!(motion.scramble_ms, fallback.scramble_ms);
  assert_eq!(motion.glitch, fallback.glitch);
  assert_eq!(motion.glitch_ms, fallback.glitch_ms);
  for key in [
    "motion.parallax",
    "login.decor[0].motion.parallax_depth",
    "login.decor[0].motion.enter",
    "login.decor[0].motion.enter_ms",
    "login.decor[0].motion.enter_delay_ms",
    "login.decor[0].motion.enter_from",
    "login.decor[0].motion.wave",
    "login.decor[0].motion.wave_ms",
    "login.decor[0].motion.wave_step",
    "login.decor[0].motion.scramble_ms",
    "login.decor[0].motion.glitch",
    "login.decor[0].motion.glitch_ms",
  ] {
    assert!(
      loaded.issues.iter().any(|issue| issue.key == key),
      "missing issue {key}: {:?}",
      loaded.issues
    );
  }
}

#[test]
fn bad_text_motions_and_caret_style_fall_back_with_issues() {
  let path = temp_config(
    "bad-text-motions",
    r##"
[login.field]
caret_style = "sparkle"

[[login.decor]]
kind = "text"
text = "HUD"
[login.decor.motion]
typer_ms = -1
blink_ms = 30000
ticker_ms = 200000
ticker_width = 9999
ticker_gap = 9999
"##,
  );
  let loaded = load(Some(&path));
  let motion = match &loaded.config.login.decor[0] {
    super::DecorSpec::Text { motion, .. } => *motion,
    other => panic!("expected a text decor, got {other:?}"),
  };
  assert_eq!(motion.typer_ms, 0.0);
  assert_eq!(motion.blink_ms, 0.0);
  assert_eq!(motion.ticker_ms, 0.0);
  assert_eq!(motion.ticker_width, 0);
  assert_eq!(motion.ticker_gap, 64);
  assert_eq!(
    loaded.config.login.field.caret_style,
    super::CaretStyle::Bar
  );
  assert_eq!(
    loaded.config.login.username.caret_style,
    super::CaretStyle::Bar
  );
  for key in [
    "login.decor[0].motion.typer_ms",
    "login.decor[0].motion.blink_ms",
    "login.decor[0].motion.ticker_ms",
    "login.decor[0].motion.ticker_width",
    "login.decor[0].motion.ticker_gap",
    "login.field.caret_style",
  ] {
    assert!(
      loaded.issues.iter().any(|issue| issue.key == key),
      "missing issue {key}: {:?}",
      loaded.issues
    );
  }
}

#[test]
fn a_ticker_without_a_width_is_reported() {
  let path = temp_config(
    "ticker-width",
    r##"
[[login.decor]]
kind = "text"
text = "Breaking news"
[login.decor.motion]
ticker_ms = 3000
"##,
  );
  let loaded = load(Some(&path));
  let issue = loaded
    .issues
    .iter()
    .find(|issue| issue.key == "login.decor[0].motion.ticker_width")
    .unwrap_or_else(|| panic!("expected a ticker_width issue, got {:?}", loaded.issues));
  assert!(issue.message.contains("ticker_width"), "{issue:?}");
}

#[test]
fn caret_style_defaults_to_bar() {
  let config = Config::default();
  assert_eq!(config.login.field.caret_style, super::CaretStyle::Bar);
  assert_eq!(config.login.username.caret_style, super::CaretStyle::Bar);
  assert_eq!(config.login.password.caret_style, super::CaretStyle::Bar);
}

#[test]
fn surface_rotation_button_effects_and_blur_are_configurable() {
  let path = temp_config(
    "effects",
    r##"
[login]
surface = "none"
show_shadow = false
show_border = false
rotation = 7.5

[login.field]
rotation = -3

[login.username]
placeholder = "User"

[login.button]
show = true
text = "PLAY"
background = "#112233"
background_end = "#445566"
gradient_angle = 90
hover_background = "#778899"
hover_background_end = "#aabbcc"
hover_text_color = "#ffffff"
hover_direction = "left"
hover_ms = 240
hover_scale = 1.05
hover_lift = 3
glow = "#ff4655"
glow_size = 12
effect = "sweep"
effect_color = "#ffffff"
effect_ms = 1800
effect_angle = 115
effect_strength = 0.4
rotation = 6

[[login.decor]]
kind = "text"
text = "Sideways"
anchor = "top-left"
offset = [10, 10]
rotation = -20

[theme.blur]
enabled = true
size = 12
passes = 4
noise = 0.02
contrast = 0.9
brightness = 0.8
vibrancy = 0.2
vibrancy_darkness = 0.1
refresh_ms = 400

[clock]
rotation = 5

[media]
rotation = -5
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  let login = &loaded.config.login;
  assert_eq!(login.surface, super::SurfaceStyle::None);
  assert!(!login.show_shadow);
  assert!(!login.show_border);
  assert_eq!(login.rotation, 7.5);
  assert_eq!(login.username.rotation, -3.0);
  let button = &login.button;
  assert_eq!(
    button.background_end,
    Some(crate::color::Rgb::solid(0x44, 0x55, 0x66))
  );
  assert_eq!(button.gradient_angle, 90.0);
  assert_eq!(button.hover_direction, super::HoverReveal::Left);
  assert_eq!(button.hover_ms, 240.0);
  assert_eq!(button.hover_scale, 1.05);
  assert_eq!(button.hover_lift, 3.0);
  assert_eq!(button.effect, super::ButtonEffect::Sweep);
  assert_eq!(button.effect_ms, 1800.0);
  assert_eq!(button.effect_angle, 115.0);
  assert_eq!(button.effect_strength, 0.4);
  assert_eq!(button.rotation, 6.0);
  match &login.decor[0] {
    super::DecorSpec::Text { rotation, .. } => assert_eq!(*rotation, -20.0),
    other => panic!("expected text decor, got {other:?}"),
  }
  let blur = &loaded.config.theme.blur;
  assert!(blur.enabled);
  assert_eq!(blur.size, 12.0);
  assert_eq!(blur.passes, 4);
  assert!((blur.noise - 0.02).abs() < f32::EPSILON);
  assert!((blur.contrast - 0.9).abs() < f32::EPSILON);
  assert!((blur.brightness - 0.8).abs() < f32::EPSILON);
  assert!((blur.vibrancy - 0.2).abs() < f32::EPSILON);
  assert!((blur.vibrancy_darkness - 0.1).abs() < f32::EPSILON);
  assert_eq!(blur.refresh, std::time::Duration::from_millis(400));
  assert_eq!(loaded.config.clock.placement.rotation, 5.0);
  assert_eq!(loaded.config.media.placement.rotation, -5.0);
}

#[test]
fn legacy_ui_aliases_still_supply_login_defaults() {
  let path = temp_config(
    "legacy-login",
    "[ui]\ngreeting = \"Hi\"\ncard_width = 410\n",
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  assert_eq!(loaded.config.login.greeting, "Hi");
  assert_eq!(loaded.config.login.width, 410.0);
}

#[test]
fn a_bad_field_falls_back_without_discarding_good_ones() {
  let path = temp_config(
    "partial",
    r#"
[ui]
font_size = "huge"
card_width = 500

[clock]
time_format = "%E"

[background]
kind = "videoo"
fps = 500
"#,
  );
  let loaded = load(Some(&path));
  assert_eq!(loaded.config.ui.font_size, 17.0);
  assert_eq!(loaded.config.ui.card_width, 500.0);
  assert_eq!(loaded.config.clock.time_format, "%H:%M");
  assert_eq!(loaded.config.background.kind, BackgroundKind::Auto);
  assert_eq!(loaded.config.background.fps, 0);
  assert_eq!(loaded.issues.len(), 4, "{:?}", loaded.issues);
  assert!(
    loaded
      .issues
      .iter()
      .any(|issue| issue.key == "ui.font_size")
  );
  assert!(
    loaded
      .issues
      .iter()
      .any(|issue| issue.key == "background.kind")
  );
}

#[test]
fn a_syntax_error_still_yields_defaults() {
  let path = temp_config("syntax", "[ui\nfont_size = 20\n");
  let loaded = load(Some(&path));
  assert_eq!(loaded.config.ui.font_size, 17.0);
  assert_eq!(loaded.issues.len(), 1);
  assert_eq!(loaded.issues[0].key, "config");
  assert!(loaded.issues[0].location.is_some());
}

#[test]
fn unknown_keys_are_reported_with_a_line() {
  let path = temp_config("unknown", "[ui]\nfont_sizee = 20\n");
  let loaded = load(Some(&path));
  assert_eq!(loaded.issues.len(), 1);
  assert!(loaded.issues[0].to_string().contains("font_sizee"));
  assert!(loaded.issues[0].to_string().contains(":2:"));
}

#[test]
fn empty_session_list_is_rejected_in_favour_of_defaults() {
  let path = temp_config("sessions", "[sessions]\ndirs = []\n");
  let loaded = load(Some(&path));
  assert!(!loaded.config.sessions.dirs.is_empty());
  assert!(loaded.issues.iter().any(|i| i.key == "sessions.dirs"));
}

#[test]
fn light_preset_supplies_a_light_background() {
  let path = temp_config("light", "[theme]\npreset = \"glass-light\"\n");
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  assert_eq!(
    loaded.config.background.color,
    ThemePreset::GlassLight.background_color()
  );
}

#[test]
fn bundled_defaults_are_sane() {
  let config = Config::default();
  assert!(config.ui.fullscreen);
  assert!(config.auth.timeout.as_secs() >= 5);
  assert!(config.plugins.dirs.iter().any(|dir| dir.is_absolute()));
  assert!(config.clock.enabled);
  assert!(config.media.enabled);
  assert!(!config.calendar.enabled);
  assert!(!config.battery.enabled);
  assert!(!config.system.enabled);
  assert_eq!(config.calendar.week_start, super::WeekStart::Monday);
  assert_eq!(config.battery.style, super::BatteryStyle::Ring);
  assert_eq!(config.system.style, super::SystemStyle::Rows);
  assert_eq!(
    config.system.fields,
    [
      super::SystemField::Hostname,
      super::SystemField::Kernel,
      super::SystemField::Uptime,
      super::SystemField::Load,
      super::SystemField::Memory
    ]
  );
  assert!(config.motion.enabled);
  assert!(!config.power.low_power);
}

#[test]
fn per_monitor_issues_name_the_exact_table() {
  // The admin needs to know which output override is wrong, not just that
  // "background.monitors" has a problem somewhere.
  let path = temp_config(
    "monitors-bad",
    "[background]\nkind = \"image\"\nimage = \"wall.png\"\n\n[background.monitors.DP-1]\nfit = \"stretch\"\n",
  );
  let loaded = load(Some(&path));
  assert!(
    loaded
      .issues
      .iter()
      .any(|issue| issue.key == "background.monitors.DP-1.fit"),
    "issue keys were {:?}",
    loaded
      .issues
      .iter()
      .map(|issue| issue.key.as_str())
      .collect::<Vec<_>>()
  );
  // The invalid override falls back without discarding the valid base.
  assert_eq!(loaded.config.background.fit, crate::layout::Fit::Cover);
}

#[test]
fn calendar_battery_and_system_parse_their_choices() {
  let path = temp_config(
    "new-widgets",
    r##"
[calendar]
enabled = true
anchor = "bottom-left"
offset = [11, 22]
monitor = "DP-2"
rotation = 3
size = 15
width = 300
week_start = "sunday"
show_header = false
color = "#112233"
muted_color = "#445566"
today_color = "#778899"
header_color = "#aabbcc"
layer = "front"
draggable = false

[battery]
enabled = true
style = "pill"
size = 16
width = 240
color = "#010203"
low_color = "#040506"
charging_color = "#070809"
low_at = 15
show_percent = false

[system]
enabled = true
style = "sparkline"
size = 11
width = 320
fields = ["memory", "hostname", "load"]
color = "#0a0b0c"
label_color = "#0d0e0f"
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  let calendar = &loaded.config.calendar;
  assert!(calendar.enabled);
  assert_eq!(calendar.placement.anchor, Anchor::BottomLeft);
  assert_eq!(calendar.placement.offset, (11, 22));
  assert_eq!(calendar.placement.monitor.as_deref(), Some("DP-2"));
  assert_eq!(calendar.placement.rotation, 3.0);
  assert_eq!(calendar.size, 15.0);
  assert_eq!(calendar.width, 300.0);
  assert_eq!(calendar.week_start, super::WeekStart::Sunday);
  assert!(!calendar.show_header);
  assert_eq!(calendar.color, crate::color::Rgb::solid(0x11, 0x22, 0x33));
  assert_eq!(calendar.layer, super::WidgetLayer::Front);
  assert!(!calendar.draggable);

  let battery = &loaded.config.battery;
  assert!(battery.enabled);
  assert_eq!(battery.style, super::BatteryStyle::Pill);
  assert_eq!(battery.low_at, 15);
  assert!(!battery.show_percent);
  assert_eq!(
    battery.charging_color,
    crate::color::Rgb::solid(0x07, 0x08, 0x09)
  );

  let system = &loaded.config.system;
  assert!(system.enabled);
  assert_eq!(system.style, super::SystemStyle::Sparkline);
  assert_eq!(
    system.fields,
    [
      super::SystemField::Memory,
      super::SystemField::Hostname,
      super::SystemField::Load
    ]
  );
  assert_eq!(system.width, 320.0);
  assert_eq!(
    system.label_color,
    crate::color::Rgb::solid(0x0d, 0x0e, 0x0f)
  );
}

#[test]
fn bad_new_widget_values_fall_back_with_issues() {
  let path = temp_config(
    "new-widgets-bad",
    r##"
[calendar]
week_start = "tuesday"
size = 400
width = 9
layer = "middle"

[battery]
style = "saucer"
low_at = 400

[system]
fields = ["hostname", "hostname", "bogus", "memory", "kernel", "uptime", "load"]
style = "chart"
"##,
  );
  let loaded = load(Some(&path));
  assert_eq!(loaded.config.calendar.week_start, super::WeekStart::Monday);
  assert_eq!(loaded.config.calendar.size, 13.0);
  assert_eq!(loaded.config.calendar.width, 260.0);
  assert_eq!(loaded.config.calendar.layer, super::WidgetLayer::Behind);
  assert_eq!(loaded.config.battery.style, super::BatteryStyle::Ring);
  assert_eq!(loaded.config.battery.low_at, 20);
  assert_eq!(loaded.config.system.style, super::SystemStyle::Rows);
  assert_eq!(
    loaded.config.system.fields,
    [
      super::SystemField::Hostname,
      super::SystemField::Memory,
      super::SystemField::Kernel,
      super::SystemField::Uptime
    ]
  );
  for key in [
    "calendar.week_start",
    "calendar.size",
    "calendar.width",
    "calendar.layer",
    "battery.style",
    "battery.low_at",
    "system.style",
    "system.fields",
  ] {
    assert!(
      loaded.issues.iter().any(|issue| issue.key == key),
      "missing issue for {key}: {:?}",
      loaded.issues
    );
  }
}

#[test]
fn built_in_widgets_can_paint_in_front_of_decor() {
  let path = temp_config(
    "widget-layer-front",
    r##"
[clock]
layer = "front"

[media]
layer = "front"
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  assert_eq!(loaded.config.clock.layer, super::WidgetLayer::Front);
  assert_eq!(loaded.config.media.layer, super::WidgetLayer::Front);
  assert_eq!(Config::default().clock.layer, super::WidgetLayer::Behind);
  assert_eq!(Config::default().media.layer, super::WidgetLayer::Behind);
}

#[test]
fn an_unknown_widget_layer_falls_back_behind() {
  let path = temp_config(
    "widget-layer-bad",
    r##"
[clock]
layer = "middle"

[media]
layer = 3
"##,
  );
  let loaded = load(Some(&path));
  assert_eq!(loaded.config.clock.layer, super::WidgetLayer::Behind);
  assert_eq!(loaded.config.media.layer, super::WidgetLayer::Behind);
  assert!(loaded.issues.iter().any(|issue| issue.key == "clock.layer"));
  assert!(loaded.issues.iter().any(|issue| issue.key == "media.layer"));
}

#[test]
fn widget_looks_default_to_the_historical_scrim() {
  let config = Config::default();
  let look = &config.clock.look;
  assert_eq!(look.plate.style, super::PlateStyle::Tint);
  assert_eq!(look.plate.radius, 14.0);
  assert_eq!(look.plate.padding, 12.0);
  assert_eq!(look.plate.opacity, 1.0);
  assert_eq!(look.plate.gradient_angle, 0.0);
  assert_eq!(look.plate.glow_size, 0.0);
  assert!(look.plate.color.is_none());
  assert!(look.plate.color_end.is_none());
  assert!(look.plate.border.is_none());
  assert!(look.plate.glow.is_none());
  assert!(!look.plate.shadow);
  assert_eq!(look.motion, super::DecorMotion::default());
  assert!(look.show_on.is_empty());
  assert!(look.hide_on.is_empty());
  // Media gets the same defaults, so an unconfigured widget of either kind
  // looks exactly like it did before plates existed.
  assert_eq!(config.media.presentation, config.clock.look);
}

#[test]
fn clock_plate_motion_and_state_lists_parse_into_the_look() {
  let path = temp_config(
    "clock-look",
    r##"
[clock]
show_on = ["typing", "busy"]
hide_on = ["idle"]

[clock.plate]
style = "gradient"
color = "#112233"
color_end = "#445566"
gradient_angle = 45
radius = 8
border = "#ffffff"
padding = 20
glow = "#00ff00"
glow_size = 12
shadow = true
opacity = 0.75

[clock.motion]
spin = 90
float = [4, 2]
blink_ms = 1000
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  let look = &loaded.config.clock.look;
  assert_eq!(look.plate.style, super::PlateStyle::Gradient);
  assert_eq!(
    look.plate.color,
    Some(crate::color::Rgb::solid(0x11, 0x22, 0x33))
  );
  assert_eq!(
    look.plate.color_end,
    Some(crate::color::Rgb::solid(0x44, 0x55, 0x66))
  );
  assert_eq!(look.plate.gradient_angle, 45.0);
  assert_eq!(look.plate.radius, 8.0);
  assert_eq!(
    look.plate.border,
    Some(crate::color::Rgb::solid(0xff, 0xff, 0xff))
  );
  assert_eq!(look.plate.padding, 20.0);
  assert_eq!(
    look.plate.glow,
    Some(crate::color::Rgb::solid(0x00, 0xff, 0x00))
  );
  assert_eq!(look.plate.glow_size, 12.0);
  assert!(look.plate.shadow);
  assert_eq!(look.plate.opacity, 0.75);
  assert_eq!(look.motion.spin, 90.0);
  assert_eq!(look.motion.float, (4, 2));
  assert_eq!(look.motion.blink_ms, 1000.0);
  assert_eq!(
    look.show_on,
    [super::WidgetState::Typing, super::WidgetState::Busy]
  );
  assert_eq!(look.hide_on, [super::WidgetState::Idle]);
}

#[test]
fn invalid_plate_values_fall_back_with_issues() {
  let path = temp_config(
    "clock-plate-bad",
    r##"
[clock.plate]
style = "fancy"
radius = 99
padding = -1
gradient_angle = 999
opacity = 2
glow_size = 99
"##,
  );
  let loaded = load(Some(&path));
  let plate = &loaded.config.clock.look.plate;
  assert_eq!(plate.style, super::PlateStyle::Tint);
  assert_eq!(plate.radius, 14.0);
  assert_eq!(plate.padding, 12.0);
  assert_eq!(plate.gradient_angle, 0.0);
  assert_eq!(plate.opacity, 1.0);
  assert_eq!(plate.glow_size, 0.0);
  for key in [
    "clock.plate.style",
    "clock.plate.radius",
    "clock.plate.padding",
    "clock.plate.gradient_angle",
    "clock.plate.opacity",
    "clock.plate.glow_size",
  ] {
    assert!(
      loaded.issues.iter().any(|issue| issue.key == key),
      "missing issue {key}: {:?}",
      loaded.issues
    );
  }
}

#[test]
fn media_look_parses_and_bad_values_fall_back_with_issues() {
  let path = temp_config(
    "media-look",
    r##"
[media]
layout = "vinyl"
art = "disc"
progress = "wave"
controls = true
tint_from_art = true
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  assert_eq!(loaded.config.media.look.layout, super::MediaLayout::Vinyl);
  assert_eq!(loaded.config.media.look.art, super::MediaArt::Disc);
  assert_eq!(
    loaded.config.media.look.progress,
    super::MediaProgress::Wave
  );
  assert!(loaded.config.media.look.controls);
  assert!(loaded.config.media.look.tint_from_art);

  let bad = temp_config(
    "media-look-bad",
    r##"
[media]
layout = "wall"
art = "sleeve"
progress = "bars"
controls = "yes"
tint_from_art = 3
"##,
  );
  let loaded = load(Some(&bad));
  let look = loaded.config.media.look;
  assert_eq!(look.layout, super::MediaLayout::Card);
  assert_eq!(look.art, super::MediaArt::Cover);
  assert_eq!(look.progress, super::MediaProgress::Line);
  assert!(!look.controls);
  assert!(!look.tint_from_art);
  for key in [
    "media.layout",
    "media.art",
    "media.progress",
    "media.controls",
    "media.tint_from_art",
  ] {
    assert!(
      loaded.issues.iter().any(|issue| issue.key == key),
      "missing issue {key}: {:?}",
      loaded.issues
    );
  }
}

#[test]
fn scanlines_parse_and_bad_values_fall_back_with_issues() {
  let path = temp_config(
    "scanlines",
    r##"
[theme]
scanlines = 0.4
scanline_gap = 4
scanline_ms = 2500
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  assert_eq!(loaded.config.theme.scanlines, 0.4);
  assert_eq!(loaded.config.theme.scanline_gap, 4);
  assert_eq!(loaded.config.theme.scanline_ms, 2500.0);

  let bad = temp_config(
    "scanlines-bad",
    r##"
[theme]
scanlines = 4.2
scanline_gap = 1
scanline_ms = 0
"##,
  );
  let loaded = load(Some(&bad));
  // Each bad field falls back on its own, so one typo cannot turn the whole
  // overlay off or remove the other two settings.
  assert_eq!(loaded.config.theme.scanlines, 0.0);
  assert_eq!(loaded.config.theme.scanline_gap, 3);
  assert_eq!(loaded.config.theme.scanline_ms, 6000.0);
  for key in ["theme.scanlines", "theme.scanline_gap", "theme.scanline_ms"] {
    assert!(
      loaded.issues.iter().any(|issue| issue.key == key),
      "missing issue {key}: {:?}",
      loaded.issues
    );
  }
}

#[test]
fn clock_decorative_keys_parse_and_fall_back() {
  let path = temp_config(
    "clock-faces",
    r##"
[clock]
style = "segment"
ghost = true
glow = "#22D3EE"
glow_size = 8
colon_blink_ms = 900
second_hand = "sweep"
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  assert_eq!(loaded.config.clock.style, super::ClockStyle::Segment);
  assert!(loaded.config.clock.ghost);
  assert!(loaded.config.clock.glow.is_some());
  assert_eq!(loaded.config.clock.glow_size, 8.0);
  assert_eq!(loaded.config.clock.colon_blink_ms, 900.0);
  assert_eq!(loaded.config.clock.second_hand, super::SecondHand::Sweep);

  let bad = temp_config(
    "clock-faces-bad",
    r##"
[clock]
style = "sundial"
glow_size = 99
colon_blink_ms = 9000
second_hand = "wiggle"
"##,
  );
  let loaded = load(Some(&bad));
  assert_eq!(loaded.config.clock.style, super::ClockStyle::Digital);
  assert_eq!(loaded.config.clock.glow_size, 0.0);
  assert_eq!(loaded.config.clock.colon_blink_ms, 0.0);
  assert_eq!(loaded.config.clock.second_hand, super::SecondHand::Tick);
  for key in [
    "clock.style",
    "clock.glow_size",
    "clock.colon_blink_ms",
    "clock.second_hand",
  ] {
    assert!(
      loaded.issues.iter().any(|issue| issue.key == key),
      "missing issue {key}: {:?}",
      loaded.issues
    );
  }
}

#[test]
fn widget_state_lists_dedupe_and_report_unknown_names() {
  let path = temp_config(
    "clock-states",
    r##"
[clock]
show_on = ["typing", "typing", "napping"]
hide_on = ["busy", "busy"]

[media]
show_on = "typing"
"##,
  );
  let loaded = load(Some(&path));
  assert_eq!(
    loaded.config.clock.look.show_on,
    [super::WidgetState::Typing]
  );
  assert_eq!(loaded.config.clock.look.hide_on, [super::WidgetState::Busy]);
  let unknown = loaded
    .issues
    .iter()
    .filter(|issue| issue.key == "clock.show_on")
    .count();
  assert_eq!(unknown, 1, "{:?}", loaded.issues);
  assert!(
    loaded
      .issues
      .iter()
      .any(|issue| issue.key == "media.show_on")
  );
  assert!(loaded.config.media.presentation.show_on.is_empty());
}

#[test]
fn panel_decor_parses_its_plate_keys() {
  let path = temp_config(
    "panel",
    r##"
[[login.decor]]
kind = "panel"
width = 320
height = 96
color = "#112233"
color_end = "#445566"
gradient_angle = 90
radius = 6
border = "#ffffff"
opacity = 0.5
anchor = "bottom-left"
offset = [10, 20]
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  match &loaded.config.login.decor[0] {
    super::DecorSpec::Panel {
      width,
      height,
      color,
      color_end,
      gradient_angle,
      radius,
      border,
      opacity,
      anchor,
      offset,
      ..
    } => {
      assert_eq!(*width, 320.0);
      assert_eq!(*height, 96.0);
      assert_eq!(*color, Some(crate::color::Rgb::solid(0x11, 0x22, 0x33)));
      assert_eq!(*color_end, Some(crate::color::Rgb::solid(0x44, 0x55, 0x66)));
      assert_eq!(*gradient_angle, 90.0);
      assert_eq!(*radius, 6.0);
      assert_eq!(*border, Some(crate::color::Rgb::solid(0xff, 0xff, 0xff)));
      assert_eq!(*opacity, 0.5);
      assert_eq!(*anchor, Anchor::BottomLeft);
      assert_eq!(*offset, (10, 20));
    }
    other => panic!("expected a panel decor, got {other:?}"),
  }
}

#[test]
fn glass_plate_style_accepts_both_spellings() {
  assert_eq!(
    super::PlateStyle::parse("glass"),
    Some(super::PlateStyle::Glass)
  );
  assert_eq!(
    super::PlateStyle::parse("blur"),
    Some(super::PlateStyle::Glass)
  );
  // Tint is still the default, so existing configs cannot change look just
  // because the glass spelling now means the backdrop filter.
  assert_eq!(
    super::PlateStyle::parse("tint"),
    Some(super::PlateStyle::Tint)
  );
  assert_eq!(
    Config::default().clock.look.plate.style,
    super::PlateStyle::Tint
  );
}

#[test]
fn panel_glass_parses_and_resolves_to_the_theme_decor() {
  let path = temp_config(
    "panel-glass",
    r##"
[[login.decor]]
kind = "panel"
width = 200
height = 80
glass = true
color = "#10172655"

[[login.decor]]
kind = "panel"
width = 120
height = 40
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  match &loaded.config.login.decor[0] {
    super::DecorSpec::Panel { glass, .. } => assert!(*glass),
    other => panic!("expected a panel decor, got {other:?}"),
  }
  // Omitting the key keeps the old opaque plate.
  match &loaded.config.login.decor[1] {
    super::DecorSpec::Panel { glass, .. } => assert!(!*glass),
    other => panic!("expected a panel decor, got {other:?}"),
  }
  let theme = crate::ui::theme::Theme::from(&loaded.config);
  assert!(theme.decor[0].glass);
  assert!(!theme.decor[1].glass);
}

#[test]
fn out_of_range_panel_size_falls_back() {
  let path = temp_config(
    "panel-size",
    r##"
[[login.decor]]
kind = "panel"
width = 1
height = 99999
"##,
  );
  let loaded = load(Some(&path));
  match &loaded.config.login.decor[0] {
    super::DecorSpec::Panel { width, height, .. } => {
      assert_eq!(*width, 320.0);
      assert_eq!(*height, 120.0);
    }
    other => panic!("expected a panel decor, got {other:?}"),
  }
  assert!(
    loaded
      .issues
      .iter()
      .any(|issue| issue.key == "login.decor[0].width")
  );
  assert!(
    loaded
      .issues
      .iter()
      .any(|issue| issue.key == "login.decor[0].height")
  );
}

#[test]
fn a_panel_with_a_text_payload_reports_one_issue() {
  let path = temp_config(
    "panel-text",
    r##"
[[login.decor]]
kind = "panel"
width = 120
height = 40
text = "ignored"
"##,
  );
  let loaded = load(Some(&path));
  let panel_issues: Vec<_> = loaded
    .issues
    .iter()
    .filter(|issue| issue.key.starts_with("login.decor[0]"))
    .collect();
  assert_eq!(panel_issues.len(), 1, "{:?}", loaded.issues);
  assert_eq!(panel_issues[0].key, "login.decor[0].kind");
  assert!(panel_issues[0].message.contains("bare panel"));
  assert!(matches!(
    &loaded.config.login.decor[0],
    super::DecorSpec::Panel { .. }
  ));
}

#[test]
fn text_stroke_parses_and_defaults_its_width() {
  let path = temp_config(
    "text-stroke",
    r##"
[[login.decor]]
kind = "text"
text = "Outline"
stroke = "#000000"

[[login.decor]]
kind = "text"
text = "No outline"
stroke_width = 99
"##,
  );
  let loaded = load(Some(&path));
  match &loaded.config.login.decor[0] {
    super::DecorSpec::Text {
      stroke,
      stroke_width,
      ..
    } => {
      assert_eq!(*stroke, Some(crate::color::Rgb::solid(0, 0, 0)));
      assert_eq!(*stroke_width, 2.0);
    }
    other => panic!("expected text decor, got {other:?}"),
  }
  match &loaded.config.login.decor[1] {
    super::DecorSpec::Text {
      stroke,
      stroke_width,
      ..
    } => {
      assert_eq!(*stroke, None);
      assert_eq!(*stroke_width, 0.0);
    }
    other => panic!("expected text decor, got {other:?}"),
  }
  assert!(
    loaded
      .issues
      .iter()
      .any(|issue| issue.key == "login.decor[1].stroke_width")
  );
}

#[test]
fn gpu_section_parses_and_defaults() {
  let defaults = super::GpuConfig::default();
  assert_eq!(Config::default().gpu, defaults);
  assert!(defaults.enabled);
  assert_eq!(defaults.backend, super::GpuBackend::Auto);
  assert_eq!(defaults.adapter, "auto");
  assert_eq!(defaults.device, super::GpuDevice::Auto);
  assert_eq!(defaults.power, super::GpuPower::Low);
  assert_eq!(defaults.present, super::GpuPresent::Fifo);
  assert_eq!(defaults.fallback, super::GpuFallback::Chain);

  let path = temp_config(
    "gpu-section",
    r##"
[gpu]
enabled = false
backend = "vulkan"
adapter = "index:2"
device = "discrete"
power = "high"
present = "mailbox"
fallback = "software"
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  assert_eq!(
    loaded.config.gpu,
    super::GpuConfig {
      enabled: false,
      backend: super::GpuBackend::Vulkan,
      adapter: "index:2".to_string(),
      device: super::GpuDevice::Discrete,
      power: super::GpuPower::High,
      present: super::GpuPresent::Mailbox,
      fallback: super::GpuFallback::Software,
    }
  );
}

#[test]
fn invalid_gpu_choices_fall_back_with_issues() {
  let path = temp_config(
    "gpu-bad-choices",
    r##"
[gpu]
backend = "metal"
device = "toaster"
power = "medium"
present = "vsync-free"
fallback = "pray"
"##,
  );
  let loaded = load(Some(&path));
  let gpu = &loaded.config.gpu;
  assert_eq!(gpu.backend, super::GpuBackend::Auto);
  assert_eq!(gpu.device, super::GpuDevice::Auto);
  assert_eq!(gpu.power, super::GpuPower::Low);
  assert_eq!(gpu.present, super::GpuPresent::Fifo);
  assert_eq!(gpu.fallback, super::GpuFallback::Chain);
  for key in [
    "gpu.backend",
    "gpu.device",
    "gpu.power",
    "gpu.present",
    "gpu.fallback",
  ] {
    assert!(
      loaded.issues.iter().any(|issue| issue.key == key),
      "missing issue {key}: {:?}",
      loaded.issues
    );
  }
}

#[test]
fn gpu_adapter_index_and_name_rules() {
  let index = load(Some(&temp_config(
    "gpu-adapter-index",
    r#"
[gpu]
adapter = "index:2"
"#,
  )));
  assert!(index.issues.is_empty(), "{:?}", index.issues);
  assert_eq!(index.config.gpu.adapter, "index:2");

  let bad = load(Some(&temp_config(
    "gpu-adapter-bad-index",
    r#"
[gpu]
adapter = "index:x"
"#,
  )));
  assert_eq!(bad.config.gpu.adapter, "auto");
  assert!(
    bad.issues.iter().any(|issue| issue.key == "gpu.adapter"),
    "{:?}",
    bad.issues
  );

  let name = load(Some(&temp_config(
    "gpu-adapter-name",
    r#"
[gpu]
adapter = "RTX"
"#,
  )));
  assert!(name.issues.is_empty(), "{:?}", name.issues);
  assert_eq!(name.config.gpu.adapter, "RTX");
}

#[test]
fn gpu_env_override_is_pure() {
  let mut gpu = super::GpuConfig::default();
  gpu.apply_env_value(None);
  assert_eq!(gpu, super::GpuConfig::default());

  gpu.apply_env_value(Some("0"));
  assert!(!gpu.enabled);

  let mut gpu = super::GpuConfig::default();
  gpu.apply_env_value(Some("vulkan"));
  assert_eq!(gpu.backend, super::GpuBackend::Vulkan);

  let mut gpu = super::GpuConfig::default();
  gpu.apply_env_value(Some("software"));
  assert_eq!(gpu.backend, super::GpuBackend::Software);
  assert_eq!(gpu.device, super::GpuDevice::Cpu);
  assert_eq!(gpu.fallback, super::GpuFallback::Software);

  let mut gpu = super::GpuConfig::default();
  gpu.apply_env_value(Some("NVIDIA"));
  assert_eq!(gpu.adapter, "NVIDIA");

  gpu.apply_env_value(Some("auto"));
  assert_eq!(gpu.adapter, "auto");
}

#[test]
fn transition_particles_and_ticker_round_trip() {
  let path = temp_config(
    "waves-round-trip",
    r##"
[login.transition]
enabled = true
trigger = "success"
cuts = ["#ffffff", "#d41830", "#0e0b10"]
cut_ms = 40
shake = 8
zoom = 0.1
slam = "WELCOME TO THE PARLOR"
slam_ms = 500
slam_color = "#f3e7d2"
slam_size = 72

[particles]
enabled = true
count = 12
sprite = "butterfly"
color = "#ff7a3c"
color2 = "#d41830"
drift = "float"
speed = 10.5
size = 4
opacity = 0.4
layer = "front"

[ticker]
enabled = true
items = ["ONE", "TWO", "THREE"]
separator = " | "
anchor = "top"
offset = [12, 8]
size = 24
speed = 30
color = "#d41830"
bold = false
tracking = 0.1
layer = "front"

[ticker.plate]
style = "glass"
color = "#0e0b1066"
radius = 2
padding = 6
"##,
  );
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);

  let transition = &loaded.config.login.transition;
  assert!(transition.enabled);
  assert_eq!(transition.trigger, super::TransitionTrigger::Success);
  assert_eq!(transition.cuts.len(), 3);
  assert_eq!(transition.cut_ms, 40.0);
  assert_eq!(transition.shake, 8.0);
  assert_eq!(transition.zoom, 0.1);
  assert_eq!(transition.slam, "WELCOME TO THE PARLOR");
  assert_eq!(transition.slam_ms, 500.0);
  assert_eq!(transition.slam_size, 72.0);

  let particles = &loaded.config.particles;
  assert!(particles.enabled);
  assert_eq!(particles.count, 12);
  assert_eq!(particles.sprite, super::ParticleSprite::Butterfly);
  assert_eq!(particles.drift, super::ParticleDrift::Float);
  assert_eq!(particles.layer, super::ParticleLayer::Front);
  assert_eq!(particles.speed, 10.5);
  assert_eq!(particles.size, 4.0);
  assert_eq!(particles.opacity, 0.4);

  let ticker = &loaded.config.ticker;
  assert!(ticker.enabled);
  assert_eq!(ticker.items, vec!["ONE", "TWO", "THREE"]);
  assert_eq!(ticker.separator, " | ");
  assert_eq!(ticker.anchor, super::TickerAnchor::Top);
  assert_eq!(ticker.offset, (12, 8));
  assert_eq!(ticker.size, 24.0);
  assert_eq!(ticker.speed, 30.0);
  assert!(!ticker.bold);
  assert_eq!(ticker.tracking, 0.1);
  assert_eq!(ticker.layer, super::WidgetLayer::Front);
  assert_eq!(ticker.plate.style, super::PlateStyle::Glass);
  assert_eq!(
    ticker.plate.color,
    Some(crate::color::Rgb::rgba(0x0e, 0x0b, 0x10, 0x66))
  );
  assert_eq!(ticker.plate.radius, 2.0);
  assert_eq!(ticker.plate.padding, 6.0);
  assert_eq!(ticker.run(), "ONE | TWO | THREE | ");
}

#[test]
fn bad_transition_particles_and_ticker_values_fall_back() {
  let path = temp_config(
    "waves-bad",
    r##"
[login.transition]
enabled = true
trigger = "later"
cuts = ["not-a-colour"]
cut_ms = 5000
shake = 999
zoom = 2
slam_size = 999
slam_ms = 99999

[particles]
enabled = true
count = 500
sprite = "dragon"
drift = "sideways"
layer = "over"
speed = 999
size = 0
opacity = 2

[ticker]
enabled = true
items = ["OK", 7, ""]
separator = " | "
anchor = "middle"
offset = [4, 5]
size = 999
speed = -1
tracking = 4
layer = "over"
"##,
  );
  let loaded = load(Some(&path));
  let transition = &loaded.config.login.transition;
  assert!(transition.enabled);
  assert_eq!(transition.trigger, super::TransitionTrigger::Enter);
  assert_eq!(transition.cuts, super::TransitionConfig::default().cuts);
  assert_eq!(transition.cut_ms, 55.0);
  assert_eq!(transition.shake, 14.0);
  assert_eq!(transition.zoom, 0.06);
  assert_eq!(transition.slam_size, 92.0);
  assert_eq!(transition.slam_ms, 900.0);

  let particles = &loaded.config.particles;
  assert_eq!(particles.count, 48);
  assert_eq!(particles.sprite, super::ParticleSprite::Flame);
  assert_eq!(particles.drift, super::ParticleDrift::Up);
  assert_eq!(particles.layer, super::ParticleLayer::Behind);
  assert_eq!(particles.speed, 24.0);
  assert_eq!(particles.size, 6.0);
  assert_eq!(particles.opacity, 0.55);

  let ticker = &loaded.config.ticker;
  assert_eq!(ticker.items, vec!["OK"]);
  assert_eq!(ticker.anchor, super::TickerAnchor::Bottom);
  assert_eq!(ticker.offset, (4, 5));
  assert_eq!(ticker.size, 20.0);
  assert_eq!(ticker.speed, 60.0);
  assert_eq!(ticker.tracking, 0.2);
  assert_eq!(ticker.layer, super::WidgetLayer::Behind);

  for key in [
    "login.transition.trigger",
    "login.transition.cuts",
    "login.transition.cut_ms",
    "login.transition.shake",
    "login.transition.zoom",
    "login.transition.slam_size",
    "login.transition.slam_ms",
    "particles.count",
    "particles.sprite",
    "particles.drift",
    "particles.layer",
    "particles.speed",
    "particles.size",
    "particles.opacity",
    "ticker.items",
    "ticker.anchor",
    "ticker.size",
    "ticker.speed",
    "ticker.tracking",
    "ticker.layer",
  ] {
    assert!(
      loaded.issues.iter().any(|issue| issue.key == key),
      "missing issue {key}: {:?}",
      loaded.issues
    );
  }
}

#[test]
fn the_new_tables_default_to_off() {
  let loaded = load(Some(&PathBuf::from("/nonexistent/dawn/config.toml")));
  assert!(!loaded.config.login.transition.enabled);
  assert!(!loaded.config.particles.enabled);
  assert!(!loaded.config.ticker.enabled);
  assert_eq!(
    loaded.config.login.transition.trigger,
    super::TransitionTrigger::Enter
  );
}

#[test]
fn audio_and_wallpaper_paths_are_resolved_once_against_the_config() {
  use crate::audio::Event as AudioEvent;

  let dir = std::env::temp_dir().join(format!(
    "dawn-audio-paths-{}-{}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|duration| duration.as_nanos())
      .unwrap_or_default()
  ));
  let assets = dir.join("assets");
  std::fs::create_dir_all(&assets).expect("assets dir");
  std::fs::write(assets.join("key.wav"), b"RIFF").expect("key file");
  std::fs::write(dir.join("wallpaper.mp4"), b"not really an mp4").expect("video file");
  let path = dir.join("config.toml");
  std::fs::write(
    &path,
    r#"
[background]
kind = "video"
image = "wallpaper.mp4"
audio = true

[audio.events.key]
file = "assets/key.wav"
"#,
  )
  .expect("config");
  let loaded = load(Some(&path));
  assert!(loaded.issues.is_empty(), "{:?}", loaded.issues);
  let image = dir.join("wallpaper.mp4");
  assert_eq!(
    loaded.config.background.image.as_deref(),
    Some(image.as_path())
  );
  assert_eq!(
    loaded
      .config
      .audio
      .events
      .get(&AudioEvent::Key)
      .map(|sound| sound.file.as_path()),
    Some(assets.join("key.wav").as_path())
  );
  assert_eq!(
    loaded.config.audio.wallpaper_audio.as_deref(),
    Some(image.as_path()),
    "the wallpaper track must not have the theme root prefixed twice"
  );
  std::fs::remove_dir_all(dir).expect("cleanup");
}
