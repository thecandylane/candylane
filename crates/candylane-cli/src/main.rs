//! candylane — the CLI surface (Lane F).
//!
//! Wires the clap subcommands to the candylane-core engine. State lives under
//! `$HOME/.candylane/` (your jar): the SQLite DB at `state.db`, dotfile backups under
//! `backups/`, and the single-writer `lock`. `--resume` is intentionally absent (cut from
//! Phase 1, Decision #5).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use candylane_core::engine::{Engine, SyncState};
use candylane_core::lock::Lock;
use candylane_core::reboot::RebootCheck;
use candylane_core::registry::Handlers;
use candylane_core::store::{SqliteStore, StateStore};
use candylane_core::{profile, UndoKind};

/// How long a single post-install script may run before it is killed (CRITICAL #1).
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(300);
/// Bound on rollback-during-rollback retries per action (CRITICAL #5).
const MAX_UNDO_ATTEMPTS: u32 = 3;

#[derive(Parser)]
#[command(
    name = "candylane",
    version,
    about = "Build your box once. Pull it onto any machine — and revert clean."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate your identity and prepare ~/.candylane/.
    Init,
    /// Show what a pull would change — without touching anything.
    Diff {
        /// Path to the box (a profile TOML file).
        #[arg(value_name = "BOX")]
        profile: String,
    },
    /// Apply a box to this machine.
    Pull {
        /// Path to the box (a profile TOML file).
        #[arg(value_name = "BOX")]
        profile: String,
    },
    /// Roll back the last pull.
    Revert,
    /// Reconcile and roll back an interrupted pull.
    Recover,
    /// Show everything Candylane has done on this machine.
    History,
    /// Check the machine against your recorded state (the jar).
    Status,
}

/// `$HOME/.candylane` — the per-machine state root (the jar).
fn candylane_home() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home).join(".candylane"))
}

/// Path to the SQLite state DB (`$HOME/.candylane/state.db`).
fn state_db_path() -> Result<PathBuf> {
    Ok(candylane_home()?.join("state.db"))
}

/// Root for dotfile backups (`$HOME/.candylane/backups`).
fn backups_root() -> Result<PathBuf> {
    Ok(candylane_home()?.join("backups"))
}

/// The production reboot-pending detector: real PowerShell probe on Windows, always-clear
/// elsewhere. Boxed so the choice is a single expression; both impls are zero-size.
fn reboot_check() -> Box<dyn RebootCheck> {
    #[cfg(windows)]
    {
        Box::new(candylane_core::reboot::PowerShellRebootCheck::new())
    }
    #[cfg(not(windows))]
    {
        Box::new(candylane_core::reboot::NoRebootCheck)
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // Built once; referenced by every engine-constructing arm (stateless, cheap).
    let reboot = reboot_check();
    match cli.command {
        Command::Init => {
            let id = candylane_crypto::Identity::generate()?;
            println!("identity created: {:?}", id.verifying_key());
            Ok(())
        }

        Command::Diff { profile } => {
            // Read-only: parse + plan, never touch the machine. No lock needed.
            let parsed = profile::parse(std::path::Path::new(&profile))
                .with_context(|| format!("could not parse box {profile}"))?;

            let mut store = SqliteStore::open(&state_db_path()?)?;
            let handlers = Handlers::new();
            let engine = Engine {
                store: &mut store,
                handlers: &handlers,
                reboot_check: reboot.as_ref(),
                backups_root: backups_root()?,
                timeout: SCRIPT_TIMEOUT,
                max_undo_attempts: MAX_UNDO_ATTEMPTS,
            };

            let plan = engine.diff(&parsed)?;
            if plan.is_empty() {
                println!("nothing to do — '{}' is already satisfied", parsed.name);
                return Ok(());
            }

            println!("plan for '{}' ({} action(s)):", parsed.name, plan.len());
            for pa in &plan {
                let tag = match pa.undo_kind {
                    UndoKind::Inverse => "[reversible]",
                    UndoKind::Noop => "[no-op]",
                    // The two LOUD cases: residue / irreversibility the user must see.
                    UndoKind::BestEffort => "[BEST-EFFORT — residue may remain on revert]",
                    UndoKind::OneWay => "[ONE-WAY — CANNOT be reverted]",
                };
                println!("  {:?} {}  {}", pa.handler, pa.target.0, tag);
            }
            Ok(())
        }

        Command::Pull { profile } => {
            let _lock = Lock::acquire(&candylane_home()?)?;
            let parsed = profile::parse(std::path::Path::new(&profile))
                .with_context(|| format!("could not parse box {profile}"))?;

            let mut store = SqliteStore::open(&state_db_path()?)?;
            let handlers = Handlers::new();
            let mut engine = Engine {
                store: &mut store,
                handlers: &handlers,
                reboot_check: reboot.as_ref(),
                backups_root: backups_root()?,
                timeout: SCRIPT_TIMEOUT,
                max_undo_attempts: MAX_UNDO_ATTEMPTS,
            };

            match engine.pull(&parsed) {
                Ok(()) => {
                    println!("pull complete — '{}' applied", parsed.name);
                    Ok(())
                }
                Err(e) => {
                    // pull() already rolled back to clean on failure.
                    eprintln!("pull failed and was rolled back: {e:#}");
                    Err(e)
                }
            }
        }

        Command::Revert => {
            let _lock = Lock::acquire(&candylane_home()?)?;
            let mut store = SqliteStore::open(&state_db_path()?)?;
            let handlers = Handlers::new();
            let mut engine = Engine {
                store: &mut store,
                handlers: &handlers,
                reboot_check: reboot.as_ref(),
                backups_root: backups_root()?,
                timeout: SCRIPT_TIMEOUT,
                max_undo_attempts: MAX_UNDO_ATTEMPTS,
            };
            engine.revert_last()?;
            println!("revert complete");
            Ok(())
        }

        Command::Recover => {
            let _lock = Lock::acquire(&candylane_home()?)?;
            let mut store = SqliteStore::open(&state_db_path()?)?;
            let handlers = Handlers::new();
            let mut engine = Engine {
                store: &mut store,
                handlers: &handlers,
                reboot_check: reboot.as_ref(),
                backups_root: backups_root()?,
                timeout: SCRIPT_TIMEOUT,
                max_undo_attempts: MAX_UNDO_ATTEMPTS,
            };
            engine.recover()?;
            println!("recover complete");
            Ok(())
        }

        Command::History => {
            // Read-only.
            let store = SqliteStore::open(&state_db_path()?)?;
            let ops = store.list_operations()?;
            if ops.is_empty() {
                println!("no operations recorded yet");
                return Ok(());
            }
            for op in ops {
                let when = op.finished_at.as_deref().unwrap_or(&op.started_at);
                println!(
                    "#{:<4} {:<8} {:<18} {:<20} {}",
                    op.id,
                    format!("{:?}", op.kind),
                    format!("{:?}", op.status),
                    op.profile.as_deref().unwrap_or("-"),
                    when,
                );
            }
            Ok(())
        }

        Command::Status => {
            // Read-only.
            let mut store = SqliteStore::open(&state_db_path()?)?;
            let handlers = Handlers::new();
            let engine = Engine {
                store: &mut store,
                handlers: &handlers,
                reboot_check: reboot.as_ref(),
                backups_root: backups_root()?,
                timeout: SCRIPT_TIMEOUT,
                max_undo_attempts: MAX_UNDO_ATTEMPTS,
            };
            let report = engine.status()?;
            if report.is_empty() {
                println!("no applied box on this machine");
                return Ok(());
            }
            for e in report {
                let tag = match e.state {
                    SyncState::InSync => "in-sync",
                    SyncState::Drifted => "DRIFTED",
                    SyncState::NotApplicable => "n/a",
                };
                println!("  {:?} {}  [{}]", e.handler, e.target.0, tag);
            }
            Ok(())
        }
    }
}
