//! Opt-in rituals: streaks, milestones, rarity drops, and achievements.
//!
//! Rituals are per machine, not per user. The greeter often does not know the
//! account yet, and building a profile per username would leak who signs in.
//! State is counts and dates only, written after a successful login or a boot.
//!
//! Everything here is cosmetic and local. Rituals never influence
//! authentication, never touch the greetd socket, and never block the login
//! screen. A missing or unwritable state file disables them for the run.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use jiff::Zoned;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::audio::Event;

/// One boot drop's rarity weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropWeights {
  pub common: u8,
  pub rare: u8,
  pub saga: u8,
}

impl Default for DropWeights {
  fn default() -> Self {
    Self {
      common: 70,
      rare: 25,
      saga: 5,
    }
  }
}

impl DropWeights {
  #[must_use]
  pub fn total(self) -> u32 {
    u32::from(self.common) + u32::from(self.rare) + u32::from(self.saga)
  }

  #[must_use]
  pub fn is_empty(self) -> bool {
    self.total() == 0
  }

  /// Picks a rarity from a 0.0 to 1.0 roll.
  #[must_use]
  pub fn pick(self, roll: f64) -> Rarity {
    let total = f64::from(self.total().max(1));
    let scaled = (roll.clamp(0.0, 1.0) * total).floor();
    let common = f64::from(self.common);
    let rare = f64::from(self.rare);
    if scaled < common {
      Rarity::Common
    } else if scaled < common + rare {
      Rarity::Rare
    } else {
      Rarity::Saga
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rarity {
  Common,
  Rare,
  Saga,
}

impl Rarity {
  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Common => "common",
      Self::Rare => "rare",
      Self::Saga => "saga",
    }
  }

  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value.trim().to_ascii_lowercase().as_str() {
      "common" => Some(Self::Common),
      "rare" => Some(Self::Rare),
      "saga" => Some(Self::Saga),
      _ => None,
    }
  }

  #[must_use]
  pub fn event(self) -> Event {
    match self {
      Self::Common => Event::RarityCommon,
      Self::Rare => Event::RarityRare,
      Self::Saga => Event::RaritySaga,
    }
  }

  #[must_use]
  pub fn label(self) -> &'static str {
    match self {
      Self::Common => "Common drop",
      Self::Rare => "Rare drop",
      Self::Saga => "Saga drop",
    }
  }
}

/// How the daily drop is seeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DropSeed {
  /// Same drop for every boot on the same local date.
  #[default]
  Date,
  /// A new roll each boot.
  Boot,
}

impl DropSeed {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value.trim().to_ascii_lowercase().as_str() {
      "date" => Some(Self::Date),
      "boot" => Some(Self::Boot),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Date => "date",
      Self::Boot => "boot",
    }
  }
}

/// The `[rituals]` table.
#[derive(Debug, Clone, PartialEq)]
pub struct RitualsConfig {
  pub enabled: bool,
  /// Where the per-machine state lives. `None` derives a path from
  /// `XDG_STATE_HOME` or `HOME` at boot; if neither is usable, rituals stay
  /// off for that run.
  pub state_path: Option<PathBuf>,
  /// Achievements are a separate switch because some themes want the streak
  /// without the unlock messages.
  pub achievements: bool,
  pub drops: bool,
  pub seed: DropSeed,
  pub weights: DropWeights,
}

impl Default for RitualsConfig {
  fn default() -> Self {
    Self {
      enabled: false,
      state_path: None,
      achievements: false,
      drops: true,
      seed: DropSeed::Date,
      weights: DropWeights::default(),
    }
  }
}

impl RitualsConfig {
  #[must_use]
  pub fn summary(&self) -> String {
    format!(
      "enabled={} achievements={} drops={} seed={} weights={}/{}/{} state={}",
      self.enabled,
      self.achievements,
      self.drops,
      self.seed.as_str(),
      self.weights.common,
      self.weights.rare,
      self.weights.saga,
      self
        .state_path
        .as_ref()
        .map_or_else(|| "auto".to_string(), |path| path.display().to_string())
    )
  }
}

/// Local date and time, split into the parts the rituals rules need. Keeping
/// this explicit makes the engine testable without freezing the process clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moment {
  /// `YYYY-MM-DD` in local time.
  pub date: String,
  /// 0 to 23.
  pub hour: i8,
  /// 0 for Monday through 6 for Sunday.
  pub weekday: u8,
}

impl Moment {
  #[must_use]
  pub fn now() -> Self {
    let now = Zoned::now();
    Self {
      date: now.date().to_string(),
      hour: now.hour(),
      weekday: match now.date().weekday() {
        jiff::civil::Weekday::Monday => 0,
        jiff::civil::Weekday::Tuesday => 1,
        jiff::civil::Weekday::Wednesday => 2,
        jiff::civil::Weekday::Thursday => 3,
        jiff::civil::Weekday::Friday => 4,
        jiff::civil::Weekday::Saturday => 5,
        jiff::civil::Weekday::Sunday => 6,
      },
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
  /// Audio event name, or empty when the entry is text only.
  pub event: String,
  pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
  pub version: u32,
  pub streak: u32,
  pub best_streak: u32,
  pub last_login: Option<String>,
  pub grace_day: Option<String>,
  pub boot_count: u64,
  pub last_drop_date: Option<String>,
  pub last_drop: Option<String>,
  pub pending: Vec<Pending>,
  pub achievements: Vec<String>,
}

impl Default for State {
  fn default() -> Self {
    Self {
      version: 1,
      streak: 0,
      best_streak: 0,
      last_login: None,
      grace_day: None,
      boot_count: 0,
      last_drop_date: None,
      last_drop: None,
      pending: Vec::new(),
      achievements: Vec::new(),
    }
  }
}

/// What the next greeter start should play and show.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Boot {
  pub events: Vec<Event>,
  pub note: Option<String>,
}

const MILESTONES: [u32; 6] = [3, 7, 14, 30, 100, 365];

/// Resolves the state path from the config, then the XDG state dir, then home.
#[must_use]
pub fn state_path(config: &RitualsConfig) -> Option<PathBuf> {
  if let Some(path) = &config.state_path {
    return Some(path.clone());
  }
  if let Some(state) = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
    return Some(PathBuf::from(state).join("dawn/rituals.toml"));
  }
  let home = std::env::var_os("HOME").filter(|value| !value.is_empty())?;
  Some(PathBuf::from(home).join(".local/state/dawn/rituals.toml"))
}

/// Runs the boot half: bump the counter, hand out today's drop, and return the
/// pending sounds and note. Never fails; a problem disables rituals for the
/// run with one warning.
#[must_use]
pub fn on_boot(config: &RitualsConfig) -> Boot {
  on_boot_at(config, &Moment::now())
}

#[must_use]
pub fn on_boot_at(config: &RitualsConfig, now: &Moment) -> Boot {
  if !config.enabled {
    return Boot::default();
  }
  let Some(path) = state_path(config) else {
    warn!("rituals: no usable state path; rituals are off for this run");
    return Boot::default();
  };
  let mut state = load(&path);
  let mut boot = Boot::default();
  state.version = 1;
  state.boot_count = state.boot_count.saturating_add(1);

  for pending in &state.pending {
    if let Some(event) = Event::parse(&pending.event) {
      boot.events.push(event);
    }
    if !pending.note.is_empty() {
      push_note(&mut boot.note, &pending.note);
    }
  }
  state.pending.clear();

  if config.drops && state.last_drop_date.as_deref() != Some(now.date.as_str()) {
    let roll = roll_for(config.seed, &now.date, state.boot_count);
    let rarity = config.weights.pick(roll);
    state.last_drop_date = Some(now.date.clone());
    state.last_drop = Some(rarity.as_str().to_string());
    boot.events.push(rarity.event());
    push_note(&mut boot.note, rarity.label());
  }

  if state.streak > 1 {
    push_note(&mut boot.note, &format!("{} day streak", state.streak));
  }

  save(&path, &state);
  boot
}

/// Runs after a successful authentication. Updates the streak, milestones, and
/// achievements. The sounds are deferred to the next boot so the handoff never
/// waits for audio.
pub fn on_success(config: &RitualsConfig) {
  on_success_at(config, &Moment::now());
}

pub fn on_success_at(config: &RitualsConfig, now: &Moment) {
  if !config.enabled {
    return;
  }
  let Some(path) = state_path(config) else {
    return;
  };
  let mut state = load(&path);
  state.version = 1;
  let first_login = state.last_login.is_none();
  let previous_streak = state.streak;
  let gap = state
    .last_login
    .as_deref()
    .and_then(|last| days_between(last, &now.date));

  if first_login {
    state.streak = 1;
  } else {
    match gap {
      Some(0) | None => {}
      Some(1) => state.streak = state.streak.saturating_add(1),
      Some(2)
        if state
          .grace_day
          .as_deref()
          .and_then(|day| days_between(day, &now.date))
          .is_none_or(|gap| gap > 7) =>
      {
        // One grace day per week keeps a missed evening from erasing a long
        // run without making the counter fake.
        state.grace_day = Some(now.date.clone());
        state.streak = state.streak.saturating_add(1);
      }
      Some(_) => state.streak = 1,
    }
  }

  state.best_streak = state.best_streak.max(state.streak);
  state.last_login = Some(now.date.clone());

  if MILESTONES.contains(&state.streak) {
    state.pending.push(Pending {
      event: "milestone".to_string(),
      note: format!("{} day streak", state.streak),
    });
  }

  if config.achievements {
    let mut unlocked: BTreeSet<String> = state.achievements.iter().cloned().collect();
    let mut award = |id: &str, note: &str| {
      if unlocked.insert(id.to_string()) {
        state.pending.push(Pending {
          event: "achievement".to_string(),
          note: note.to_string(),
        });
      }
    };
    if first_login {
      award("first_login", "First login");
    }
    if state.streak >= 7 {
      award("first_week", "One week streak");
    }
    if now.hour >= 23 || now.hour < 4 {
      award("night_owl", "Night owl");
    }
    if (5..=6).contains(&now.hour) {
      award("early_bird", "Early bird");
    }
    if now.weekday >= 5 {
      award("weekend", "Weekend login");
    }
    if previous_streak >= 3 && state.streak == 1 {
      award("comeback", "Back on stage");
    }
    state.achievements = unlocked.into_iter().collect();
  }

  save(&path, &state);
}

/// CLI entry point for `dawn rituals status|reset`.
pub fn run_cli(command: crate::cli::RitualsCommand) -> std::process::ExitCode {
  use std::process::ExitCode;

  let loaded = crate::config::load(None);
  match command {
    crate::cli::RitualsCommand::Status => {
      println!("{}", status_text(&loaded.config.rituals));
      ExitCode::SUCCESS
    }
    crate::cli::RitualsCommand::Reset => match reset(&loaded.config.rituals) {
      Ok(path) => {
        println!("reset {}", path.display());
        ExitCode::SUCCESS
      }
      Err(error) => {
        eprintln!("dawn: {error}");
        ExitCode::FAILURE
      }
    },
  }
}

/// Human-readable status for `dawn rituals status`.
#[must_use]
pub fn status_text(config: &RitualsConfig) -> String {
  let Some(path) = state_path(config) else {
    return "rituals: no usable state path".to_string();
  };
  let state = load(&path);
  format!(
    "rituals: enabled={} state={}\nstreak={} best={} last_login={} boot_count={} drop={} achievements={}",
    config.enabled,
    path.display(),
    state.streak,
    state.best_streak,
    state.last_login.as_deref().unwrap_or("never"),
    state.boot_count,
    state.last_drop.as_deref().unwrap_or("none"),
    if state.achievements.is_empty() {
      "none".to_string()
    } else {
      state.achievements.join(", ")
    }
  )
}

/// Removes the state file. Returns the path for the CLI message.
pub fn reset(config: &RitualsConfig) -> Result<PathBuf, String> {
  let Some(path) = state_path(config) else {
    return Err("no usable state path".to_string());
  };
  match std::fs::remove_file(&path) {
    Ok(()) => Ok(path),
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
    Err(error) => Err(format!("cannot remove {}: {error}", path.display())),
  }
}

fn push_note(note: &mut Option<String>, text: &str) {
  match note {
    Some(existing) => {
      existing.push_str(" · ");
      existing.push_str(text);
    }
    None => *note = Some(text.to_string()),
  }
}

fn load(path: &Path) -> State {
  let metadata = match std::fs::metadata(path) {
    Ok(metadata) => metadata,
    Err(_) => return State::default(),
  };
  if metadata.len() > 64 * 1024 {
    warn!(
      "rituals: {} is larger than 64 KiB; resetting",
      path.display()
    );
    return State::default();
  }
  let text = match std::fs::read_to_string(path) {
    Ok(text) => text,
    Err(error) => {
      warn!(
        "rituals: cannot read {}: {error}; resetting",
        path.display()
      );
      return State::default();
    }
  };
  match toml::from_str::<State>(&text) {
    Ok(mut state) if state.version <= 1 => {
      state.version = 1;
      state
    }
    Ok(_) => {
      warn!(
        "rituals: unknown state version; resetting {}",
        path.display()
      );
      State::default()
    }
    Err(error) => {
      warn!(
        "rituals: cannot parse {}: {error}; resetting",
        path.display()
      );
      State::default()
    }
  }
}

fn save(path: &Path, state: &State) {
  let text = match toml::to_string(state) {
    Ok(text) => text,
    Err(error) => {
      warn!("rituals: cannot encode state: {error}");
      return;
    }
  };
  if let Err(error) = crate::fsutil::write_atomic(path, &text, 0o600) {
    warn!("rituals: {error}; state not saved");
  }
}

/// Deterministic roll for a date (and boot count when the seed asks for it).
fn roll_for(seed: DropSeed, date: &str, boot_count: u64) -> f64 {
  let host = std::env::var("HOSTNAME").unwrap_or_default();
  let mut hash = 0xcbf2_9ce4_8422_2325u64;
  let mut feed = |bytes: &[u8]| {
    for byte in bytes {
      hash ^= u64::from(*byte);
      hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
  };
  feed(date.as_bytes());
  feed(host.as_bytes());
  if seed == DropSeed::Boot {
    feed(&boot_count.to_le_bytes());
  }
  (hash >> 11) as f64 / (1u64 << 53) as f64
}

/// Day number for a `YYYY-MM-DD` date, using the civil-from-days algorithm.
fn day_number(date: &str) -> Option<i64> {
  let mut parts = date.split('-');
  let year: i64 = parts.next()?.parse().ok()?;
  let month: i64 = parts.next()?.parse().ok()?;
  let day: i64 = parts.next()?.parse().ok()?;
  if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
    return None;
  }
  let year = if month <= 2 { year - 1 } else { year };
  let era = if year >= 0 { year } else { year - 399 } / 400;
  let yoe = year - era * 400;
  let mp = (month + 9) % 12;
  let doy = (153 * mp + 2) / 5 + day - 1;
  let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
  Some(era * 146_097 + doe - 719_468)
}

fn days_between(earlier: &str, later: &str) -> Option<i64> {
  Some(day_number(later)? - day_number(earlier)?)
}

#[cfg(test)]
mod tests;
