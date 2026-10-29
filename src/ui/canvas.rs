//! A small drawing surface on top of tiny-skia.
//!
//! Everything the greeter paints goes through here: background gradients,
//! rounded glass panels, plugin images, media art, and text pixels from
//! cosmic-text. The backing pixmap is premultiplied RGBA, which is what
//! tiny-skia produces and what our text blending assumes. `to_argb` is the one
//! place that knows softbuffer's pixel layout.

use tiny_skia::{
  FillRule, FilterQuality, GradientStop, LinearGradient, Mask, Paint, Path, PathBuilder, Pattern,
  Pixmap, PixmapPaint, Point, Rect as SkRect, Shader, SpreadMode, Stroke, Transform,
};

use crate::color::Rgb;

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
  pub x: f32,
  pub y: f32,
  pub w: f32,
  pub h: f32,
}

impl Rect {
  pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
    Self { x, y, w, h }
  }

  pub fn inset(self, amount: f32) -> Self {
    Self {
      x: self.x + amount,
      y: self.y + amount,
      w: (self.w - amount * 2.0).max(0.0),
      h: (self.h - amount * 2.0).max(0.0),
    }
  }

  pub fn right(self) -> f32 {
    self.x + self.w
  }

  pub fn bottom(self) -> f32 {
    self.y + self.h
  }

  pub fn center(self) -> (f32, f32) {
    (self.x + self.w / 2.0, self.y + self.h / 2.0)
  }

  /// Point-in-rectangle test for pointer hit testing.
  #[must_use]
  pub fn contains(self, point: (f32, f32)) -> bool {
    point.0 >= self.x && point.0 <= self.right() && point.1 >= self.y && point.1 <= self.bottom()
  }

  fn to_skia(self) -> Option<SkRect> {
    SkRect::from_xywh(self.x, self.y, self.w, self.h)
  }
}

/// A CPU canvas. `None` is returned for zero-sized surfaces instead of
/// panicking: a compositor is allowed to hand us a 0x0 window while resizing.
///
/// A canvas can also describe a sub-region of the screen. `offset` is the
/// absolute position of the canvas' top-left pixel. Widgets keep drawing with
/// absolute coordinates, so the rotation and effect layers can reuse every
/// existing draw function without passing a translation through each call.
pub struct Canvas {
  pixmap: Pixmap,
  offset: (f32, f32),
  /// Size of the screen this canvas is a part of. A scratch layer is smaller
  /// than the screen, but a cached screen-sized blur must still be scaled by
  /// the screen ratio, not by the layer ratio.
  screen: (u32, u32),
}

impl Canvas {
  #[must_use]
  pub fn new(width: u32, height: u32) -> Option<Self> {
    Pixmap::new(width, height).map(|pixmap| Self {
      pixmap,
      offset: (0.0, 0.0),
      screen: (width, height),
    })
  }

  /// A transparent scratch canvas covering an absolute screen rectangle.
  /// `offset` is the absolute top-left of the region. Rotation layers use it
  /// so their cost is proportional to the widget, not to the whole screen.
  #[must_use]
  pub fn region(width: u32, height: u32, offset: (f32, f32)) -> Option<Self> {
    Pixmap::new(width, height).map(|pixmap| Self {
      pixmap,
      offset,
      screen: (width, height),
    })
  }

  /// Tells a scratch layer which screen it belongs to. Rotation layers inherit
  /// this so a screen-sized blur cache can be sampled correctly.
  #[must_use]
  pub fn with_screen(mut self, screen: (u32, u32)) -> Self {
    self.screen = screen;
    self
  }

  #[must_use]
  pub fn screen(&self) -> (u32, u32) {
    self.screen
  }

  /// Absolute position of this canvas' top-left pixel.
  #[must_use]
  pub fn offset(&self) -> (f32, f32) {
    self.offset
  }

  #[must_use]
  pub fn width(&self) -> u32 {
    self.pixmap.width()
  }

  #[must_use]
  pub fn height(&self) -> u32 {
    self.pixmap.height()
  }

  /// The resolved background, before any UI is painted. The glass cache blurs
  /// this, which is why it needs read access to the pixels.
  #[must_use]
  pub fn pixmap(&self) -> &Pixmap {
    &self.pixmap
  }

  pub fn clear(&mut self, color: Rgb) {
    self.pixmap.fill(color.to_skia());
  }

  /// Empties the canvas to fully transparent pixels. The GPU overlay uses a
  /// transparent full-screen canvas so the compositor can put the wallpaper
  /// underneath it instead of repainting the background on the CPU.
  pub fn clear_transparent(&mut self) {
    self.pixmap.fill(tiny_skia::Color::TRANSPARENT);
  }

  fn local(&self, rect: Rect) -> Rect {
    Rect::new(
      rect.x - self.offset.0,
      rect.y - self.offset.1,
      rect.w,
      rect.h,
    )
  }

  fn local_point(&self, point: (f32, f32)) -> (f32, f32) {
    (point.0 - self.offset.0, point.1 - self.offset.1)
  }

  /// A vertical gradient, used for the default background. A flat background
  /// is just this with the same colour twice, so callers do not need two paths.
  pub fn vertical_gradient(&mut self, top: Rgb, bottom: Rgb) {
    let width = self.width() as f32;
    let height = self.height() as f32;
    let Some(rect) = SkRect::from_xywh(0.0, 0.0, width, height) else {
      return;
    };
    let Some(shader) = LinearGradient::new(
      Point::from_xy(0.0, 0.0),
      Point::from_xy(0.0, height),
      vec![
        GradientStop::new(0.0, top.to_skia()),
        GradientStop::new(1.0, bottom.to_skia()),
      ],
      SpreadMode::Pad,
      Transform::identity(),
    ) else {
      return;
    };
    let paint = Paint {
      shader,
      anti_alias: false,
      ..Paint::default()
    };
    self
      .pixmap
      .fill_rect(rect, &paint, Transform::identity(), None);
  }

  pub fn fill(&mut self, rect: Rect, color: Rgb, radius: f32) {
    let Some(path) = rounded_path(self.local(rect), radius) else {
      return;
    };
    let paint = Paint {
      shader: Shader::SolidColor(color.to_skia()),
      anti_alias: true,
      ..Paint::default()
    };
    self.pixmap.fill_path(
      &path,
      &paint,
      FillRule::Winding,
      Transform::identity(),
      None,
    );
  }

  /// A horizontal gradient inside a rounded rectangle. Used by the hero login
  /// panel (transparent on the artwork side, solid on the form side) and by
  /// themed field fills. Stops are fractions from 0 to 1 and must be sorted.
  pub fn horizontal_gradient_stops(&mut self, rect: Rect, radius: f32, stops: &[(f32, Rgb)]) {
    self.linear_gradient_stops(rect, radius, 0.0, stops);
  }

  /// A gradient at any angle. `angle` is in degrees: 0 goes left to right,
  /// 90 top to bottom. Button fills, hover wipes, and sweep effects all use
  /// this, which is why the direction is a config value rather than a second
  /// drawing function per orientation.
  pub fn linear_gradient_stops(
    &mut self,
    rect: Rect,
    radius: f32,
    angle: f32,
    stops: &[(f32, Rgb)],
  ) {
    if rect.w <= 0.0 || rect.h <= 0.0 || stops.len() < 2 {
      return;
    }
    let local = self.local(rect);
    let Some(path) = rounded_path(local, radius) else {
      return;
    };
    let stops = stops
      .iter()
      .map(|(at, color)| GradientStop::new(at.clamp(0.0, 1.0), color.to_skia()))
      .collect();
    let radians = angle.to_radians();
    let (sin, cos) = radians.sin_cos();
    let half_x = local.w * 0.5;
    let half_y = local.h * 0.5;
    let center = local.center();
    // Project the rectangle onto the gradient direction. This makes a 45
    // degree gradient cover every corner instead of leaving a solid band.
    let reach = half_x * cos.abs() + half_y * sin.abs();
    let start = (center.0 - cos * reach, center.1 - sin * reach);
    let end = (center.0 + cos * reach, center.1 + sin * reach);
    let Some(shader) = LinearGradient::new(
      Point::from_xy(start.0, start.1),
      Point::from_xy(end.0, end.1),
      stops,
      SpreadMode::Pad,
      Transform::identity(),
    ) else {
      return;
    };
    let paint = Paint {
      shader,
      anti_alias: true,
      ..Paint::default()
    };
    self.pixmap.fill_path(
      &path,
      &paint,
      FillRule::Winding,
      Transform::identity(),
      None,
    );
  }

  pub fn stroke(&mut self, rect: Rect, color: Rgb, radius: f32, width: f32) {
    let Some(path) = rounded_path(self.local(rect), radius) else {
      return;
    };
    let paint = Paint {
      shader: Shader::SolidColor(color.to_skia()),
      anti_alias: true,
      ..Paint::default()
    };
    let stroke = Stroke {
      width,
      ..Stroke::default()
    };
    self
      .pixmap
      .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
  }

  /// A translucent rounded panel filled with an already-blurred copy of the
  /// background. This is the fake backdrop blur: the compositor is not asked
  /// for anything, and the expensive part (blurring) happens in
  /// [`crate::ui::glass`] at a lower resolution and a lower rate.
  pub fn fill_blurred(&mut self, rect: Rect, radius: f32, image: &Pixmap, opacity: f32) {
    let local = self.local(rect);
    let Some(path) = rounded_path(local, radius) else {
      return;
    };
    let Some(mask) = self.canvas_mask(local, radius) else {
      return;
    };
    // The blur cache is screen-sized. A scratch canvas starts at `offset`, so
    // shift the pattern back to keep sampling the right part of the wallpaper.
    let paint = Paint {
      shader: Pattern::new(
        image.as_ref(),
        SpreadMode::Pad,
        FilterQuality::Bilinear,
        opacity.clamp(0.0, 1.0),
        Transform::from_translate(-self.offset.0, -self.offset.1),
      ),
      anti_alias: true,
      ..Paint::default()
    };
    self.pixmap.fill_path(
      &path,
      &paint,
      FillRule::Winding,
      Transform::identity(),
      Some(&mask),
    );
  }

  /// Adds a low-amplitude noise texture inside a rounded panel. The noise is
  /// cached at 64x64 and tiled, which keeps the memory cost tiny.
  pub fn fill_noise(&mut self, rect: Rect, radius: f32, noise: &Pixmap, opacity: f32) {
    let local = self.local(rect);
    let Some(path) = rounded_path(local, radius) else {
      return;
    };
    let Some(mask) = self.canvas_mask(local, radius) else {
      return;
    };
    let paint = Paint {
      shader: Pattern::new(
        noise.as_ref(),
        SpreadMode::Repeat,
        FilterQuality::Nearest,
        opacity.clamp(0.0, 1.0),
        Transform::from_translate(-self.offset.0, -self.offset.1),
      ),
      anti_alias: false,
      ..Paint::default()
    };
    self.pixmap.fill_path(
      &path,
      &paint,
      FillRule::Winding,
      Transform::identity(),
      Some(&mask),
    );
  }

  fn canvas_mask(&self, rect: Rect, radius: f32) -> Option<Mask> {
    let path = rounded_path(rect, radius)?;
    let mut mask = Mask::new(self.width(), self.height())?;
    mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    Some(mask)
  }

  pub fn fill_circle(&mut self, center: (f32, f32), radius: f32, color: Rgb) {
    let Some(path) = circle_path(self.local_point(center), radius) else {
      return;
    };
    let paint = Paint {
      shader: Shader::SolidColor(color.to_skia()),
      anti_alias: true,
      ..Paint::default()
    };
    self.pixmap.fill_path(
      &path,
      &paint,
      FillRule::Winding,
      Transform::identity(),
      None,
    );
  }

  pub fn stroke_circle(&mut self, center: (f32, f32), radius: f32, color: Rgb, width: f32) {
    let Some(path) = circle_path(self.local_point(center), radius) else {
      return;
    };
    let paint = Paint {
      shader: Shader::SolidColor(color.to_skia()),
      anti_alias: true,
      ..Paint::default()
    };
    let stroke = Stroke {
      width,
      ..Stroke::default()
    };
    self
      .pixmap
      .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
  }

  pub fn line(&mut self, from: (f32, f32), to: (f32, f32), color: Rgb, width: f32) {
    let from = self.local_point(from);
    let to = self.local_point(to);
    let mut builder = PathBuilder::new();
    builder.move_to(from.0, from.1);
    builder.line_to(to.0, to.1);
    let Some(path) = builder.finish() else {
      return;
    };
    let paint = Paint {
      shader: Shader::SolidColor(color.to_skia()),
      anti_alias: true,
      ..Paint::default()
    };
    let stroke = Stroke {
      width,
      ..Stroke::default()
    };
    self
      .pixmap
      .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
  }

  /// Draws an image so it covers the whole canvas, cropping the excess. This
  /// is the wallpaper path; frames are already screen-sized, so the transform
  /// is normally the identity and the opacity is what fades a new wallpaper in.
  pub fn image_cover(&mut self, image: &Pixmap) {
    self.image_cover_alpha(image, 1.0);
  }

  pub fn image_cover_alpha(&mut self, image: &Pixmap, opacity: f32) {
    if image.width() == 0 || image.height() == 0 {
      return;
    }
    let scale = (self.width() as f32 / image.width() as f32)
      .max(self.height() as f32 / image.height() as f32);
    let dx = (self.width() as f32 - image.width() as f32 * scale) / 2.0;
    let dy = (self.height() as f32 - image.height() as f32 * scale) / 2.0;
    self.draw_image(
      image,
      Transform::from_scale(scale, scale).post_translate(dx, dy),
      opacity,
      FilterQuality::Bilinear,
    );
  }

  /// Draws an image into `rect`, scaling it to fit exactly. Plugin images
  /// already have their aspect ratio applied by the renderer.
  pub fn image_in(&mut self, image: &Pixmap, rect: Rect) {
    self.image_in_alpha(image, rect, 1.0);
  }

  /// Same as [`Self::image_in`], with an opacity for decorative images.
  pub fn image_in_alpha(&mut self, image: &Pixmap, rect: Rect, opacity: f32) {
    if image.width() == 0 || image.height() == 0 || rect.w <= 0.0 || rect.h <= 0.0 {
      return;
    }
    let sx = rect.w / image.width() as f32;
    let sy = rect.h / image.height() as f32;
    // A 1:1 blit is common for decor panels and is exact with nearest
    // sampling. Bilinear on an identical grid still costs a full resample.
    let quality = if (rect.w - image.width() as f32).abs() < 0.5
      && (rect.h - image.height() as f32).abs() < 0.5
    {
      FilterQuality::Nearest
    } else {
      FilterQuality::Bilinear
    };
    self.draw_image(
      image,
      Transform::from_scale(sx, sy).post_translate(rect.x, rect.y),
      opacity,
      quality,
    );
  }

  /// Draws an image into `rect` with an extra rotation and uniform scale
  /// around the rect centre. Unlike a scratch-layer composite this samples the
  /// source once, which is much cheaper for the small animated ornaments.
  pub fn image_transformed(
    &mut self,
    image: &Pixmap,
    rect: Rect,
    angle: f32,
    scale: f32,
    opacity: f32,
  ) {
    if image.width() == 0 || image.height() == 0 || rect.w <= 0.0 || rect.h <= 0.0 || opacity <= 0.0
    {
      return;
    }
    let scale = scale.clamp(0.01, 8.0);
    let sx = rect.w / image.width() as f32 * scale;
    let sy = rect.h / image.height() as f32 * scale;
    let untouched = angle.abs() <= 0.01
      && (scale - 1.0).abs() <= 0.001
      && (rect.w - image.width() as f32).abs() < 0.5
      && (rect.h - image.height() as f32).abs() < 0.5;
    let quality = if untouched {
      FilterQuality::Nearest
    } else {
      FilterQuality::Bilinear
    };
    let pivot = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    let transform = Transform::from_translate(pivot.0, pivot.1)
      .pre_concat(Transform::from_rotate(angle))
      .pre_concat(Transform::from_scale(sx, sy))
      .pre_concat(Transform::from_translate(
        -(image.width() as f32) / 2.0,
        -(image.height() as f32) / 2.0,
      ));
    self.draw_image(image, transform, opacity, quality);
  }

  fn draw_image(
    &mut self,
    image: &Pixmap,
    transform: Transform,
    opacity: f32,
    quality: FilterQuality,
  ) {
    let paint = PixmapPaint {
      quality,
      opacity: opacity.clamp(0.0, 1.0),
      ..PixmapPaint::default()
    };
    let transform = transform.post_translate(-self.offset.0, -self.offset.1);
    self
      .pixmap
      .draw_pixmap(0, 0, image.as_ref(), &paint, transform, None);
  }

  /// Composites a scratch layer at its own absolute origin, rotated around an
  /// absolute pivot. The layer already contains only the widget, so this is
  /// cheap even when the widget has text, an image, and a bracket.
  pub fn draw_layer(&mut self, layer: &Canvas, center: (f32, f32), angle: f32, opacity: f32) {
    self.draw_layer_scaled(layer, center, angle, 1.0, opacity);
  }

  /// Same as [`Self::draw_layer`] with a uniform scale around the same pivot.
  /// Animated decor uses this for its breathe/pulse motion without re-shaping
  /// text or re-rasterising the source at every size.
  pub fn draw_layer_scaled(
    &mut self,
    layer: &Canvas,
    center: (f32, f32),
    angle: f32,
    scale: f32,
    opacity: f32,
  ) {
    if layer.width() == 0 || layer.height() == 0 {
      return;
    }
    let scale = scale.clamp(0.01, 8.0);
    let dx = layer.offset.0 - self.offset.0;
    let dy = layer.offset.1 - self.offset.1;
    let pivot = (center.0 - layer.offset.0, center.1 - layer.offset.1);
    let scale_at = Transform::from_translate(pivot.0, pivot.1)
      .pre_concat(Transform::from_scale(scale, scale))
      .pre_concat(Transform::from_translate(-pivot.0, -pivot.1));
    let transform = Transform::from_translate(dx, dy)
      .pre_concat(Transform::from_rotate_at(angle, pivot.0, pivot.1))
      .pre_concat(scale_at);
    let paint = PixmapPaint {
      quality: FilterQuality::Bilinear,
      opacity: opacity.clamp(0.0, 1.0),
      ..PixmapPaint::default()
    };
    self
      .pixmap
      .draw_pixmap(0, 0, layer.pixmap.as_ref(), &paint, transform, None);
  }

  /// Blends one glyph pixel span onto the canvas.
  ///
  /// cosmic-text reports straight sRGB plus coverage alpha, while the canvas is
  /// premultiplied. Blending therefore multiplies the source by its alpha and
  /// scales the destination by the inverse. `clip` keeps a long username from
  /// painting outside its input field.
  pub fn blend_text_pixels(
    &mut self,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: [u8; 4],
    clip: Option<Rect>,
  ) {
    let [r, g, b, alpha] = color;
    if alpha == 0 {
      return;
    }
    let x = x - self.offset.0.round() as i32;
    let y = y - self.offset.1.round() as i32;
    let clip = clip.map(|clip| self.local(clip));
    let canvas_width = self.width() as i32;
    let canvas_height = self.height() as i32;
    let data = self.pixmap.data_mut();

    for row in 0..height as i32 {
      let py = y + row;
      if py < 0 || py >= canvas_height {
        continue;
      }
      for column in 0..width as i32 {
        let px = x + column;
        if px < 0 || px >= canvas_width {
          continue;
        }
        if let Some(clip) = clip {
          let inside = px as f32 >= clip.x
            && (px as f32) < clip.right()
            && py as f32 >= clip.y
            && (py as f32) < clip.bottom();
          if !inside {
            continue;
          }
        }
        let index = ((py as u32 * canvas_width as u32 + px as u32) * 4) as usize;
        let Some(pixel) = data.get_mut(index..index + 4) else {
          continue;
        };
        let inverse = 255 - u32::from(alpha);
        let source = u32::from(alpha);
        pixel[0] = ((u32::from(r) * source + u32::from(pixel[0]) * inverse) / 255) as u8;
        pixel[1] = ((u32::from(g) * source + u32::from(pixel[1]) * inverse) / 255) as u8;
        pixel[2] = ((u32::from(b) * source + u32::from(pixel[2]) * inverse) / 255) as u8;
        pixel[3] = ((255 * source + u32::from(pixel[3]) * inverse) / 255) as u8;
      }
    }
  }

  /// Converts to softbuffer's `0x00RRGGBB` layout. The background is opaque, so
  /// dropping the premultiplied alpha channel is safe here.
  pub fn to_argb(&self, out: &mut [u32]) {
    for (pixel, source) in out.iter_mut().zip(self.pixmap.data().chunks_exact(4)) {
      *pixel = (u32::from(source[0]) << 16) | (u32::from(source[1]) << 8) | u32::from(source[2]);
    }
  }

  pub fn save_png(&self, path: &std::path::Path) -> Result<(), png::EncodingError> {
    self.pixmap.save_png(path)
  }
}

/// The axis-aligned bounds of a rotated rectangle. Rotation scratch layers
/// use this to size themselves tightly around the widget.
#[must_use]
pub fn rotated_bounds(rect: Rect, angle: f32) -> Rect {
  if angle == 0.0 {
    return rect;
  }
  let (sin, cos) = angle.to_radians().sin_cos();
  let width = rect.w * cos.abs() + rect.h * sin.abs();
  let height = rect.w * sin.abs() + rect.h * cos.abs();
  let center = rect.center();
  Rect::new(
    center.0 - width / 2.0,
    center.1 - height / 2.0,
    width,
    height,
  )
}

fn rounded_path(rect: Rect, radius: f32) -> Option<Path> {
  let skia = rect.to_skia()?;
  if rect.w <= 0.0 || rect.h <= 0.0 {
    return None;
  }
  let radius = radius.max(0.0).min(rect.w / 2.0).min(rect.h / 2.0);
  if radius <= 0.5 {
    return Some(PathBuilder::from_rect(skia));
  }

  let mut builder = PathBuilder::new();
  let (x, y, w, h) = (rect.x, rect.y, rect.w, rect.h);
  builder.move_to(x + radius, y);
  builder.line_to(x + w - radius, y);
  builder.quad_to(x + w, y, x + w, y + radius);
  builder.line_to(x + w, y + h - radius);
  builder.quad_to(x + w, y + h, x + w - radius, y + h);
  builder.line_to(x + radius, y + h);
  builder.quad_to(x, y + h, x, y + h - radius);
  builder.line_to(x, y + radius);
  builder.quad_to(x, y, x + radius, y);
  builder.close();
  builder.finish()
}

fn circle_path(center: (f32, f32), radius: f32) -> Option<Path> {
  if radius <= 0.0 {
    return None;
  }
  let mut builder = PathBuilder::new();
  builder.push_circle(center.0, center.1, radius);
  builder.finish()
}
