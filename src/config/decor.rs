//! Decorative login items and their motion model.

use super::*;

/// One cosmetic item inside the login card.
#[derive(Debug, Clone)]
pub enum DecorSpec {
  Text {
    text: String,
    size: f32,
    color: Option<Rgb>,
    bold: bool,
    tracking: f32,
    /// Optional outline colour. When set, the run is drawn around a small
    /// radius before the fill pass, which keeps a thin HUD label readable over
    /// busy artwork.
    stroke: Option<Rgb>,
    /// Outline radius in logical pixels, 0 to 12. A set `stroke` with a zero
    /// width is resolved to 2 so the option never silently does nothing.
    stroke_width: f32,
    anchor: Anchor,
    offset: (i32, i32),
    rotation: f32,
    motion: DecorMotion,
    /// Multiply this item's scale by a small popping factor on each beat of
    /// `[motion.beat]`. Off by default so the beat clock alone changes nothing.
    pulse_on_beat: bool,
  },
  Image {
    path: PathBuf,
    width: f32,
    anchor: Anchor,
    offset: (i32, i32),
    opacity: f32,
    rotation: f32,
    motion: DecorMotion,
    pulse_on_beat: bool,
  },
  /// An anchored plate behind or over other decor. Unlike an image it needs no
  /// asset; the colour falls back to the theme tint. `color_end` turns the fill
  /// into a linear gradient at `gradient_angle`, `border` adds a one-pixel
  /// stroke, and `glass` paints the cached backdrop underneath. Typewriter and
  /// ticker motion do not apply, but blink still does.
  Panel {
    width: f32,
    height: f32,
    color: Option<Rgb>,
    color_end: Option<Rgb>,
    gradient_angle: f32,
    radius: f32,
    border: Option<Rgb>,
    opacity: f32,
    /// Paint the shared blurred backdrop under the fill. With a translucent
    /// `color` this is the same frosted plate the login card uses; with no
    /// colour it still falls back to the theme tint, so an author who wants to
    /// see the blur has to set an alpha colour.
    glass: bool,
    anchor: Anchor,
    offset: (i32, i32),
    rotation: f32,
    motion: DecorMotion,
    pulse_on_beat: bool,
  },
}

/// How a decor item enters the screen on the first frames.
///
/// This is a one-shot choreography, not a loop. At `elapsed = 0` the item is
/// still in its hidden pose, so a screenshot captures the start of the entrance
/// rather than the settled widget. That is deliberate: the same frame can be
/// compared across runs without waiting for time to pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnterStyle {
  /// No entrance. The item is fully visible immediately.
  #[default]
  None,
  /// Fade from transparent to the configured opacity.
  Fade,
  /// Slide in from the side named by `enter_from`.
  Slide,
  /// Grow from 92 percent of the base scale to the full size.
  Scale,
}

impl EnterStyle {
  /// Parses the TOML spelling. Unknown values return `None` so the validator
  /// can report the exact key and fall back to [`EnterStyle::None`].
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "none" => Some(Self::None),
      "fade" => Some(Self::Fade),
      "slide" => Some(Self::Slide),
      "scale" => Some(Self::Scale),
      _ => None,
    }
  }
}

/// Which edge a [`EnterStyle::Slide`] entrance starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EnterFrom {
  /// Start offset to the left.
  #[default]
  Left,
  /// Start offset to the right.
  Right,
  /// Start offset above the final position.
  Top,
  /// Start offset below the final position.
  Bottom,
}

impl EnterFrom {
  /// Parses the TOML spelling. Unknown values return `None` so the validator
  /// can report the exact key and fall back to [`EnterFrom::Left`].
  #[must_use]
  pub fn parse(value: &str) -> Option<Self> {
    match value {
      "left" => Some(Self::Left),
      "right" => Some(Self::Right),
      "top" => Some(Self::Top),
      "bottom" => Some(Self::Bottom),
      _ => None,
    }
  }
}

/// Cubic ease-out used by the entrance. It starts fast and settles gently,
/// which reads as a deliberate arrival rather than a linear push. This lives
/// in config rather than the UI so the sample stays a pure value calculation.
fn ease_out_cubic(t: f32) -> f32 {
  1.0 - (1.0 - t).powi(3)
}

/// How a decor item moves. All values are optional in TOML; an item with no
/// motion table stays completely static.
///
/// The transform motions are deliberately additive so one ornament can spin,
/// drift, and breathe at the same time. `phase` offsets the whole pattern in
/// turns, which is how a row of particles avoids moving in lockstep. The
/// clock-like fields have two scopes: `blink_ms` gates both text and image
/// decor, while `typer_ms`, `scramble_ms`, `wave`, `glitch`, and the ticker
/// pair only apply to text; images ignore those.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecorMotion {
  /// Continuous rotation in degrees per second.
  pub spin: f32,
  /// Sine-wave rotation amplitude in degrees.
  pub sway: f32,
  /// Period of `sway` in milliseconds.
  pub sway_ms: f32,
  /// Elliptical drift amplitude in logical pixels. X uses sine, Y uses cosine.
  pub float: (i32, i32),
  /// Period of `float` in milliseconds.
  pub float_ms: f32,
  /// Scale pulse as a fraction of the base size, 0 to 0.5.
  pub pulse: f32,
  /// Period of `pulse` in milliseconds.
  pub pulse_ms: f32,
  /// Opacity fade depth, 0 to 1. The item cycles between full and `1 - fade`.
  pub fade: f32,
  /// Period of `fade` in milliseconds.
  pub fade_ms: f32,
  /// Phase offset in turns, 0 to 1.
  pub phase: f32,
  /// One-shot character reveal time for text decor, in milliseconds. Zero
  /// disables it. The reveal is linear, because a typewriter should not ease:
  /// easing makes the rhythm feel like it stutters near the end.
  pub typer_ms: f32,
  /// Square-wave blink period for text and image decor, in milliseconds. Zero
  /// disables it. The first half of each period is visible, the second half is
  /// not, which is the classic terminal cursor rhythm rather than a smooth
  /// fade.
  pub blink_ms: f32,
  /// Time for a ticker to travel one full tile, in milliseconds. Zero disables
  /// scrolling. A ticker overrides the typewriter and the scramble on the same
  /// item, since a marquee is already showing the whole run.
  pub ticker_ms: f32,
  /// Width of the ticker strip in logical pixels. Zero leaves the ticker off
  /// even when `ticker_ms` is set, because there is nowhere to draw it.
  pub ticker_width: i32,
  /// Space between two copies of the ticker text, in logical pixels. The gap
  /// is part of the tile, so the loop stays seamless.
  pub ticker_gap: i32,
  /// How strongly the pointer shifts this item, 0 to 1. The actual travel also
  /// depends on `[motion].parallax`, so either value at zero keeps the item
  /// still. Parallax only does anything in a live window with a pointer.
  pub parallax_depth: f32,
  /// One-shot entrance style. See [`EnterStyle`].
  pub enter: EnterStyle,
  /// Duration of the entrance in milliseconds, 0 to 20000.
  pub enter_ms: f32,
  /// Delay before the entrance starts in milliseconds, 0 to 20000.
  pub enter_delay_ms: f32,
  /// Edge the slide entrance starts from. Ignored by the other styles.
  pub enter_from: EnterFrom,
  /// Vertical wave amplitude in logical pixels, 0 to 40. Text only. Each
  /// character rides a sine offset by `wave_step` turns from the previous one.
  pub wave: f32,
  /// Period of the character wave in milliseconds, 200 to 20000.
  pub wave_ms: f32,
  /// Phase step between adjacent characters in turns, 0 to 1. Around 0.12
  /// gives a visible ripple without making the run look broken.
  pub wave_step: f32,
  /// Text reveal time in milliseconds, 0 to 20000. Zero is off. A positive
  /// value overrides `typer_ms` on the same item, and a ticker overrides both.
  /// At `elapsed = 0` the final text is shown, so screenshots stay readable.
  pub scramble_ms: f32,
  /// Glitch strength, 0 to 1. Zero is off. Text only. The effect shows two
  /// coloured copies of the run for a short slice of each `glitch_ms` period.
  pub glitch: f32,
  /// Period of the glitch window in milliseconds, 200 to 20000.
  pub glitch_ms: f32,
}

impl Default for DecorMotion {
  fn default() -> Self {
    Self {
      spin: 0.0,
      sway: 0.0,
      sway_ms: 6000.0,
      float: (0, 0),
      float_ms: 7000.0,
      pulse: 0.0,
      pulse_ms: 2400.0,
      fade: 0.0,
      fade_ms: 3000.0,
      phase: 0.0,
      typer_ms: 0.0,
      blink_ms: 0.0,
      ticker_ms: 0.0,
      ticker_width: 0,
      ticker_gap: 64,
      parallax_depth: 0.0,
      enter: EnterStyle::None,
      enter_ms: 600.0,
      enter_delay_ms: 0.0,
      enter_from: EnterFrom::Left,
      wave: 0.0,
      wave_ms: 2400.0,
      wave_step: 0.12,
      scramble_ms: 0.0,
      glitch: 0.0,
      glitch_ms: 2000.0,
    }
  }
}

/// The values one decor item uses at a single instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecorSample {
  /// Drift from the configured anchor, in logical pixels.
  pub offset: (f32, f32),
  /// Additional rotation from `spin` and `sway`, in degrees.
  pub angle: f32,
  /// Scale multiplier around the item centre.
  pub scale: f32,
  /// Opacity multiplier from `fade`, 0 to 1.
  pub opacity: f32,
  /// Portion of the text visible from the typewriter, 0 to 1. It is 1 when
  /// `typer_ms` is off, so a static item never looks half-written.
  pub reveal: f32,
  /// Whether the blink wave is in its visible half. It is true when blink is
  /// off, which keeps the common case free of an extra branch in the renderer.
  pub blink_on: bool,
  /// Fraction of one ticker tile already travelled, 0 to 1. Zero when the
  /// ticker is off.
  pub loop_frac: f32,
  /// Age of the sample in seconds after clamping. Negative or non-finite input
  /// becomes zero, so every time-based effect has a safe neutral pose.
  pub elapsed: f32,
}

impl DecorMotion {
  /// True when any component can change over time.
  #[must_use]
  pub fn active(self) -> bool {
    self.spin.abs() > f32::EPSILON
      || self.sway > 0.0
      || self.float != (0, 0)
      || self.pulse > 0.0
      || self.fade > 0.0
      || self.typer_ms > 0.0
      || self.blink_ms > 0.0
      || (self.ticker_ms > 0.0 && self.ticker_width > 0)
      || self.parallax_depth > 0.0
      || self.enter != EnterStyle::None
      || self.wave > 0.0
      || self.scramble_ms > 0.0
      || self.glitch > 0.0
  }

  /// Samples the motion at `elapsed` seconds since the UI started.
  ///
  /// The looping formulas are simple on purpose: phase 0 starts at the neutral
  /// position, so a screenshot (`elapsed = 0`) matches the first live frame.
  /// The entrance is the exception: it starts hidden at zero and settles after
  /// `enter_delay_ms + enter_ms`, because an entrance that was already finished
  /// would not be an entrance at all.
  #[must_use]
  pub fn sample(self, elapsed: f32) -> DecorSample {
    if !elapsed.is_finite() {
      return DecorSample {
        offset: (0.0, 0.0),
        angle: 0.0,
        scale: 1.0,
        opacity: 1.0,
        reveal: 1.0,
        blink_on: true,
        loop_frac: 0.0,
        elapsed: 0.0,
      };
    }
    let turns = elapsed.max(0.0);
    let ms = turns * 1000.0;
    let phase = self.phase.rem_euclid(1.0) * std::f32::consts::TAU;
    let wave = |period_ms: f32| std::f32::consts::TAU * turns / (period_ms.max(1.0) / 1000.0);
    // Every oscillation subtracts its value at phase alone, so `elapsed = 0`
    // is the neutral pose no matter how the item is phase-shifted. Without
    // that, a staggered row would start mid-stride and screenshots would
    // disagree with the first live frame.
    let float_x = self.float.0 as f32 * ((wave(self.float_ms) + phase).sin() - phase.sin());
    let float_y = self.float.1 as f32 * ((wave(self.float_ms) + phase).cos() - phase.cos());
    let angle = self.spin * turns + self.sway * ((wave(self.sway_ms) + phase).sin() - phase.sin());
    let scale = 1.0 + self.pulse * ((wave(self.pulse_ms) + phase).sin() - phase.sin());
    let rest = 0.5 - 0.5 * phase.cos();
    let opacity = 1.0 - self.fade * ((0.5 - 0.5 * (wave(self.fade_ms) + phase).cos()) - rest);
    let enter_t = if self.enter == EnterStyle::None {
      1.0
    } else {
      let t = ((ms - self.enter_delay_ms) / self.enter_ms.max(1.0)).clamp(0.0, 1.0);
      ease_out_cubic(t)
    };
    let mut offset = (float_x, float_y);
    let mut scale = scale;
    let mut opacity = opacity;
    match self.enter {
      EnterStyle::None => {}
      EnterStyle::Fade => opacity *= enter_t,
      EnterStyle::Scale => scale *= 0.92 + 0.08 * enter_t,
      EnterStyle::Slide => {
        let d = 28.0 * (1.0 - enter_t);
        match self.enter_from {
          EnterFrom::Left => offset.0 -= d,
          EnterFrom::Right => offset.0 += d,
          EnterFrom::Top => offset.1 -= d,
          EnterFrom::Bottom => offset.1 += d,
        }
      }
    }
    DecorSample {
      offset,
      angle,
      scale: scale.clamp(0.1, 2.0),
      opacity: opacity.clamp(0.0, 1.0),
      reveal: if self.typer_ms <= 0.0 {
        1.0
      } else {
        (ms / self.typer_ms).clamp(0.0, 1.0)
      },
      blink_on: if self.blink_ms <= 0.0 {
        true
      } else {
        ((ms / self.blink_ms) as u64).is_multiple_of(2)
      },
      loop_frac: if self.ticker_ms <= 0.0 {
        0.0
      } else {
        (ms / self.ticker_ms) % 1.0
      },
      elapsed: turns,
    }
  }
}
