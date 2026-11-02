//! GPU adapter, backend, and presentation policy.
//!
//! This is host policy, not theme data. Selecting a theme must not change the
//! adapter a machine boots on.

/// Which graphics API the live compositor may use.
///
/// This is deliberately separate from the software compositor used for
/// screenshots and the terminal fallback: `[gpu]` only influences the window's
/// GPU path. A driver that enumerates but cannot present still falls back to
/// softbuffer, so a bad value here can never stop a login.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuBackend {
  /// Try Vulkan and then GL, whichever can present to the window.
  #[default]
  Auto,
  /// Vulkan only. Useful when a broken GL driver wins the automatic order.
  Vulkan,
  /// OpenGL only. The usual rescue on older hardware and some remote sessions.
  Gl,
  /// Keep the GPU pipeline but restrict it to CPU adapters such as lavapipe or
  /// llvmpipe. The caller still falls back to softbuffer if none is present.
  Software,
}

impl GpuBackend {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "auto" => Some(Self::Auto),
      "vulkan" | "vk" => Some(Self::Vulkan),
      "gl" | "opengl" | "gles" => Some(Self::Gl),
      "software" | "sw" => Some(Self::Software),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Auto => "auto",
      Self::Vulkan => "vulkan",
      Self::Gl => "gl",
      Self::Software => "software",
    }
  }
}

/// Which class of adapter the compositor should prefer.
///
/// `Auto` keeps every surface-capable adapter. The other values are a hard
/// filter, not a hint: `device = "discrete"` on a laptop with only integrated
/// graphics produces a readable error instead of silently using the iGPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuDevice {
  /// Keep every surface-capable adapter and let `power` decide the order.
  #[default]
  Auto,
  /// A graphics card with its own memory, usually the fastest option.
  Discrete,
  /// Shared-memory graphics built into the CPU package.
  Integrated,
  /// A virtualised or hosted GPU, common inside VMs.
  Virtual,
  /// A CPU implementation such as lavapipe or llvmpipe.
  Cpu,
}

impl GpuDevice {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "auto" => Some(Self::Auto),
      "discrete" => Some(Self::Discrete),
      "integrated" => Some(Self::Integrated),
      "virtual" => Some(Self::Virtual),
      "cpu" | "software" => Some(Self::Cpu),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Auto => "auto",
      Self::Discrete => "discrete",
      Self::Integrated => "integrated",
      Self::Virtual => "virtual",
      Self::Cpu => "cpu",
    }
  }
}

/// How the compositor ranks adapters when `adapter = "auto"`.
///
/// Low power is the default because a greeter spends almost all its life idle.
/// It is a preference, not a filter: when the preferred class is missing the
/// next one in the ranking is still used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuPower {
  /// Prefer integrated, then other, virtual, discrete, and finally CPU.
  #[default]
  Low,
  /// Prefer discrete, then integrated, virtual, other, and finally CPU.
  High,
  /// Keep the order the backend enumerated, which is useful for debugging.
  Default,
}

impl GpuPower {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "low" => Some(Self::Low),
      "high" => Some(Self::High),
      "default" => Some(Self::Default),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Low => "low",
      Self::High => "high",
      Self::Default => "default",
    }
  }
}

/// The swapchain presentation mode requested from the surface.
///
/// `Fifo` is the only mode every backend must support, so it is the default.
/// A mode the adapter does not advertise is ignored with a warning rather than
/// turning into a startup failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuPresent {
  /// Vsync-style queueing with no tearing.
  #[default]
  Fifo,
  /// Tear-free but replaces the queued frame instead of blocking, when offered.
  Mailbox,
  /// Present immediately and allow tearing, when offered.
  Immediate,
}

impl GpuPresent {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "fifo" | "vsync" => Some(Self::Fifo),
      "mailbox" => Some(Self::Mailbox),
      "immediate" | "tearing" => Some(Self::Immediate),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Fifo => "fifo",
      Self::Mailbox => "mailbox",
      Self::Immediate => "immediate",
    }
  }
}

/// What to do when the requested GPU path cannot start.
///
/// The default chain is the safe policy for a greeter: try the configured
/// adapter, then the software adapter, then softbuffer. `None` is for admins
/// who would rather drop to the terminal than show a CPU-painted window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuFallback {
  /// Configured adapter, then software GPU, then softbuffer.
  #[default]
  Chain,
  /// Configured adapter, then software GPU, then fail to the terminal.
  Software,
  /// Skip the GPU entirely and use softbuffer immediately.
  Cpu,
  /// No fallback: a GPU failure goes straight to the terminal fallback.
  None,
}

impl GpuFallback {
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "chain" => Some(Self::Chain),
      "software" => Some(Self::Software),
      "cpu" => Some(Self::Cpu),
      "none" => Some(Self::None),
      _ => None,
    }
  }

  #[must_use]
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Chain => "chain",
      Self::Software => "software",
      Self::Cpu => "cpu",
      Self::None => "none",
    }
  }
}

/// How the live window chooses and initialises its GPU compositor.
///
/// Screenshots and the CPU compositor still run on tiny-skia, so this table
/// never changes the pixels in a `--screenshot` file. It only decides which
/// adapter the interactive window uses and what happens when that choice is
/// unavailable at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuConfig {
  /// Set false to use softbuffer without touching a driver. `DAWN_GPU=0` is
  /// the environment equivalent.
  pub enabled: bool,
  /// Graphics API to enable during adapter enumeration.
  pub backend: GpuBackend,
  /// `"auto"`, a class keyword (`"integrated"`, `"discrete"`, `"software"`),
  /// `"index:N"` into the filtered adapter list, or a case-insensitive
  /// substring of the adapter name or driver.
  pub adapter: String,
  /// Adapter class filter. `backend = "software"` forces this to `Cpu`, and
  /// the `adapter` class keywords override it as the more specific request.
  pub device: GpuDevice,
  /// Ranking preference when the adapter is `auto`. The `integrated` and
  /// `discrete` keywords set this instead.
  pub power: GpuPower,
  /// Requested swapchain mode. Unsupported modes fall back without failing.
  pub present: GpuPresent,
  /// What to try when the selected adapter cannot produce a device or surface.
  pub fallback: GpuFallback,
}

impl Default for GpuConfig {
  fn default() -> Self {
    Self {
      enabled: true,
      backend: GpuBackend::Auto,
      adapter: "auto".to_string(),
      device: GpuDevice::Auto,
      power: GpuPower::Low,
      present: GpuPresent::Fifo,
      fallback: GpuFallback::Chain,
    }
  }
}

impl GpuConfig {
  /// Applies one `DAWN_GPU` value without touching the process environment.
  ///
  /// This exists as a pure method because `std::env::set_var` is unsafe in the
  /// 2024 edition and tests should not race the rest of the test binary. The
  /// accepted spellings are intentionally small: `0`/`cpu` disables the GPU,
  /// a backend name selects that backend, `auto` clears a previous adapter
  /// choice, and anything else is treated as an adapter name.
  pub fn apply_env_value(&mut self, value: Option<&str>) {
    let Some(value) = value else {
      return;
    };
    let value = value.trim();
    if value.is_empty() {
      return;
    }
    if value == "0" || value.eq_ignore_ascii_case("cpu") {
      self.enabled = false;
      return;
    }
    if value.eq_ignore_ascii_case("auto") {
      self.adapter = "auto".to_string();
      return;
    }
    let normalised = value.to_ascii_lowercase();
    if let Some(backend) = GpuBackend::parse(&normalised) {
      self.backend = backend;
      if backend == GpuBackend::Software {
        self.device = GpuDevice::Cpu;
        self.fallback = GpuFallback::Software;
      }
      return;
    }
    self.adapter = value.to_string();
  }

  /// Reads `DAWN_GPU` once and applies it with [`Self::apply_env_value`].
  ///
  /// Non-Unicode values are ignored on purpose: a broken environment variable
  /// should not keep the greeter from starting.
  pub fn apply_env(&mut self) {
    let value = std::env::var("DAWN_GPU").ok();
    self.apply_env_value(value.as_deref());
  }

  /// One-line lowercase summary shared by `--check-config` and `--list-gpus`.
  /// Using the same method in both places keeps the admin-facing spelling in
  /// sync with the values accepted in TOML.
  #[must_use]
  pub fn summary(&self) -> String {
    format!(
      "enabled={} backend={} adapter={} device={} power={} present={} fallback={}",
      self.enabled,
      self.backend.as_str(),
      self.adapter,
      self.device.as_str(),
      self.power.as_str(),
      self.present.as_str(),
      self.fallback.as_str(),
    )
  }
}
