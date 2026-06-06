//! Core data types shared by the engine and the handlers.
//!
//! SCAFFOLD: structurally complete, NOT yet compiled (no toolchain on the dev host).
//! The enums map 1:1 to the CHECK constraints in `migrations/0001_init.sql`.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

pub type Json = serde_json::Value;

/// Which handler owns an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandlerKind {
    Winget,
    Dotfile,
    Script,
}

/// How reversible an action is. Drives `diff` honesty and `revert` behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoKind {
    /// Precisely reversible (dotfile, paired down-script).
    Inverse,
    /// Reversible at the artifact level only; registry/PATH/shell crumbs may remain (winget).
    BestEffort,
    /// Not reversible (post-script with no down-script). diff warns; revert skips + reports.
    OneWay,
    /// Nothing to undo (action was a no-op / pre-existing, delta ownership).
    Noop,
}

/// Per-action lifecycle. Mirrors the `actions.status` CHECK set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Pending,
    Applied,
    Failed,
    Reverted,
    Skipped,
    /// undo() exhausted `max_undo_attempts`; rollback continued best-effort.
    UndoFailed,
}

/// Per-operation lifecycle. Mirrors the `operations.status` CHECK set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpStatus {
    Pending,
    Applied,
    Failed,
    Reverted,
    /// Some actions reverted, some left applied (could not undo).
    PartiallyReverted,
    /// At least one action hit UndoFailed.
    RevertFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    Pull,
    Revert,
    Recover,
}

/// The system-level identifier a handler reads/acts on:
/// a winget package id, a dotfile destination path, or a script path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target(pub String);

/// A desired item parsed from the profile. The engine pairs each with its handler.
#[derive(Debug, Clone)]
pub enum Item {
    Winget { pkg: String },
    Dotfile { src: String, target: String },
    Script { run: String, undo: Option<String> },
}

impl Item {
    pub fn handler_kind(&self) -> HandlerKind {
        match self {
            Item::Winget { .. } => HandlerKind::Winget,
            Item::Dotfile { .. } => HandlerKind::Dotfile,
            Item::Script { .. } => HandlerKind::Script,
        }
    }

    pub fn target(&self) -> Target {
        match self {
            Item::Winget { pkg } => Target(pkg.clone()),
            Item::Dotfile { target, .. } => Target(target.clone()),
            Item::Script { run, .. } => Target(run.clone()),
        }
    }
}

/// Result of a pure system probe. Comparable to a stored `before_json` for reconcile.
/// No `Eq`: `serde_json::Value` is only `PartialEq` (floats). `PartialEq` is all
/// reconcile needs (`real != before`).
#[derive(Debug, Clone, PartialEq)]
pub struct Probe(pub Json);

/// What `plan()` decided to do. `before` is captured here; the final undo recipe
/// is produced by `apply()` (so it can reflect the real post-state).
#[derive(Debug, Clone)]
pub struct PlannedAction {
    pub handler: HandlerKind,
    pub target: Target,
    pub before: Json,
    pub undo_kind: UndoKind,
}

/// Returned by `apply()` after it re-probes to confirm the real effect.
#[derive(Debug, Clone)]
pub struct Applied {
    pub after: Json, // → actions.after_json
    pub undo: Json,  // → actions.undo_json (imperative recipe)
}

/// A row read back from the DB, handed to `undo()`.
#[derive(Debug, Clone)]
pub struct RecordedAction {
    pub id: i64,
    pub op_id: i64,
    pub seq: u32,
    pub handler: HandlerKind,
    pub target: Target,
    pub status: ActionStatus,
    pub before: Json,
    pub after: Option<Json>,
    pub undo_kind: UndoKind,
    pub undo: Json,
    pub undo_attempts: u32,
    pub undo_error: Option<String>,
}

/// Ambient context every `apply()` / `undo()` receives.
pub struct ApplyCtx<'a> {
    /// ~/.candylane/backups/<op>/ — where clobbered dotfile bytes are stored.
    pub backups_dir: &'a Path,
    /// ScriptHandler kills the child after this elapses (CRITICAL #1).
    pub timeout: Duration,
    /// diff sets this true: plan()+probe() only, never apply().
    pub dry_run: bool,
    /// Bounds rollback-during-rollback retries (CRITICAL #5).
    pub max_undo_attempts: u32,
}
