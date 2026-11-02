//! Boot splash and login cut-sequence configuration.

use super::*;

/// What dismisses the boot splash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SplashDismiss {
  /// Only the clock. This is the safe default.
  #[default]
  Timeout,
  /// Enter dismisses early. Other keys are ignored while the form is hidden.
  Enter,
  /// Any key except Escape dismisses early.
  Any,
}

impl SplashDismiss {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "timeout" => Some(Self::Timeout),
      "enter" => Some(Self::Enter),
      "any" => Some(Self::Any),
      _ => None,
    }
  }
}

/// The optional boot title card. It never blocks auth: there is no auth request
/// to block, and once the form is visible the splash state is gone.
#[derive(Debug, Clone, PartialEq)]
pub struct SplashConfig {
  pub enabled: bool,
  /// Time the title stays fully visible, 500 to 10000 ms.
  pub ms: Duration,
  pub title: String,
  pub sub: String,
  /// Crossfade into the login form, 100 to 2000 ms.
  pub fade: Duration,
  pub dismiss_on: SplashDismiss,
  /// Background override. `None` uses the resolved wallpaper colour.
  pub bg: Option<Rgb>,
  /// Accent override. `None` uses the theme accent.
  pub accent: Option<Rgb>,
}

impl Default for SplashConfig {
  fn default() -> Self {
    Self {
      enabled: false,
      ms: Duration::from_millis(2500),
      title: String::new(),
      sub: String::new(),
      fade: Duration::from_millis(300),
      dismiss_on: SplashDismiss::Timeout,
      bg: None,
      accent: None,
    }
  }
}

/// When the login cut sequence is allowed to play.
///
/// `Enter` is the safe default: the sequence starts the moment an auth request
/// is actually sent, so it never delays or replaces anything the auth worker
/// does. `Success` waits for greetd to accept the start, which means the window
/// briefly holds its exit so the slam can be seen. `Both` plays at whichever
/// moment comes first and never starts a second sequence while one is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransitionTrigger {
  #[default]
  Enter,
  Success,
  Both,
  /// Play once when the window first comes up, before the idle screen.
  /// Authentication never starts or extends this sequence.
  Boot,
  /// Play on a rejected auth attempt. The window keeps the normal form and
  /// schedules the next frame; a failure must never hold the process open.
  Failure,
}

impl TransitionTrigger {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "enter" => Some(Self::Enter),
      "success" => Some(Self::Success),
      "both" => Some(Self::Both),
      "boot" => Some(Self::Boot),
      "failure" => Some(Self::Failure),
      _ => None,
    }
  }

  /// True when an event of `kind` should start the sequence.
  ///
  /// `Both` deliberately keeps its historical meaning of enter plus success.
  /// Letting it also accept boot or failure would make an existing config
  /// play a cut the administrator never asked for, which breaks the rule that
  /// new triggers are opt-in.
  #[must_use]
  pub fn accepts(self, kind: Self) -> bool {
    match kind {
      Self::Enter | Self::Success => self == kind || self == Self::Both,
      Self::Boot | Self::Failure | Self::Both => self == kind,
    }
  }
}

/// A short montage of full-screen colour cuts followed by one slammed line.
///
/// This is purely decorative. The auth request is already on its way when the
/// sequence starts, and the window only ever holds its own exit for a bounded
/// moment on the `Success` trigger. A broken or zero-length sequence therefore
/// degrades to the normal screen instead of ever blocking a login.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionConfig {
  pub enabled: bool,
  pub trigger: TransitionTrigger,
  /// Full-screen flash colours, one per cut. Capped at eight at load time.
  pub cuts: Vec<Rgb>,
  /// How long each cut stays on screen.
  pub cut_ms: f32,
  /// Peak shake amplitude in logical pixels. Decays to zero over the slam.
  pub shake: f32,
  /// Extra scale punch at the start of the slam, 0 to 0.5.
  pub zoom: f32,
  /// The big centred line shown after the cuts.
  pub slam: String,
  /// How long the slam stays on screen, including its fade out.
  pub slam_ms: f32,
  pub slam_color: Rgb,
  pub slam_size: f32,
}

impl Default for TransitionConfig {
  fn default() -> Self {
    Self {
      enabled: false,
      trigger: TransitionTrigger::Enter,
      cuts: vec![
        Rgb::solid(0xFF, 0xFF, 0xFF),
        Rgb::solid(0xD4, 0x18, 0x30),
        Rgb::solid(0x0E, 0x0B, 0x10),
      ],
      cut_ms: 55.0,
      shake: 14.0,
      zoom: 0.06,
      slam: "WELCOME".to_string(),
      slam_ms: 900.0,
      slam_color: Rgb::solid(0xF3, 0xE7, 0xD2),
      slam_size: 92.0,
    }
  }
}

impl TransitionConfig {
  /// Total time the sequence can run. Cuts that are all zero still leave the
  /// slam, and a zero slam collapses the whole thing to nothing.
  #[must_use]
  pub fn total(&self) -> Duration {
    let cut_ms = if self.cut_ms.is_finite() {
      self.cut_ms.max(0.0)
    } else {
      0.0
    };
    let slam_ms = if self.slam_ms.is_finite() {
      self.slam_ms.max(0.0)
    } else {
      0.0
    };
    let millis = self.cuts.len() as f32 * cut_ms + slam_ms;
    if !millis.is_finite() {
      return Duration::ZERO;
    }
    Duration::from_secs_f32(millis / 1000.0)
  }
}
