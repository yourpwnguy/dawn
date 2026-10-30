//! Drawing, layout, and input for the login screen.
//!
//! The UI layer is stateless between frames: a [`view::View`] describes what
//! should be on screen, and [`render::paint`] turns that description into
//! pixels. Keeping the description separate from the drawing code is what makes
//! `--screenshot` possible without a window, and it is also how the app can
//! redraw after a resize without keeping a window handle around.
//!
//! Pixels are rasterised by tiny-skia on the CPU, which is what keeps the
//! screenshot and live paths identical. The window may hand the resulting
//! overlay to [`gpu`] for compositing over the wallpaper, or fall back to the
//! CPU softbuffer path when no GPU is available.
//!
//! Nothing in this module sees a credential. The password field only ever hands
//! out masked glyphs; the real value moves from [`input::TextInput`] straight
//! into [`crate::secret::Secret`] when the user submits.

pub mod anim;
pub mod background;
pub mod battery;
pub mod calendar;
pub mod canvas;
pub mod clock;
pub mod drag;
pub mod glass;
pub mod gpu;
pub mod hud;
pub mod image;
pub mod input;
pub mod media;
pub mod motion;
pub mod particles;
pub mod render;
pub mod screenshot;
pub mod stream;
pub mod system;
pub mod text;
pub mod theme;
pub mod transition;
pub mod video;
pub mod view;
pub mod window;
