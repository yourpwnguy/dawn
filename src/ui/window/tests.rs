use super::{AuthMode, MAX_SUCCESS_HOLD, clamped_hitstop, dev_auth_mode, media_control_id};
use crate::ui::media::MediaControl;
use std::time::Duration;

#[test]
fn success_hitstop_never_outlives_the_exit_budget() {
  // The config ranges top out below the ceiling today, but the clamp is the
  // invariant: a future range change must not be able to hold greetd.
  let requested = Duration::from_secs(9);
  assert_eq!(clamped_hitstop(requested, true), MAX_SUCCESS_HOLD);
  assert_eq!(
    clamped_hitstop(Duration::from_millis(400), true),
    Duration::from_millis(400)
  );
}

#[test]
fn submit_and_failure_hitstops_keep_their_full_length() {
  // These never gate the handoff because only a finished login checks them.
  let requested = Duration::from_millis(300);
  assert_eq!(clamped_hitstop(requested, false), requested);
}

#[test]
fn dev_mode_never_constructs_a_greetd_auth_mode() {
  assert!(matches!(dev_auth_mode(), AuthMode::DryRun));
}

#[test]
fn media_control_ids_map_to_the_right_action() {
  assert_eq!(
    media_control_id("media:0:prev"),
    Some((0, MediaControl::Previous))
  );
  assert_eq!(
    media_control_id("media:12:play"),
    Some((12, MediaControl::PlayPause))
  );
  assert_eq!(
    media_control_id("media:3:next"),
    Some((3, MediaControl::Next))
  );
}

#[test]
fn malformed_media_ids_are_ignored() {
  // Missing pieces, extra pieces, unknown actions, and values that would
  // overflow a usize all have to fall through without panicking.
  for id in [
    "",
    "media",
    "media:",
    "media:0",
    "media::play",
    "media:0:stop",
    "media:0:play:extra",
    "music:0:play",
    "media:-1:play",
    "media:abc:play",
    "media:999999999999999999999999999999:play",
  ] {
    assert_eq!(media_control_id(id), None, "{id:?} should be ignored");
  }
}

#[test]
fn a_parseable_index_is_left_for_the_handle_lookup() {
  // The parser is deliberately not bound to the current handle count. A
  // valid id with a large index is still well formed; `pointer_press` drops
  // it with a checked `.get(index)` rather than treating it as malformed.
  assert_eq!(
    media_control_id("media:42:next"),
    Some((42, MediaControl::Next))
  );
}
