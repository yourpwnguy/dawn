//! A frame's worth of state, described without any drawing calls.
//!
//! The app builds one of these before every redraw and the renderer consumes
//! it. Splitting the two is what lets `--screenshot` render exactly what the
//! window would show, and it keeps the app free of pixel math. Everything here
//! is plain data and already safe to display: the password is passed as its
//! masked string, never as the real value.

use crate::sysinfo::{BatterySnapshot, SystemSnapshot};

/// Which control currently owns the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
  #[default]
  Username,
  Password,
  /// The session picker is open and grabs the arrow keys and Enter.
  Picker,
}

/// How a status line should be coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
  Info,
  Error,
}

/// One message under the form: an error, a hint, or "signing in".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
  pub text: String,
  pub tone: Tone,
}

impl Line {
  #[must_use]
  pub fn info(text: impl Into<String>) -> Self {
    Self {
      text: text.into(),
      tone: Tone::Info,
    }
  }

  #[must_use]
  pub fn error(text: impl Into<String>) -> Self {
    Self {
      text: text.into(),
      tone: Tone::Error,
    }
  }
}

/// One frame of the optional boot title card.
///
/// The state machine lives in `ui::splash`; this is only the already-resolved
/// text, opacity, and colours the renderer needs. `None` on `View::splash`
/// means the normal login form is the whole scene.
#[derive(Debug, Clone, PartialEq)]
pub struct SplashView {
  pub title: String,
  pub sub: String,
  /// 1.0 while the card is fully visible, falling to 0 during the crossfade.
  pub opacity: f32,
  /// Beat phase, used for the pulsing underline when the beat clock is on.
  pub beat_phase: f32,
  /// True when `[motion.beat]` has a positive BPM, so the underline pulses.
  pub underline: bool,
  /// Optional overrides. `None` follows the wallpaper or theme accent.
  pub bg: Option<crate::color::Rgb>,
  pub accent: Option<crate::color::Rgb>,
}

impl SplashView {
  /// True while the splash fully covers the form, so the renderer can skip the
  /// login overlay entirely instead of painting it under an opaque card.
  #[must_use]
  pub fn covers(&self) -> bool {
    self.opacity >= 0.999
  }

  /// True when the card also hides the wallpaper. A fixed background colour is
  /// opaque; a colour override with alpha still needs the wallpaper underneath,
  /// and a fading card always does.
  #[must_use]
  pub fn opaque(&self, background: crate::color::Rgb) -> bool {
    self.covers() && self.bg.unwrap_or(background).a == 255
  }
}

/// Everything the renderer needs for one frame.
#[derive(Debug, Clone, Default)]
pub struct View {
  /// Optional line above the form, from `ui.greeting`.
  pub greeting: String,
  /// Actual username text.
  pub username: String,
  /// Caret position in the username, as a char index.
  pub username_cursor: usize,
  /// Password field contents, already masked by the app.
  pub password: String,
  /// Caret position in the password field, as a char index.
  pub password_cursor: usize,
  pub focus: Focus,
  /// When the focused field last received a typed character. `None` means no
  /// pop is active. The renderer only uses this for the keystroke animation.
  pub typed_at: Option<std::time::Instant>,
  /// When the selected session last changed. Drives the ticker flash beat.
  pub session_changed_at: Option<std::time::Instant>,
  /// PAM's question, shown just above the password field.
  pub prompt: Option<String>,
  pub status: Option<Line>,
  /// Name of the session that will be launched.
  pub session: String,
  pub session_count: usize,
  /// Session names shown in the picker. Empty when it is closed.
  pub picker: Vec<String>,
  pub picker_selected: usize,
  pub dry_run: bool,
  /// Config warning, painted at the bottom so it cannot cover the form.
  pub warning: Option<String>,
  /// True while an auth request is in flight. Hides the caret and disables the
  /// usual hints so the screen does not look editable.
  pub busy: bool,
  /// True once greetd accepted the credentials. Widgets that could expose
  /// user data (the media widget in particular) must stay inert until this is
  /// set, because the greeter is a pre-login surface.
  pub authenticated: bool,
  /// Boot title card, or `None` once the normal form is visible.
  pub splash: Option<SplashView>,
  /// Last local battery reading. `present = false` on a desktop.
  pub battery: BatterySnapshot,
  /// Last local system reading. The renderer reads `load_history` for the
  /// sparkline; the app is the only writer.
  pub system: SystemSnapshot,
}

impl View {
  /// True when the picker is open. The renderer uses one flag for the overlay
  /// and the card layout.
  #[must_use]
  pub fn picker_open(&self) -> bool {
    self.focus == Focus::Picker
  }
}
