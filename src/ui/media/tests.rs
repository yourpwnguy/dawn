use super::{
  MediaControl, MediaSnapshot, MediaState, art_path, circular_mask, clean_text, dominant_tint,
  percent_decode, when_no_player,
};

#[test]
fn visible_equality_ignores_art_pixels() {
  let left = MediaSnapshot::default();
  let mut right = MediaSnapshot::default();
  assert!(left.visible_eq(&right));
  right.title = "Song".to_string();
  assert!(!left.visible_eq(&right));
}

#[test]
fn state_generation_only_moves_on_visible_change() {
  let state = MediaState::new();
  assert_eq!(state.generation(), 0);
  assert!(!state.update(MediaSnapshot::default()));
  let next = MediaSnapshot {
    title: "Song".to_string(),
    ..MediaSnapshot::default()
  };
  assert!(state.update(next));
  assert_eq!(state.generation(), 1);
}

#[test]
fn file_uris_are_decoded_and_http_is_ignored() {
  assert_eq!(
    art_path("file:///tmp/a%20b.png"),
    Some(std::path::PathBuf::from("/tmp/a b.png"))
  );
  assert_eq!(art_path("https://example.invalid/a.png"), None);
  assert_eq!(art_path("file://localhostrelative.png"), None);
  assert_eq!(percent_decode("a%20b"), "a b");
  assert_eq!(
    clean_text(" a\u{1b}[31m  title \n".to_string()),
    "a[31m title"
  );
}

#[test]
fn the_last_track_survives_a_disappearing_player() {
  let current = MediaSnapshot {
    available: true,
    title: "Song".to_string(),
    artist: "Artist".to_string(),
    playing: true,
    ..MediaSnapshot::default()
  };
  let next = when_no_player(current.clone(), true);
  assert!(next.available);
  assert_eq!(next.title, "Song");
  assert!(!next.playing);
  assert!(next.stale);

  let next = when_no_player(current, false);
  assert!(!next.available);
  assert!(!next.stale);
}

#[test]
fn controls_are_sendable() {
  let (sender, receiver) = std::sync::mpsc::channel();
  sender.send(MediaControl::PlayPause).unwrap();
  assert_eq!(receiver.recv().unwrap(), MediaControl::PlayPause);
}

#[test]
fn dominant_colour_ignores_transparency_and_keeps_the_hue() {
  let mut pixmap = tiny_skia::Pixmap::new(8, 8).unwrap();
  pixmap.fill(tiny_skia::Color::from_rgba8(200, 40, 40, 255));
  let tint = dominant_tint(&pixmap).expect("an opaque pixmap has a dominant colour");
  assert_eq!(tint.a, 255);
  assert!(tint.r > tint.g && tint.r > tint.b, "red should stay red");

  let empty = tiny_skia::Pixmap::new(8, 8).unwrap();
  assert_eq!(dominant_tint(&empty), None);
}

#[test]
fn the_disc_mask_clears_the_corners_but_keeps_the_centre() {
  let mut pixmap = tiny_skia::Pixmap::new(8, 8).unwrap();
  pixmap.fill(tiny_skia::Color::from_rgba8(200, 40, 40, 255));
  let masked = circular_mask(&pixmap).expect("mask should allocate");
  let corner = masked.pixel(0, 0).unwrap().alpha();
  let center = masked.pixel(4, 4).unwrap().alpha();
  assert_eq!(corner, 0);
  assert_eq!(center, 255);
}
