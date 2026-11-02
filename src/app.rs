//! UI state and the glue between keyboard input and the auth worker.
//!
//! This module owns the login form, the session picker, and the small amount of
//! policy that connects them: when to send a login attempt, when Escape cancels
//! one, and what to show when greetd answers. It deliberately has no window and
//! no drawing calls, so the entire interaction model is testable in-process.
//!
//! The one subtle rule here is the cancel guard. Pressing Escape while a request
//! is in flight sends `cancel_session` and then ignores any prompt that was
//! already on the wire. Without that, a late "Password:" reply would reopen the
//! form after the user asked to stop, and the next keystroke would be sent into
//! a session they thought they had abandoned.
//!
//! Keyboard map (pointer clicks and drags are translated by the window layer
//! into the same keys and focus requests):
//!
//! - `Tab` / `Shift+Tab`: move between username, password, and the picker.
//! - `Enter`: continue from the username, sign in from the password, or pick a
//!   session while the picker is open.
//! - `Up` / `Down`: choose a session in the picker.
//! - `Escape`: cancel a login in flight, otherwise clear the current field.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{info, warn};

use crate::audio::{AudioHandle, Event as AudioEvent};
use crate::auth::model::{Launch, NoticeKind, Prompt, PromptKind};
use crate::auth::worker::{Event, WorkerHandle};
use crate::config::{Config, MotionMoment, SESSION_FLASH_MS};
use crate::plugin::{self, Plugin};
use crate::rituals::RitualsConfig;
use crate::session::Session;
use crate::sysinfo::{BatterySnapshot, SystemSnapshot};
use crate::ui::input::TextInput;
use crate::ui::media::MediaState;
use crate::ui::motion::{Motion, MotionFrame};
use crate::ui::splash::{SplashArt, SplashState};
use crate::ui::theme::Theme;
use crate::ui::view::{Focus, Line, View};

const MAX_USERNAME_CHARS: usize = 256;
const MAX_ANSWER_CHARS: usize = 1024;
/// How often the local `/sys` and `/proc` snapshots are refreshed.
const SYSINFO_REFRESH: Duration = Duration::from_secs(2);
/// Number of load samples kept for the system sparkline.
const MAX_LOAD_HISTORY: usize = 32;

mod auth;
mod input;
mod lifecycle;
mod motion;
mod view;

#[cfg(test)]
mod tests;

/// A key the window or the TTY frontend recognised. Text keys are separate from
/// control keys so each frontend only has to map its own key codes once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
  Char(char),
  Enter,
  Tab,
  BackTab,
  Backspace,
  Delete,
  Left,
  Right,
  Home,
  End,
  Up,
  Down,
  Escape,
}

/// Why the frontend should stop. `Started` means greetd accepted
/// `start_session`, so the process must exit for the session to launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
  Started,
  Quit,
}

pub struct App {
  theme: Theme,
  sessions: Vec<Session>,
  session_index: usize,
  picker_index: usize,
  focus: Focus,
  username: TextInput,
  answer: TextInput,
  prompt: Option<Prompt>,
  /// Whether the current answer should be masked. Login passwords always are;
  /// a PAM "visible" prompt (an OTP, for example) is not.
  answer_secret: bool,
  /// Whether the password the user typed before PAM asked for one has already
  /// been sent for this conversation. At most ONE auto-answer is allowed
  /// between a Begin and the next Failed or Starting event, so a PAM re-ask
  /// after a wrong password is never answered with the stale secret.
  secret_auto: bool,
  status: Option<Line>,
  busy: bool,
  cancel_requested: bool,
  dry_run: bool,
  warning: Option<String>,
  worker: Option<WorkerHandle>,
  signature: String,
  plugins: Vec<Plugin>,
  media: Vec<Arc<MediaState>>,
  /// Last local battery reading and the time that read happened.
  battery: BatterySnapshot,
  system: SystemSnapshot,
  sysinfo_at: Instant,
  motion: Motion,
  authenticated: bool,
  finished: Option<Finish>,
  /// Increments every time an auth request is actually handed to the worker.
  /// The window watches it to start the optional cut sequence exactly on a
  /// submit, not on a Tab or an Enter that only moved focus. It carries no
  /// credential and no state the worker reads.
  submit_epoch: u64,
  /// Instant of the last inserted character, for the optional key pop.
  typed_at: Option<Instant>,
  /// Instant the session selection last changed, for the ticker flash.
  session_changed_at: Option<Instant>,
  /// The optional boot title card. `None` means the login form owns the screen
  /// from the first frame, which is the default for every existing config.
  splash: Option<SplashState>,
  /// Theme audio. Disabled unless the window attached a running mixer.
  audio: AudioHandle,
  /// Ritual rules copied out of the config. Empty unless enabled.
  rituals: RitualsConfig,
}

impl Drop for App {
  fn drop(&mut self) {
    // Dropping the input wipes any password still in it, even if the window
    // closed mid-typing. The worker handle wipes anything queued for greetd.
    self.answer.clear();
  }
}
