# resonate-plugins

Transport plugins for the Resonate server. A plugin represents an external
system's unit of work — anything with a beginning and an end — as a durable
promise: the plugin begins the work, sees it through to its terminal state,
and settles the promise with the outcome.

- [Plugins.md](Plugins.md) — the plugin catalog.
- [plugins/](plugins/) — one folder per plugin:
  - `README.md`
  - `spec/specification.md` — the implementation specification
  - `src/` — the Rust crate
