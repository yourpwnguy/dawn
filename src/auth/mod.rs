//! greetd authentication.
//!
//! The public surface is intentionally small: [`flow::Flow`] is the explicit
//! state machine, [`client::GreetdClient`] is the wire client, and
//! [`worker::WorkerHandle`] is what the GUI talks to. Everything in this module
//! is credential-aware, so nothing here is exposed to plugins and no code path
//! logs a password.

pub mod backend;
pub mod client;
pub mod flow;
pub mod model;
pub mod worker;
