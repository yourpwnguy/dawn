//! Low-power policy.

#[derive(Debug, Clone, Default)]
pub struct PowerConfig {
  /// Freeze animation and motion. Intended for battery-powered or remote
  /// greeter hardware; it never affects login responsiveness.
  pub low_power: bool,
}
