//! resonate-plugin-n8n: a Resonate transport plugin.
//!
//! A library crate. The server binary constructs [`worker::Worker`] with
//! its `ResonateServer` port and registers it on the router under the
//! plugin's scheme; the router reads only the scheme, so adding a plugin
//! never requires editing core.

// `plugin` is public so `tests/process.rs` can exercise `plugin::process`
// directly — the plugin's only test surface.
pub mod plugin;
// n8n stamps no client-supplied identity on a retry (§ Idempotency), so
// this is the one plugin whose `process` never calls the frame's
// `sanitize`. The frame is never edited, so the lint is silenced here.
#[allow(dead_code)]
mod worker;

pub use plugin::Config;
pub use worker::Worker;

pub const SCHEME: &str = "n8n";
