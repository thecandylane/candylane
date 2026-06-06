//! candylane-core — the state engine, the Handler trait, and the handlers.
//!
//! The engine, `SqliteStore`, profile parser, dotfile + script handlers, and registry are
//! implemented and tested on Linux. The winget handler (Lane B) and the Windows-only paths
//! (`preflight`/`reboot_pending`, the crypto ACL in `candylane-crypto`) remain — see
//! `docs/FOLLOWUPS.md`.

pub mod engine;
pub mod handler;
pub mod lock; // single-writer lockfile (fs2) — fail-fast, crash-safe via OS flock
pub mod store;
pub mod types;

pub mod profile; // TOML parse → Vec<Item>

pub mod handlers; // winget (Lane B stub) / dotfile / script
pub mod registry; // concrete HandlerRegistry over the three handlers

pub use handler::{Handler, RawOutput, WingetExecutor};
pub use registry::Handlers;
pub use types::*;

/// Crate-wide result. Swap to a `thiserror` enum once error taxonomy settles.
pub type Result<T> = anyhow::Result<T>;
