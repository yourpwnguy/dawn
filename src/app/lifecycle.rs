//! Lifecycle methods for the login app.

use super::*;

impl App {
  pub fn new(
    config: Config,
    sessions: Vec<Session>,
    mut plugins: Vec<Plugin>,
    worker: Option<WorkerHandle>,
    warning: Option<String>,
    dry_run: bool,
  ) -> Self {
    let theme = Theme::from(&config);
    let builtins = plugin::builtins(&config);
    // Collect third-party media state before appending the built-in plugin,
    // otherwise the built-in media widget would be counted twice and the
    // window would start two identical MPRIS pollers.
    let mut media: Vec<Arc<MediaState>> = plugins.iter().flat_map(Plugin::media_states).collect();
    plugins.extend(builtins.plugins);
    media.extend(builtins.media);

    let status = sessions
      .is_empty()
      .then(|| Line::error(config.login.no_sessions.clone()));
    let motion = Motion::new(&config.motion, config.power.low_power);
    let splash = SplashState::new(&config.login.splash, Instant::now());
    // Read the local hardware once here so the very first frame is complete.
    // Later refreshes happen on the UI tick; there is no background thread.
    let battery = BatterySnapshot::read(Path::new("/"));
    let mut system = SystemSnapshot::read(Path::new("/"));
    system.load_history.push(system.load[0]);
    let sysinfo_at = Instant::now();
    // The prefill is a convenience, never an authentication decision. If the
    // account database is unreadable the field simply starts empty. A filled
    // field also starts on the password, which is the whole point: one Enter
    // and the user is in.
    let prefilled = crate::user::resolve(&config.login.prefill);
    let has_prefill = prefilled.is_some();
    let username = prefilled.map_or_else(
      || TextInput::new(MAX_USERNAME_CHARS),
      |name| TextInput::with_value(name, MAX_USERNAME_CHARS),
    );

    Self {
      theme,
      sessions,
      session_index: 0,
      picker_index: 0,
      focus: if has_prefill {
        Focus::Password
      } else {
        Focus::Username
      },
      username,
      answer: TextInput::new(MAX_ANSWER_CHARS),
      prompt: None,
      answer_secret: true,
      secret_auto: false,
      status,
      busy: false,
      cancel_requested: false,
      dry_run,
      warning,
      worker,
      signature: String::new(),
      plugins,
      media,
      battery,
      system,
      sysinfo_at,
      motion,
      authenticated: false,
      finished: None,
      submit_epoch: 0,
      typed_at: None,
      session_changed_at: None,
      splash,
      audio: AudioHandle::disabled(),
      rituals: config.rituals,
    }
  }

  /// Attaches a running mixer. Tests and screenshots keep the disabled handle
  /// from [`App::new`], so no test can open a sound device by accident.
  pub fn attach_audio(&mut self, audio: AudioHandle) {
    self.audio = audio;
  }

  #[must_use]
  pub fn audio(&self) -> &AudioHandle {
    &self.audio
  }

  /// Shows a boot note from the rituals engine. It never replaces an existing
  /// status such as "no sessions", because that error is more important.
  pub fn set_boot_note(&mut self, text: String) {
    if self.status.is_none() {
      self.status = Some(Line::info(text));
    }
  }

  /// Swaps the theme and widget list after a successful hot reload.
  ///
  /// This deliberately keeps the auth worker, sessions, typed input, and media
  /// pollers. A theme edit is a cosmetic event: it must not reset a half-typed
  /// password or restart the login state machine. The text engine lives in the
  /// window layer and is only rebuilt when the font actually changed.
  pub fn reload(&mut self, config: Config, mut plugins: Vec<Plugin>, warning: Option<String>) {
    self.theme = Theme::from(&config);
    // A reload is a cosmetic event. If the splash already finished or was
    // disabled, it stays gone; if the editor just enabled one, it starts now.
    if self.splash.is_none() && config.login.splash.enabled {
      self.splash = SplashState::new(&config.login.splash, Instant::now());
    } else if self.splash.is_some() && !config.login.splash.enabled {
      self.splash = None;
    }
    let builtins = plugin::builtins(&config);
    plugins.extend(builtins.plugins);
    self.plugins = plugins;
    self.warning = warning;
    self.motion = Motion::new(&config.motion, config.power.low_power);
    // The tick signature is built from the old widget list. Clearing it makes
    // the next tick notice the new widgets instead of reporting no change.
    self.signature.clear();
    if self.sessions.is_empty() {
      self.status = Some(Line::error(config.login.no_sessions));
    }
  }

  #[must_use]
  pub fn finished(&self) -> Option<Finish> {
    self.finished
  }

  /// How many auth requests have actually been sent. The window compares this
  /// before and after a key so a focus move never plays the cut sequence.
  #[must_use]
  pub fn submit_epoch(&self) -> u64 {
    self.submit_epoch
  }

  /// Moves keyboard focus when the pointer clicks a field. The fields are
  /// still fully keyboard-driven; this just removes the surprise of a click
  /// doing nothing.
  pub fn focus_field(&mut self, focus: Focus) -> bool {
    if self.busy || self.focus == focus {
      return false;
    }
    self.focus = focus;
    self.typed_at = None;
    self.audio.play(AudioEvent::FieldFocus);
    self.motion.notify_focus(Instant::now());
    true
  }

  /// Called when the window closes. The auth worker is stopped by dropping the
  /// handle, which also wipes any secret still queued for it.
  pub fn quit(&mut self) {
    self.finished = Some(Finish::Quit);
  }

  /// Recomputes every widget's visible signature. Returns true when something
  /// changed, which is the only reason an otherwise idle greeter redraws.
  pub fn tick(&mut self, now: &jiff::Zoned) -> bool {
    let sysinfo_changed = self.refresh_sysinfo(Instant::now());
    let mut signature = String::new();
    for plugin in &self.plugins {
      for widget in &plugin.widgets {
        signature.push('\u{1f}');
        signature.push_str(&widget.signature(now));
      }
    }
    let changed = signature != self.signature;
    self.signature = signature;
    changed || sysinfo_changed
  }

  /// Refreshes the battery and system snapshots at most once per
  /// [`SYSINFO_REFRESH`].
  ///
  /// The refresh is deliberately synchronous. Reading a handful of tiny proc
  /// files costs far less than spawning a thread and waking it forever, and the
  /// UI tick already exists. We compare the visible fields before assigning the
  /// new snapshot so a new sparkline sample alone does not mark the whole UI
  /// dirty; the sample is appended after the comparison.
  pub(super) fn refresh_sysinfo(&mut self, now: Instant) -> bool {
    if now.saturating_duration_since(self.sysinfo_at) < SYSINFO_REFRESH {
      return false;
    }
    self.sysinfo_at = now;
    let battery = BatterySnapshot::read(Path::new("/"));
    let next = SystemSnapshot::read(Path::new("/"));
    let changed = battery != self.battery
      || next.hostname != self.system.hostname
      || next.kernel != self.system.kernel
      || next.uptime != self.system.uptime
      || next.load != self.system.load
      || next.mem_used != self.system.mem_used
      || next.mem_total != self.system.mem_total;
    let mut system = next;
    system.load_history = self.system.load_history.clone();
    let sample = system.load[0];
    system.load_history.push(sample);
    if system.load_history.len() > MAX_LOAD_HISTORY {
      let excess = system.load_history.len() - MAX_LOAD_HISTORY;
      system.load_history.drain(..excess);
    }
    self.battery = battery;
    self.system = system;
    changed
  }
}
