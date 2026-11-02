//! Top-level machine settings and the compiled [`Config`].
//!
//! This module also owns the defaults. Everything a theme can change lives in
//! the sibling domain modules; this file is the shape that ties them together.

use super::*;

#[derive(Debug, Clone)]
pub struct Config {
  pub ui: UiConfig,
  /// The login card, its fields, and every visible string in the form.
  pub login: LoginConfig,
  pub clock: ClockConfig,
  pub media: MediaConfig,
  /// Month grid. Disabled by default so an existing theme is unchanged.
  pub calendar: CalendarConfig,
  /// Local battery indicator. Disabled by default.
  pub battery: BatteryConfig,
  /// Local system monitor. Disabled by default.
  pub system: SystemConfig,
  /// Ambient procedural emitters. Disabled by default.
  pub particles: ParticlesConfig,
  /// The kinetic marquee. Disabled by default.
  pub ticker: TickerConfig,
  pub background: BackgroundConfig,
  pub theme: ThemeConfig,
  pub motion: MotionConfig,
  pub power: PowerConfig,
  /// Adapter, backend, and fallback policy for the live GPU compositor.
  pub gpu: GpuConfig,
  /// Audio policy from the host plus the selected theme's sound content.
  pub audio: AudioConfig,
  /// Opt-in streaks, milestones, drops, and achievements.
  pub rituals: RitualsConfig,
  /// Dragging and where the dragged positions are remembered.
  pub layout: LayoutConfig,
  pub sessions: SessionsConfig,
  pub plugins: PluginsConfig,
  pub auth: AuthConfig,
}

#[derive(Debug, Clone)]
pub struct UiConfig {
  /// Font family name. Empty or unavailable names fall back to the embedded
  /// Inter family, so text is always readable.
  pub font: String,
  pub font_size: f32,
  /// Kept for older configs. New configs should use `login.width`.
  pub card_width: f32,
  pub fullscreen: bool,
  /// Kept for older configs. New configs should use `login.greeting`.
  pub greeting: String,
}

#[derive(Debug, Clone)]
pub struct SessionsConfig {
  pub dirs: Vec<PathBuf>,
  /// Desktop file id to preselect. Unknown ids fall back to the first session.
  pub default: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginsConfig {
  pub enabled: bool,
  pub dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
  /// Empty means "use GREETD_SOCK".
  pub socket: Option<PathBuf>,
  pub timeout: Duration,
}

/// Returns the system plugins directory plus the user's, when HOME exists.
/// Missing HOME is not an error: a greeter running as root with a minimal
/// environment should simply not look in a nonexistent directory.
fn default_plugin_dirs() -> Vec<PathBuf> {
  let mut dirs = vec![PathBuf::from("/usr/share/dawn/plugins")];
  if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
    dirs.push(PathBuf::from(home).join(".config/dawn/plugins"));
  }
  dirs
}

impl Default for Config {
  fn default() -> Self {
    let theme = ThemePreset::GlassDark.tokens();
    Self {
      ui: UiConfig {
        font: "Inter".to_string(),
        font_size: 17.0,
        card_width: 380.0,
        fullscreen: true,
        greeting: String::new(),
      },
      login: LoginConfig {
        layout: LoginLayout::Card,
        placement: Placement {
          anchor: Anchor::Center,
          offset: (0, 0),
          monitor: None,
          rotation: 0.0,
        },
        width: 380.0,
        height: 0.0,
        padding: 26.0,
        gap: 14.0,
        radius: 18.0,
        panel: None,
        panel_fade: (1.0, 1.0),
        border: None,
        surface: SurfaceStyle::Glass,
        show_shadow: true,
        show_border: true,
        rotation: 0.0,
        draggable: false,
        greeting: String::new(),
        show_hints: true,
        show_picker: true,
        picker_title: "Choose a session".to_string(),
        busy_text: "Signing in\u{2026}".to_string(),
        dry_run_badge: "DRY RUN".to_string(),
        show_dry_run_badge: true,
        no_sessions: "No desktop sessions found. Install one or set [sessions].dirs.".to_string(),
        prefill: UsernamePrefill::Auto,
        field: FieldConfig::default_username(),
        username: FieldConfig::default_username(),
        password: FieldConfig::default_password(),
        session: SessionRowConfig {
          label: "Session".to_string(),
          change_hint: "Tab to change".to_string(),
          empty: "No sessions found".to_string(),
          show: true,
        },
        hints: HintConfig {
          username: "Enter  Continue      Tab  Password      Esc  Clear".to_string(),
          password: "Enter  Sign in      Tab  Session      Esc  Clear".to_string(),
          picker: "Up/Down  Choose      Enter  Select      Esc  Cancel".to_string(),
          busy: String::new(),
        },
        tab: String::new(),
        form_offset: (0, 0),
        form_width: 0.0,
        button: ButtonConfig::default(),
        splash: SplashConfig::default(),
        decor: Vec::new(),
        transition: TransitionConfig::default(),
      },
      clock: ClockConfig {
        enabled: true,
        style: ClockStyle::Digital,
        layer: WidgetLayer::Behind,
        look: WidgetLook::default(),
        time_format: "%H:%M".to_string(),
        date_format: "%A, %B %d".to_string(),
        hour12: false,
        seconds: false,
        timezone: None,
        posix_locale: false,
        placement: Placement {
          anchor: Anchor::TopCenter,
          offset: (0, 48),
          monitor: None,
          rotation: 0.0,
        },
        size: 54.0,
        date_size: 18.0,
        color: None,
        date_color: None,
        label: String::new(),
        label_color: None,
        label_size: 0.0,
        label_tracking: 0.0,
        ghost: false,
        glow: None,
        glow_size: 0.0,
        colon_blink_ms: 0.0,
        second_hand: SecondHand::Tick,
        draggable: true,
      },
      media: MediaConfig {
        enabled: true,
        placement: Placement {
          anchor: Anchor::BottomLeft,
          offset: (32, 32),
          monitor: None,
          rotation: 0.0,
        },
        size: 16.0,
        show_art: true,
        look: MediaLook::default(),
        layer: WidgetLayer::Behind,
        presentation: WidgetLook::default(),
        color: None,
        show_before_login: true,
        keep_last: true,
        draggable: true,
      },
      calendar: CalendarConfig {
        enabled: false,
        placement: Placement {
          anchor: Anchor::TopRight,
          offset: (40, 40),
          monitor: None,
          rotation: 0.0,
        },
        size: 13.0,
        width: 260.0,
        week_start: WeekStart::Monday,
        show_header: true,
        color: Rgb::solid(0xF7, 0xF3, 0xE9),
        muted_color: Rgb::solid(0x8A, 0x8D, 0x80),
        today_color: Rgb::solid(0xE2, 0x73, 0x4F),
        header_color: Rgb::solid(0xE7, 0xDA, 0x19),
        layer: WidgetLayer::Behind,
        look: WidgetLook::default(),
        draggable: true,
      },
      battery: BatteryConfig {
        enabled: false,
        placement: Placement {
          anchor: Anchor::BottomRight,
          offset: (40, 40),
          monitor: None,
          rotation: 0.0,
        },
        size: 14.0,
        width: 220.0,
        style: BatteryStyle::Ring,
        color: Rgb::solid(0xF7, 0xF3, 0xE9),
        low_color: Rgb::solid(0xE2, 0x73, 0x4F),
        charging_color: Rgb::solid(0xC6, 0xD6, 0x2B),
        low_at: 20,
        show_percent: true,
        layer: WidgetLayer::Behind,
        look: WidgetLook::default(),
        draggable: true,
      },
      system: SystemConfig {
        enabled: false,
        placement: Placement {
          anchor: Anchor::TopLeft,
          offset: (40, 40),
          monitor: None,
          rotation: 0.0,
        },
        size: 12.0,
        width: 280.0,
        style: SystemStyle::Rows,
        fields: vec![
          SystemField::Hostname,
          SystemField::Kernel,
          SystemField::Uptime,
          SystemField::Load,
          SystemField::Memory,
        ],
        color: Rgb::solid(0xF7, 0xF3, 0xE9),
        label_color: Rgb::solid(0x4E, 0x51, 0x48),
        layer: WidgetLayer::Behind,
        look: WidgetLook::default(),
        draggable: true,
      },
      particles: ParticlesConfig::default(),
      ticker: TickerConfig::default(),
      background: BackgroundConfig {
        kind: BackgroundKind::Auto,
        color: theme.preset.background_color(),
        gradient: true,
        image: None,
        audio: false,
        fit: Fit::Cover,
        // Zero means "follow the source". A positive value is still honoured
        // as a cap for people who want to save power on a slow panel.
        fps: 0,
        memory_limit: 128 * 1024 * 1024,
        idle_pause: Duration::from_secs(45),
        monitors: BTreeMap::new(),
      },
      theme,
      motion: MotionConfig::default(),
      power: PowerConfig::default(),
      gpu: GpuConfig::default(),
      audio: AudioConfig::default(),
      rituals: RitualsConfig::default(),
      layout: LayoutConfig {
        save: false,
        file: None,
      },
      sessions: SessionsConfig {
        dirs: vec![
          PathBuf::from("/usr/share/wayland-sessions"),
          PathBuf::from("/usr/share/xsessions"),
        ],
        default: None,
      },
      plugins: PluginsConfig {
        enabled: true,
        dirs: default_plugin_dirs(),
      },
      auth: AuthConfig {
        socket: None,
        timeout: Duration::from_secs(120),
      },
    }
  }
}
