use super::{Drag, HitRegion, Offset};
use crate::ui::canvas::Rect;
use std::time::{Duration, Instant};

fn region(id: &str, rect: Rect, draggable: bool) -> HitRegion {
  HitRegion::new(id, rect, draggable)
}

#[test]
fn hit_testing_prefers_the_topmost_region() {
  let regions = vec![
    region("card", Rect::new(0.0, 0.0, 100.0, 100.0), true),
    region("field", Rect::new(10.0, 10.0, 30.0, 20.0), false),
  ];
  let mut drag = Drag::empty();
  // The field wins, so a click on the password box does not drag the card.
  assert!(!drag.begin(&regions, (15.0, 15.0), 1.0));
  assert!(drag.begin(&regions, (60.0, 60.0), 1.0));
  assert!(drag.is_dragging());
}

#[test]
fn dragging_moves_the_target_and_clamps_to_the_screen() {
  let regions = vec![region("clock", Rect::new(80.0, 80.0, 20.0, 20.0), true)];
  let mut drag = Drag::empty();
  assert!(drag.begin(&regions, (90.0, 90.0), 1.0));
  assert!(drag.move_to((210.0, 90.0), (400.0, 400.0)));
  let saved = drag.saved_offsets();
  assert_eq!(saved["clock"], [120.0, 0.0]);
  // Push far past the right edge; the target stops at 300, which puts the
  // widget's right edge on the 400px screen.
  assert!(drag.move_to((5000.0, 90.0), (400.0, 400.0)));
  let saved = drag.saved_offsets();
  assert_eq!(saved["clock"], [300.0, 0.0]);
}

#[test]
fn a_held_drag_only_animates_while_the_offset_moves() {
  let regions = vec![region("clock", Rect::new(80.0, 80.0, 20.0, 20.0), true)];
  let mut drag = Drag::new(None, Duration::from_millis(120));
  assert!(drag.begin(&regions, (90.0, 90.0), 1.0));
  // Grab without moving: nothing on screen changes, so no frame is needed.
  assert!(!drag.is_animating());
  assert!(drag.move_to((140.0, 90.0), (400.0, 400.0)));
  assert!(drag.is_animating());
  let now = drag.last_frame + Duration::from_secs(2);
  assert!(drag.advance(now, false));
  assert!(!drag.is_animating());
}

#[test]
fn a_card_space_widget_drag_matches_the_pointer() {
  // A front widget stores card_scale in its hit region. The pointer moved
  // 30 screen pixels, so the logical offset must grow by 30 / 0.5 = 60.
  // Without that, the widget would trail the pointer on a shrunken card.
  let regions = vec![region("clock", Rect::new(80.0, 80.0, 20.0, 20.0), true).with_drag_scale(0.5)];
  let mut drag = Drag::empty();
  assert!(drag.begin(&regions, (90.0, 90.0), 1.0));
  assert!(drag.move_to((120.0, 90.0), (400.0, 400.0)));
  assert_eq!(drag.saved_offsets()["clock"], [60.0, 0.0]);
}

#[test]
fn offsets_ease_toward_the_target() {
  let mut drag = Drag::new(None, Duration::from_millis(120));
  drag.offsets.insert(
    "clock".to_string(),
    Offset {
      current: (0.0, 0.0),
      target: (100.0, 0.0),
    },
  );
  let start = Instant::now();
  drag.last_frame = start;
  assert!(drag.advance(start + Duration::from_millis(50), false));
  let x = drag.offset("clock").0;
  assert!(x > 0.0 && x < 100.0, "x was {x}");
  assert!(drag.advance(start + Duration::from_secs(2), false));
  assert_eq!(drag.offset("clock"), (100.0, 0.0));
}

#[test]
fn rotated_regions_hit_test_in_their_own_frame() {
  // A 100x20 bar rotated 90 degrees becomes a 20x100 bar around the same
  // centre. A point 40 px above centre is inside after rotation even though
  // it is outside the unrotated rectangle.
  let region = HitRegion::rotated("field", Rect::new(100.0, 100.0, 100.0, 20.0), false, 90.0);
  assert!(region.contains((150.0, 70.0)));
  assert!(!region.contains((190.0, 110.0)));
  let moved = region.rotated_around((0.0, 0.0), 90.0);
  assert!(moved.rotation > 179.0);
}

#[test]
fn hover_tracks_non_draggable_regions_for_button_effects() {
  let regions = vec![region(
    "login.button",
    Rect::new(10.0, 10.0, 40.0, 20.0),
    false,
  )];
  let mut drag = Drag::new(None, Duration::from_millis(100));
  assert!(drag.hover(&regions, Some((20.0, 20.0))));
  assert!(drag.is_hovered("login.button"));
  assert!(!drag.hover_is_draggable());
  assert!(drag.hover_elapsed("login.button", Instant::now()) <= Duration::from_millis(1));
  assert_eq!(
    drag.hover_t("login.button", Instant::now(), Duration::ZERO),
    1.0
  );
  assert!(drag.hover(&regions, None));
  assert!(!drag.is_hovered("login.button"));
  assert!(drag.hover_t("login.button", Instant::now(), Duration::from_millis(100)) < 1.0);
}

#[test]
fn saved_offsets_round_trip() {
  let dir = std::env::temp_dir().join(format!("dawn-drag-{}", std::process::id()));
  std::fs::create_dir_all(&dir).unwrap();
  let path = dir.join("layout.toml");
  let mut drag = Drag::new(Some(path.clone()), Duration::from_millis(120));
  drag
    .offsets
    .insert("media".to_string(), Offset::at((12.5, -4.0)));
  drag.save();
  let loaded = Drag::new(Some(path), Duration::from_millis(120));
  assert_eq!(loaded.offset("media"), (12.5, -4.0));
  let _ = std::fs::remove_dir_all(&dir);
}
