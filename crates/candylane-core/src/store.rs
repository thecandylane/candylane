//! State persistence seam. The engine talks to `StateStore`; `SqliteStore` is the
//! real impl over `~/.candylane/state.db`. A trait so the engine is testable with an
//! in-memory fake and so the SQL stays in one place.
//!
//! SCAFFOLD: SQL bodies are `todo!()` (Lane A tail). Trait + signatures are final.

use crate::types::*;
use crate::Result;
use std::path::Path;

/// Fields needed to insert a new action row.
pub struct NewAction {
    pub handler: HandlerKind,
    pub target: Target,
    pub before: Json,
    pub undo_kind: UndoKind,
    pub status: ActionStatus, // Pending for real actions, Skipped for no-ops
}

pub trait StateStore {
    fn begin_op(
        &mut self,
        kind: OpKind,
        profile: Option<&str>,
        profile_hash: Option<&str>,
        parent: Option<i64>,
    ) -> Result<i64>;

    fn set_op_status(&mut self, op: i64, status: OpStatus) -> Result<()>;

    /// Insert with status from `NewAction`. For real actions undo_json starts as
    /// `"{}"` and is filled by `set_action_applied`.
    fn insert_action(&mut self, op: i64, seq: u32, action: &NewAction) -> Result<i64>;

    /// Promote a pending action to applied, recording the confirmed after-state and
    /// the imperative undo recipe.
    fn set_action_applied(&mut self, action: i64, after: &Json, undo: &Json) -> Result<()>;

    fn set_action_status(&mut self, action: i64, status: ActionStatus) -> Result<()>;

    /// Increment undo_attempts, store the error, return the new attempt count.
    fn bump_undo_attempt(&mut self, action: i64, err: &str) -> Result<u32>;

    /// Applied actions for an op, newest seq first (revert order).
    fn applied_actions_desc(&self, op: i64) -> Result<Vec<RecordedAction>>;

    /// The single in-flight action (status=pending) of an op, if any.
    fn pending_action(&self, op: i64) -> Result<Option<RecordedAction>>;

    /// Statuses of all actions in an op (for finalize_op).
    fn action_statuses(&self, op: i64) -> Result<Vec<ActionStatus>>;

    /// An op left in `pending` (a crashed pull), if any.
    fn unfinished_op(&self) -> Result<Option<i64>>;

    /// The most recent successfully-applied pull (revert target).
    fn last_applied_op(&self) -> Result<Option<i64>>;
}

/// Real SQLite-backed store.
pub struct SqliteStore {
    // conn: rusqlite::Connection,
}

impl SqliteStore {
    /// Open ~/.candylane/state.db, set PRAGMAs on the connection (foreign_keys + WAL),
    /// run pending migrations. foreign_keys is per-connection and OFF by default.
    pub fn open(_path: &Path) -> Result<Self> {
        // let conn = rusqlite::Connection::open(path)?;
        // conn.pragma_update(None, "foreign_keys", "ON")?;
        // conn.pragma_update(None, "journal_mode", "WAL")?;
        // migrate(&conn)?;   // apply migrations/*.sql, gated on meta.schema_version
        todo!("open connection, set PRAGMAs, run migrations")
    }
}

impl StateStore for SqliteStore {
    fn begin_op(&mut self, _kind: OpKind, _profile: Option<&str>, _hash: Option<&str>, _parent: Option<i64>) -> Result<i64> { todo!() }
    fn set_op_status(&mut self, _op: i64, _status: OpStatus) -> Result<()> { todo!() }
    fn insert_action(&mut self, _op: i64, _seq: u32, _action: &NewAction) -> Result<i64> { todo!() }
    fn set_action_applied(&mut self, _action: i64, _after: &Json, _undo: &Json) -> Result<()> { todo!() }
    fn set_action_status(&mut self, _action: i64, _status: ActionStatus) -> Result<()> { todo!() }
    fn bump_undo_attempt(&mut self, _action: i64, _err: &str) -> Result<u32> { todo!() }
    fn applied_actions_desc(&self, _op: i64) -> Result<Vec<RecordedAction>> { todo!() }
    fn pending_action(&self, _op: i64) -> Result<Option<RecordedAction>> { todo!() }
    fn action_statuses(&self, _op: i64) -> Result<Vec<ActionStatus>> { todo!() }
    fn unfinished_op(&self) -> Result<Option<i64>> { todo!() }
    fn last_applied_op(&self) -> Result<Option<i64>> { todo!() }
}
