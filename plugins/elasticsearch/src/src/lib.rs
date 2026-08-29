//! resonate-plugin-elasticsearch: a Resonate transport plugin.
//!
//! A library crate. The server binary constructs [`worker::Worker`] with
//! its `ResonateServer` port and registers it on the router under the
//! plugin's scheme; the router reads only the scheme, so adding a plugin
//! never requires editing core.

// `plugin` is public so `tests/process.rs` can exercise `plugin::process`
// directly — the plugin's only test surface.
pub mod plugin;
mod worker;

pub use plugin::Config;
pub use worker::Worker;

pub const SCHEME: &str = "elasticsearch";
