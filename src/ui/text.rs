//! Text shaping, layout, and rasterisation.
//!
//! cosmic-text handles the hard parts (bidirectional text, font fallback,
//! shaping, grayscale rasterisation). This wrapper adds the three things the
//! greeter needs on top: an embedded fallback font so text is never invisible,
//! a clip rectangle so an overlong username cannot paint over the card, and a
//! height measurement so the card layout can grow for wrapped PAM messages.
//!
//! We load system fonts as well as the embedded Inter face. The embedded face
//! guarantees readable Latin text on a minimal boot environment; system fonts
//! cover other scripts.

use std::collections::HashMap;

use cosmic_text::{
  Align, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight, Wrap,
};

use crate::color::Rgb;
use crate::ui::canvas::{Canvas, Rect};

const EMBEDDED_FONT: &[u8] = include_bytes!("../../assets/InterVariable.ttf");
const FALLBACK_FAMILY: &str = "Inter";

/// Size, colour, weight, and tracking of one text run.
#[derive(Debug, Clone, Copy)]
pub struct Style {
  pub size: f32,
  pub color: Rgb,
  pub weight: Weight,
  /// Letter spacing in em. Zero keeps the font's natural tracking.
  pub tracking: f32,
}

impl Style {
  #[must_use]
  pub const fn new(size: f32, color: Rgb) -> Self {
    Self {
      size,
      color,
      weight: Weight::NORMAL,
      tracking: 0.0,
    }
  }

  /// Bold text, used by the hero submit button and any themed headings.
  #[must_use]
  pub const fn bold(mut self) -> Self {
    self.weight = Weight::BOLD;
    self
  }

  /// Widen the letters, in em. Short, uppercase button copy is tracked out.
  #[must_use]
  pub const fn tracking(mut self, tracking: f32) -> Self {
    self.tracking = tracking;
    self
  }
}

/// Where a text run goes. Grouping the origin, wrap width, alignment, and clip
/// keeps the drawing calls readable and stops them from growing an unwieldy
/// argument list.
#[derive(Debug, Clone, Copy)]
pub struct Area {
  pub x: f32,
  pub y: f32,
  pub width: f32,
  pub align: Align,
  pub clip: Option<Rect>,
}

impl Area {
  #[must_use]
  pub const fn new(x: f32, y: f32, width: f32) -> Self {
    Self {
      x,
      y,
      width,
      align: Align::Left,
      clip: None,
    }
  }

  #[must_use]
  pub const fn align(mut self, align: Align) -> Self {
    self.align = align;
    self
  }

  #[must_use]
  pub const fn clip(mut self, clip: Rect) -> Self {
    self.clip = Some(clip);
    self
  }
}

/// Owns the font database and glyph cache. Creating this is the expensive part
/// of startup, so it is built once and reused for every frame.
pub struct TextEngine {
  font_system: FontSystem,
  swash_cache: SwashCache,
  family: String,
  /// Ink bounds are requested every frame for the fields and the button, so a
  /// small cache avoids re-shaping the same placeholder twice.
  ink_cache: HashMap<(String, u32, u32, u32), (f32, f32)>,
}

impl std::fmt::Debug for TextEngine {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("TextEngine")
      .field("family", &self.family)
      .finish_non_exhaustive()
  }
}

impl TextEngine {
  /// Loads system fonts plus the embedded face and resolves the configured
  /// family. An unknown family falls back to the embedded Inter rather than
  /// disappearing, and the caller gets the resolved name back for logging.
  #[must_use]
  pub fn new(preferred: &str) -> Self {
    let mut font_system = FontSystem::new();
    font_system.db_mut().load_font_data(EMBEDDED_FONT.to_vec());
    let family = if preferred.is_empty() || !family_exists(&font_system, preferred) {
      FALLBACK_FAMILY.to_string()
    } else {
      preferred.to_string()
    };
    Self {
      font_system,
      swash_cache: SwashCache::new(),
      family,
      ink_cache: HashMap::new(),
    }
  }

  /// The family that will actually be used.
  #[must_use]
  pub fn family(&self) -> &str {
    &self.family
  }

  /// Draws wrapped text and returns the height it occupied.
  pub fn draw(&mut self, canvas: &mut Canvas, text: &str, style: Style, area: Area) -> f32 {
    self.draw_inner(canvas, text, style, area, true)
  }

  /// Draws a single line without wrapping. Used for input fields, where a line
  /// break would move the cursor somewhere unexpected.
  pub fn draw_line(&mut self, canvas: &mut Canvas, text: &str, style: Style, area: Area) -> f32 {
    self.draw_inner(canvas, text, style, area, false)
  }

  /// Measures wrapped text without painting it. The card layout uses this to
  /// decide how tall it needs to be before anything is drawn.
  pub fn measure(&mut self, text: &str, size: f32, max_width: f32, wrap: bool) -> (f32, f32) {
    if text.is_empty() || max_width <= 0.0 {
      return (0.0, line_height(size));
    }
    let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(size, line_height(size)));
    buffer.set_size(Some(max_width.max(1.0)), None);
    buffer.set_wrap(if wrap { Wrap::WordOrGlyph } else { Wrap::None });
    let attrs = self.attrs(Weight::NORMAL, 0.0);
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut self.font_system, false);
    let mut width: f32 = 0.0;
    let mut height: f32 = 0.0;
    for run in buffer.layout_runs() {
      width = width.max(run.line_w);
      height = height.max(run.line_top + run.line_height);
    }
    (width, height.max(line_height(size)))
  }

  /// Vertical ink bounds of a single line, relative to the text area's top.
  ///
  /// This exists because a line box is not the same thing as the pixels a
  /// font actually paints. A 17 px D-DIN line box is 23 px tall, but a
  /// placeholder's ink is only about 12 px, sitting slightly above the box
  /// centre. Centring the box therefore leaves the text visibly higher than a
  /// centred icon. Measuring the glyph images is the only way to align the two
  /// optically, and it keeps working when the family changes.
  pub fn ink_bounds(&mut self, text: &str, size: f32, weight: Weight, tracking: f32) -> (f32, f32) {
    if text.is_empty() || size <= 0.0 {
      return (0.0, line_height(size));
    }
    let key = (
      text.to_string(),
      size.to_bits(),
      u32::from(weight.0),
      tracking.to_bits(),
    );
    if let Some(bounds) = self.ink_cache.get(&key) {
      return *bounds;
    }

    let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(size, line_height(size)));
    buffer.set_size(Some(f32::MAX / 4.0), None);
    buffer.set_wrap(Wrap::None);
    let attrs = self.attrs(weight, tracking);
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut self.font_system, false);

    let mut glyphs = Vec::new();
    for run in buffer.layout_runs() {
      for glyph in run.glyphs {
        glyphs.push(glyph.physical((0.0, run.line_y), 1.0));
      }
    }
    drop(buffer);

    let mut top = f32::MAX;
    let mut bottom = f32::MIN;
    for glyph in glyphs {
      if let Some(image) = self
        .swash_cache
        .get_image(&mut self.font_system, glyph.cache_key)
      {
        let glyph_top = glyph.y as f32 - image.placement.top as f32;
        let glyph_bottom = glyph_top + image.placement.height as f32;
        top = top.min(glyph_top);
        bottom = bottom.max(glyph_bottom);
      }
    }
    let bounds = if top.is_finite() && bottom.is_finite() && bottom > top {
      (top.max(0.0), bottom)
    } else {
      (0.0, line_height(size))
    };
    if self.ink_cache.len() >= 256 {
      self.ink_cache.clear();
    }
    self.ink_cache.insert(key, bounds);
    bounds
  }

  /// Width of a single line. Used to place the text cursor.
  #[must_use]
  pub fn line_width(&mut self, text: &str, size: f32) -> f32 {
    let line_height = line_height(size);
    let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(size, line_height));
    buffer.set_size(Some(f32::MAX / 4.0), None);
    buffer.set_wrap(Wrap::None);
    let attrs = self.attrs(Weight::NORMAL, 0.0);
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(&mut self.font_system, false);
    buffer.layout_runs().next().map_or(0.0, |run| run.line_w)
  }

  fn draw_inner(
    &mut self,
    canvas: &mut Canvas,
    text: &str,
    style: Style,
    area: Area,
    wrap: bool,
  ) -> f32 {
    let size = style.size;
    if text.is_empty() || size <= 0.0 || area.width <= 0.0 {
      return line_height(size);
    }

    let left = area.x.round() as i32;
    let top = area.y.round() as i32;
    let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(size, line_height(size)));
    buffer.set_size(Some(area.width.max(1.0)), None);
    buffer.set_wrap(if wrap { Wrap::WordOrGlyph } else { Wrap::None });
    let attrs = self.attrs(style.weight, style.tracking);
    buffer.set_text(text, &attrs, Shaping::Advanced, Some(area.align));

    let cosmic_color = to_cosmic(style.color);
    buffer.draw(
      &mut self.font_system,
      &mut self.swash_cache,
      cosmic_color,
      |x, y, width, height, pixel| {
        canvas.blend_text_pixels(left + x, top + y, width, height, pixel.as_rgba(), area.clip);
      },
    );

    buffer
      .layout_runs()
      .map(|run| run.line_top + run.line_height)
      .fold(0.0, f32::max)
  }

  fn attrs(&self, weight: Weight, tracking: f32) -> Attrs<'_> {
    let attrs = Attrs::new()
      .family(Family::Name(&self.family))
      .weight(weight);
    if tracking == 0.0 {
      attrs
    } else {
      attrs.letter_spacing(tracking)
    }
  }
}

fn line_height(size: f32) -> f32 {
  (size * 1.35).max(size + 2.0)
}

fn to_cosmic(color: Rgb) -> cosmic_text::Color {
  cosmic_text::Color::rgba(color.r, color.g, color.b, color.a)
}

fn family_exists(font_system: &FontSystem, wanted: &str) -> bool {
  font_system.db().faces().any(|face| {
    face
      .families
      .iter()
      .any(|(family, _)| family.eq_ignore_ascii_case(wanted))
  })
}
