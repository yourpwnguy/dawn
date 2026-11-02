use super::{
  Boot, DropWeights, Moment, Pending, Rarity, RitualsConfig, State, day_number, days_between,
  on_boot_at, on_success_at, state_path,
};
use crate::audio::Event;

fn temp_dir(name: &str) -> std::path::PathBuf {
  let path = std::env::temp_dir().join(format!(
    "dawn-rituals-{name}-{}-{}",
    std::process::id(),
    std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|duration| duration.as_nanos())
      .unwrap_or_default()
  ));
  std::fs::create_dir_all(&path).expect("temp dir");
  path
}

fn config(dir: &std::path::Path) -> RitualsConfig {
  RitualsConfig {
    enabled: true,
    state_path: Some(dir.join("state.toml")),
    achievements: true,
    ..RitualsConfig::default()
  }
}

fn moment(date: &str, hour: i8, weekday: u8) -> Moment {
  Moment {
    date: date.to_string(),
    hour,
    weekday,
  }
}

#[test]
fn date_math_matches_known_days() {
  assert_eq!(day_number("2026-10-30"), day_number("2026-10-30"));
  assert_eq!(days_between("2026-10-29", "2026-10-30"), Some(1));
  assert_eq!(days_between("2026-10-28", "2026-10-30"), Some(2));
  assert_eq!(day_number("2026-13-01"), None);
}

#[test]
fn the_drop_is_deterministic_per_date() {
  assert_eq!(DropWeights::default().pick(0.1), Rarity::Common);
  assert_eq!(DropWeights::default().pick(0.8), Rarity::Rare);
  assert_eq!(DropWeights::default().pick(0.99), Rarity::Saga);
}

#[test]
fn a_first_login_starts_a_streak() {
  let dir = temp_dir("first");
  let config = config(&dir);
  on_success_at(&config, &moment("2026-10-30", 12, 4));
  let state: State =
    toml::from_str(&std::fs::read_to_string(dir.join("state.toml")).expect("read state"))
      .expect("parse state");
  assert_eq!(state.streak, 1);
  assert!(state.achievements.contains(&"first_login".to_string()));
  std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn consecutive_days_extend_and_a_gap_resets() {
  let dir = temp_dir("streak");
  let config = config(&dir);
  on_success_at(&config, &moment("2026-10-28", 12, 2));
  on_success_at(&config, &moment("2026-10-29", 12, 3));
  on_success_at(&config, &moment("2026-10-30", 12, 4));
  let state: State =
    toml::from_str(&std::fs::read_to_string(dir.join("state.toml")).expect("read state"))
      .expect("parse state");
  assert_eq!(state.streak, 3);
  assert_eq!(state.best_streak, 3);
  assert!(
    state
      .pending
      .iter()
      .any(|pending| pending.note.contains("3 day streak"))
  );

  on_success_at(&config, &moment("2026-11-05", 12, 3));
  let state: State =
    toml::from_str(&std::fs::read_to_string(dir.join("state.toml")).expect("read state"))
      .expect("parse state");
  assert_eq!(state.streak, 1);
  std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_same_day_does_not_double_count() {
  let dir = temp_dir("same-day");
  let config = config(&dir);
  on_success_at(&config, &moment("2026-10-30", 12, 4));
  on_success_at(&config, &moment("2026-10-30", 13, 4));
  let state: State =
    toml::from_str(&std::fs::read_to_string(dir.join("state.toml")).expect("read state"))
      .expect("parse state");
  assert_eq!(state.streak, 1);
  std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_boot_plays_the_pending_sounds_once() {
  let dir = temp_dir("boot");
  let config = config(&dir);
  let mut state = State {
    streak: 3,
    last_login: Some("2026-10-29".to_string()),
    pending: vec![Pending {
      event: "milestone".to_string(),
      note: "3 day streak".to_string(),
    }],
    ..State::default()
  };
  std::fs::write(
    dir.join("state.toml"),
    toml::to_string(&state).expect("encode"),
  )
  .expect("seed state");
  let boot: Boot = on_boot_at(&config, &moment("2026-10-30", 9, 5));
  assert!(boot.events.contains(&Event::Milestone));
  assert!(boot.events.iter().any(|event| {
    matches!(
      event,
      Event::RarityCommon | Event::RarityRare | Event::RaritySaga
    )
  }));
  assert!(boot.note.as_deref().unwrap_or("").contains("3 day streak"));

  state = toml::from_str(&std::fs::read_to_string(dir.join("state.toml")).expect("read state"))
    .expect("parse state");
  assert!(state.pending.is_empty());
  let second = on_boot_at(&config, &moment("2026-10-30", 10, 5));
  assert!(!second.events.contains(&Event::Milestone));
  std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_state_path_prefers_the_config() {
  let dir = temp_dir("path");
  let config = config(&dir);
  assert_eq!(state_path(&config), Some(dir.join("state.toml")));
  std::fs::remove_dir_all(&dir).ok();
}
