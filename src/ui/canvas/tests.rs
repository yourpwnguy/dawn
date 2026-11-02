use super::Canvas;
use tiny_skia::{Color, Pixmap};

#[test]
fn an_opaque_screen_sized_wallpaper_is_a_memcpy() {
  let mut canvas = Canvas::new(4, 4).expect("canvas");
  let mut source = Pixmap::new(4, 4).expect("source");
  source.fill(Color::from_rgba8(10, 20, 30, 255));
  canvas.image_cover(&source);
  assert_eq!(canvas.pixmap.data(), source.data());
}
