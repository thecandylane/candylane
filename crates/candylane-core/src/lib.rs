//! candylane-core — the state engine, the Handler trait, and the handlers.
//!
//! The engine, `SqliteStore`, profile parser, all three handlers (winget/dotfile/script),
//! the registry, and the reboot-pending seam are implemented and tested. Reboot detection
//! and the winget executor have real Windows impls behind seams; off-Windows they fall back
//! to safe no-ops. The crypto owner-only ACL (in `candylane-crypto`) remains — see
//! `docs/FOLLOWUPS.md`.

pub mod engine;
pub mod handler;
pub mod lock; // single-writer lockfile (fs2) — fail-fast, crash-safe via OS flock
pub mod reboot; // reboot-pending detection seam (CBS∨WU gate, PFRO advisory)
pub mod store;
pub mod types;

pub mod profile; // TOML parse → Vec<Item>

pub mod handlers; // winget / dotfile / script
pub mod registry; // concrete HandlerRegistry over the three handlers

pub use handler::{Handler, RawOutput, WingetExecutor};
pub use reboot::{NoRebootCheck, RebootCheck, RebootState};
pub use registry::Handlers;
pub use types::*;

/// Crate-wide result. Swap to a `thiserror` enum once error taxonomy settles.
pub type Result<T> = anyhow::Result<T>;
