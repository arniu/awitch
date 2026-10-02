//! The cheapskate source adapter (doc placeholder).
//!
//! Source location: API keys live in environment variables (`api_key_env`);
//! the provider configurations live in `config.yaml` + `registry.yaml`
//! under `~/.config/cheapskate/`.
//!
//! Data schema: provider entries with base_url, key, and protocols — same
//! structure as cc_switch, different storage format. Returns `Vec<Source>`
//! using the shared `Source`/`Provider` types from `super`.
