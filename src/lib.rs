//! dawn is a GPU-composited login greeter for greetd, with Wayland and X11
//! frontends, animated wallpapers, declarative themes, and a terminal fallback.
//!
//! The crate is split into four layers that only ever depend inward:
//!
//! - [`auth`]: the greetd wire protocol and the authentication state machine.
//!   It never knows what the UI looks like and it owns every credential.
//! - [`session`] and [`config`]: discover what can be started and how the
//!   greeter should look. Both are pure data producers.
//! - [`plugin`]: loads inert, declarative widget descriptions. Plugins get no
//!   input events, no auth state, and no code execution.
//! - [`theme`]: validates and stores portable theme bundles. A bundle keeps
//!   its config and assets together, and the greeter resolves one by id.
//! - [`ui`] and [`app`]: layout, drawing, keyboard handling, and the glue that
//!   moves events between the UI and the auth worker.
//!
//! The rules that shape the crate are simple: never block the UI thread on
//! authentication, never let a plugin touch a credential, and always keep a
//! path to a working login screen even when config, themes, plugins, or the
//! graphical renderer break. See `docs/architecture.md` for the full flow.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod app;
pub mod auth;
pub mod cli;
pub mod color;
pub mod config;
pub mod dev;
pub mod elevate;
pub mod fsutil;
pub mod init;
pub mod layout;
pub mod plugin;
pub mod secret;
pub mod session;
pub mod sysinfo;
pub mod theme;
pub mod tty;
pub mod ui;
