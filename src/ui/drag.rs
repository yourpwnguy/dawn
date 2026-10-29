//! Pointer dragging for the login card and the first-party widgets.
//!
//! The renderer already knows where every widget ended up, so it hands back a
//! [`HitRegion`] for each one every frame. This module keeps the small amount
//! of state needed to turn those regions into a drag: which widget is under the
//! pointer, the offset it had when the drag started, and a target offset that
//! the current offset eases toward.
//!
//! Offsets live in logical pixels, which is also what the config uses. The
//! renderer multiplies by the output scale, and pointer coordinates are divided
//! by the same scale before they reach this module. That keeps a saved layout
//! usable after a monitor change.
//!
//! Nothing here can fail the greeter: a missing or corrupt layout file is just
//! an empty layout, and a failed save logs a warning. Dragging is decoration on
//! top of the login form, never a dependency of it.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::ui::canvas::Rect;

const MAX_LAYOUT_BYTES: u64 = 64 * 1024;
const MAX_ENTRIES: usize = 256;
const MAX_OFFSET: f32 = 8192.0;
const SETTLE_EPSILON: f32 = 0.25;
const MAX_ID_CHARS: usize = 128;

/// One drawable region from the last frame.
#[derive(Debug, Clone, PartialEq)]
pub struct HitRegion {
  pub id: String,
  pub rect: Rect,
  /// A region can exist purely to stop a click from falling through to the
  /// card behind it. The username and password fields work that way.
  pub draggable: bool,
  /// Clockwise rotation around the rectangle's centre, in degrees.
  pub rotation: f32,
  /// Scale that turns this region's saved offset into pixels. Screen-space
  /// regions use the window scale, so `None` keeps the historical behaviour.
  /// A front widget lives in card space and stores `card_scale` here; without
  /// it a drag would move slower than the pointer on a shrunken hero card.
  pub drag_scale: Option<f32>,
}

impl HitRegion {
  #[must_use]
  pub fn new(id: impl Into<String>, rect: Rect, draggable: bool) -> Self {
    Self {
      id: id.into(),
      rect,
      draggable,
      rotation: 0.0,
      drag_scale: None,
    }
  }

  #[must_use]
  pub fn rotated(id: impl Into<String>, rect: Rect, draggable: bool, rotation: f32) -> Self {
    Self {
      id: id.into(),
      rect,
      draggable,
      rotation,
      drag_scale: None,
    }
  }

  /// Records a different offset-to-pixel scale for this region. See
  /// [`Self::drag_scale`].
  #[must_use]
  pub fn with_drag_scale(mut self, scale: f32) -> Self {
    self.drag_scale = Some(scale);
    self
  }

  /// Moves the region's centre around another pivot and adds the pivot's
  /// rotation. Nested rotations (a field inside a rotated card) compose this
  /// way, so hit testing still lands on the visible widget.
  #[must_use]
  pub fn rotated_around(mut self, pivot: (f32, f32), angle: f32) -> Self {
    let center = self.rect.center();
    let (x, y) = rotate_point(center, pivot, angle);
    self.rect.x = x - self.rect.w / 2.0;
    self.rect.y = y - self.rect.h / 2.0;
    self.rotation += angle;
    self
  }

  /// True when the pointer is inside the rotated rectangle.
  #[must_use]
  pub fn contains(&self, point: (f32, f32)) -> bool {
    if self.rotation.abs() <= 0.01 {
      return self.rect.contains(point);
    }
    let local = rotate_point(point, self.rect.center(), -self.rotation);
    self.rect.contains(local)
  }
}

fn rotate_point(point: (f32, f32), pivot: (f32, f32), angle: f32) -> (f32, f32) {
  if angle.abs() <= 0.01 {
    return point;
  }
  let (sin, cos) = angle.to_radians().sin_cos();
  let dx = point.0 - pivot.0;
  let dy = point.1 - pivot.1;
  (pivot.0 + dx * cos - dy * sin, pivot.1 + dx * sin + dy * cos)
}

#[derive(Debug, Clone, Copy, Default)]
struct Offset {
  current: (f32, f32),
  target: (f32, f32),
}

impl Offset {
  fn at(value: (f32, f32)) -> Self {
    Self {
      current: value,
      target: value,
    }
  }

  fn settled(&self) -> bool {
    (self.current.0 - self.target.0).abs() < SETTLE_EPSILON
      && (self.current.1 - self.target.1).abs() < SETTLE_EPSILON
  }
}

struct ActiveDrag {
  id: String,
  /// Region at the moment the drag started, in physical pixels.
  start_rect: Rect,
  start_offset: (f32, f32),
  last_cursor: (f32, f32),
  scale: f32,
  moved: bool,
}

/// The layout file on disk. Kept deliberately tiny and versioned so a future
/// schema change can ignore old files instead of misplacing the login form.
#[derive(Debug, Default, Serialize, Deserialize)]
struct LayoutFile {
  #[serde(default)]
  version: u32,
  #[serde(default)]
  offsets: BTreeMap<String, [f32; 2]>,
}

/// Drag state for one window.
pub struct Drag {
  offsets: BTreeMap<String, Offset>,
  active: Option<ActiveDrag>,
  hover: Option<String>,
  hover_draggable: bool,
  hover_since: Instant,
  previous_hover: Option<String>,
  hover_ended: Option<Instant>,
  save_path: Option<PathBuf>,
  smoothing: Duration,
  last_frame: Instant,
}

impl Drag {
  /// Loads saved positions. A missing file is normal; a broken file is logged
  /// and ignored, because a bad decoration must not stop a login.
  #[must_use]
  pub fn new(save_path: Option<PathBuf>, smoothing: Duration) -> Self {
    let offsets = save_path
      .as_deref()
      .and_then(load_offsets)
      .unwrap_or_default();
    Self {
      offsets,
      active: None,
      hover: None,
      hover_draggable: false,
      hover_since: Instant::now(),
      previous_hover: None,
      hover_ended: None,
      save_path,
      smoothing,
      last_frame: Instant::now(),
    }
  }

  /// A layout for screenshots and tests. No file access, no dragging.
  #[must_use]
  pub fn empty() -> Self {
    Self::new(None, Duration::ZERO)
  }

  /// The logical offset for a widget. Missing ids sit at their configured
  /// anchor, which is exactly what an un-dragged widget should do.
  #[must_use]
  pub fn offset(&self, id: &str) -> (f32, f32) {
    self.offsets.get(id).map_or((0.0, 0.0), |o| o.current)
  }

  #[must_use]
  pub fn is_dragging(&self) -> bool {
    self.active.is_some()
  }

  #[must_use]
  pub fn active_id(&self) -> Option<&str> {
    self.active.as_ref().map(|active| active.id.as_str())
  }

  #[must_use]
  pub fn is_active(&self, id: &str) -> bool {
    self.active_id() == Some(id)
  }

  #[must_use]
  pub fn is_hovered(&self, id: &str) -> bool {
    self.hover.as_deref() == Some(id)
  }

  /// True while the pointer should show a grab cursor.
  #[must_use]
  pub fn hover_is_draggable(&self) -> bool {
    self.hover.is_some() && self.hover_draggable
  }

  /// How long the pointer has been on `id`. Zero when it is elsewhere.
  #[must_use]
  pub fn hover_elapsed(&self, id: &str, now: Instant) -> Duration {
    if self.is_hovered(id) {
      now.saturating_duration_since(self.hover_since)
    } else {
      Duration::ZERO
    }
  }

  /// A 0..1 hover transition for `id`. It eases in while hovered and back out
  /// after the pointer leaves, which is what stops a button from snapping.
  #[must_use]
  pub fn hover_t(&self, id: &str, now: Instant, duration: Duration) -> f32 {
    if duration.is_zero() {
      return if self.is_hovered(id) { 1.0 } else { 0.0 };
    }
    if self.is_hovered(id) {
      crate::ui::anim::progress(now, self.hover_since, duration)
    } else if self.previous_hover.as_deref() == Some(id) {
      let ended = self.hover_ended.unwrap_or(now);
      1.0 - crate::ui::anim::progress(now, ended, duration)
    } else {
      0.0
    }
  }

  /// True while the hover colour/geometry transition is still running. A
  /// looping effect is a separate decision, because it wants frames for as long
  /// as the pointer stays on the widget.
  #[must_use]
  pub fn hover_transition_active(&self, id: &str, now: Instant, duration: Duration) -> bool {
    if duration.is_zero() {
      return false;
    }
    if self.is_hovered(id) {
      return now.saturating_duration_since(self.hover_since) < duration;
    }
    if self.previous_hover.as_deref() == Some(id)
      && let Some(ended) = self.hover_ended
    {
      return now.saturating_duration_since(ended) < duration;
    }
    false
  }

  /// Starts a drag if the topmost region under the pointer is draggable.
  ///
  /// Regions are tested in reverse paint order, which is why a non-draggable
  /// password field can sit on top of a draggable card without the card
  /// swallowing the click.
  pub fn begin(&mut self, regions: &[HitRegion], cursor: (f32, f32), scale: f32) -> bool {
    let Some(region) = regions.iter().rev().find(|region| region.contains(cursor)) else {
      self.active = None;
      return false;
    };
    if !region.draggable {
      self.active = None;
      return false;
    }
    let start_offset = self.offset(&region.id);
    self
      .offsets
      .entry(region.id.clone())
      .or_insert_with(|| Offset::at(start_offset));
    self.active = Some(ActiveDrag {
      id: region.id.clone(),
      start_rect: region.rect,
      start_offset,
      last_cursor: cursor,
      scale: region.drag_scale.unwrap_or(scale).max(0.25),
      moved: false,
    });
    true
  }

  /// Moves the active widget by the pointer delta. Returns true when the target
  /// changed.
  pub fn move_to(&mut self, cursor: (f32, f32), screen: (f32, f32)) -> bool {
    let Some(active) = self.active.as_mut() else {
      return false;
    };
    let delta = (
      (cursor.0 - active.last_cursor.0) / active.scale,
      (cursor.1 - active.last_cursor.1) / active.scale,
    );
    active.last_cursor = cursor;
    if delta.0 == 0.0 && delta.1 == 0.0 {
      return false;
    }

    let Some(offset) = self.offsets.get_mut(&active.id) else {
      return false;
    };
    let mut target = (offset.target.0 + delta.0, offset.target.1 + delta.1);
    // Keep the widget on screen. The start rect already includes the current
    // offset, so shifting it by the change in target is the visual rect.
    let width = active.start_rect.w.max(0.0);
    let height = active.start_rect.h.max(0.0);
    let min_x = active.start_offset.0 - active.start_rect.x / active.scale;
    let max_x = active.start_offset.0 + (screen.0 - width - active.start_rect.x) / active.scale;
    let min_y = active.start_offset.1 - active.start_rect.y / active.scale;
    let max_y = active.start_offset.1 + (screen.1 - height - active.start_rect.y) / active.scale;
    let clamp = |value: f32, min: f32, max: f32| {
      if min > max {
        0.0
      } else {
        value.clamp(min, max)
      }
    };
    target.0 = clamp(target.0, min_x, max_x);
    target.1 = clamp(target.1, min_y, max_y);

    let changed = (offset.target.0 - target.0).abs() > f32::EPSILON
      || (offset.target.1 - target.1).abs() > f32::EPSILON;
    if changed {
      active.moved = true;
    }
    offset.target = target;
    changed
  }

  /// Ends the active drag and returns true when the widget actually moved. A
  /// plain click should not write a layout file.
  pub fn end(&mut self) -> bool {
    self.active.take().is_some_and(|active| active.moved)
  }

  /// Updates which widget is under the pointer. Returns true when it changed.
  pub fn hover(&mut self, regions: &[HitRegion], cursor: Option<(f32, f32)>) -> bool {
    let next = cursor.and_then(|point| {
      regions
        .iter()
        .rev()
        .find(|region| region.contains(point))
        .map(|region| (region.id.clone(), region.draggable))
    });
    let (next_id, next_draggable) = match next {
      Some((id, draggable)) => (Some(id), draggable),
      None => (None, false),
    };
    if next_id == self.hover {
      self.hover_draggable = next_draggable;
      return false;
    }
    self.previous_hover = self.hover.take();
    self.hover_ended = Some(Instant::now());
    self.hover = next_id;
    self.hover_draggable = next_draggable;
    self.hover_since = Instant::now();
    true
  }

  /// Eases every current offset toward its target. Call once per painted frame,
  /// before asking for offsets.
  pub fn advance(&mut self, now: Instant, reduced: bool) -> bool {
    let dt = now.saturating_duration_since(self.last_frame);
    self.last_frame = now;
    if dt.is_zero() {
      return false;
    }
    let mut changed = false;
    for offset in self.offsets.values_mut() {
      if offset.settled() {
        offset.current = offset.target;
        continue;
      }
      if reduced || self.smoothing.is_zero() {
        offset.current = offset.target;
        changed = true;
        continue;
      }
      // Exponential easing is frame-rate independent: the fraction of the
      // remaining distance covered depends on elapsed time, not on how often
      // the compositor happens to wake us.
      let fraction = 1.0 - (-dt.as_secs_f32() / self.smoothing.as_secs_f32()).exp();
      offset.current.0 += (offset.target.0 - offset.current.0) * fraction;
      offset.current.1 += (offset.target.1 - offset.current.1) * fraction;
      if offset.settled() {
        offset.current = offset.target;
      }
      changed = true;
    }
    changed
  }

  /// True while a frame would visibly move. The window uses this to schedule
  /// another repaint instead of polling forever.
  ///
  /// An active drag alone is not enough: holding the pointer still on a
  /// grabbed widget should not repaint at 60 fps. Pointer moves already
  /// request their own redraw, so only a target that still differs from the
  /// painted offset needs a frame.
  #[must_use]
  pub fn is_animating(&self) -> bool {
    self.offsets.values().any(|offset| !offset.settled())
  }

  /// Writes the current targets to disk. Failure is a warning, never an error.
  pub fn save(&self) {
    let Some(path) = &self.save_path else {
      return;
    };
    if let Err(error) = save_offsets(path, &self.offsets) {
      warn!(path = %path.display(), %error, "cannot save the widget layout");
    }
  }

  /// Test and debug helper: the offsets that would be written to disk.
  #[must_use]
  pub fn saved_offsets(&self) -> BTreeMap<String, [f32; 2]> {
    self
      .offsets
      .iter()
      .map(|(id, offset)| (id.clone(), [offset.target.0, offset.target.1]))
      .collect()
  }
}

fn load_offsets(path: &Path) -> Option<BTreeMap<String, Offset>> {
  let metadata = match std::fs::metadata(path) {
    Ok(metadata) => metadata,
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
    Err(error) => {
      debug!(path = %path.display(), %error, "cannot read the widget layout");
      return None;
    }
  };
  if !metadata.is_file() || metadata.len() > MAX_LAYOUT_BYTES {
    warn!(path = %path.display(), "ignoring the widget layout: not a small regular file");
    return None;
  }
  let text = match std::fs::read_to_string(path) {
    Ok(text) => text,
    Err(error) => {
      warn!(path = %path.display(), %error, "cannot read the widget layout");
      return None;
    }
  };
  let file: LayoutFile = match toml::from_str(&text) {
    Ok(file) => file,
    Err(error) => {
      warn!(path = %path.display(), %error, "ignoring a corrupt widget layout");
      return None;
    }
  };
  let mut offsets = BTreeMap::new();
  for (id, value) in file.offsets.into_iter().take(MAX_ENTRIES) {
    if id.is_empty() || id.chars().count() > MAX_ID_CHARS || id.chars().any(char::is_control) {
      warn!(path = %path.display(), "ignoring a layout entry with an invalid id");
      continue;
    }
    let (x, y) = (value[0], value[1]);
    if !x.is_finite() || !y.is_finite() || x.abs() > MAX_OFFSET || y.abs() > MAX_OFFSET {
      warn!(path = %path.display(), id = %id, "ignoring a layout entry out of range");
      continue;
    }
    offsets.insert(id, Offset::at((x, y)));
  }
  Some(offsets)
}

fn save_offsets(path: &Path, offsets: &BTreeMap<String, Offset>) -> std::io::Result<()> {
  let file = LayoutFile {
    version: 1,
    offsets: offsets
      .iter()
      .map(|(id, offset)| (id.clone(), [offset.target.0, offset.target.1]))
      .collect(),
  };
  let text = toml::to_string(&file)
    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
  if let Some(parent) = path.parent()
    && !parent.as_os_str().is_empty()
  {
    std::fs::create_dir_all(parent)?;
  }
  let temporary = path.with_extension("toml.tmp");
  {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
      use std::os::unix::fs::OpenOptionsExt;
      options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
  }
  std::fs::rename(&temporary, path)?;
  Ok(())
}

#[cfg(test)]
mod tests {
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
    let regions =
      vec![region("clock", Rect::new(80.0, 80.0, 20.0, 20.0), true).with_drag_scale(0.5)];
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
}
