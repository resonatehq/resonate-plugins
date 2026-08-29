//! resonate-plugin-baserow: a Resonate transport plugin.
//!
//! A library crate. The server binary constructs [`worker::Worker`] with
//! its `ResonateServer` port and registers it on the router under the
//! plugin's scheme; the router reads only the scheme, so adding a plugin
//! never requires editing core.

pub mod plugin;
mod worker;

pub use plugin::Config;
pub use worker::Worker;

pub const SCHEME: &str = "baserow";
