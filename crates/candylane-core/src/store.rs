//! State persistence seam. The engine talks to `StateStore`; `SqliteStore` is the
//! real impl over `~/.candylane/state.db`. A trait so the engine is testable with an
//! in-memory fake and so the SQL stays in one place.

use crate::types::*;
use crate::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

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

// ---- enum <-> TEXT helpers -------------------------------------------------

fn op_kind_to_str(k: OpKind) -> &'static str {
    match k {
        OpKind::Pull => "pull",
        OpKind::Revert => "revert",
        OpKind::Recover => "recover",
    }
}

fn op_status_to_str(s: OpStatus) -> &'static str {
    match s {
        OpStatus::Pending => "pending",
        OpStatus::Applied => "applied",
        OpStatus::Failed => "failed",
        OpStatus::Reverted => "reverted",
        OpStatus::PartiallyReverted => "partially_reverted",
        OpStatus::RevertFailed => "revert_failed",
    }
}

fn action_status_to_str(s: ActionStatus) -> &'static str {
    match s {
        ActionStatus::Pending => "pending",
        ActionStatus::Applied => "applied",
        ActionStatus::Failed => "failed",
        ActionStatus::Reverted => "reverted",
        ActionStatus::Skipped => "skipped",
        ActionStatus::UndoFailed => "undo_failed",
    }
}

fn action_status_from_str(s: &str) -> Result<ActionStatus> {
    match s {
        "pending" => Ok(ActionStatus::Pending),
        "applied" => Ok(ActionStatus::Applied),
        "failed" => Ok(ActionStatus::Failed),
        "reverted" => Ok(ActionStatus::Reverted),
        "skipped" => Ok(ActionStatus::Skipped),
        "undo_failed" => Ok(ActionStatus::UndoFailed),
        other => anyhow::bail!("unknown action status in DB: {:?}", other),
    }
}

fn handler_kind_to_str(h: HandlerKind) -> &'static str {
    match h {
        HandlerKind::Winget => "winget",
        HandlerKind::Dotfile => "dotfile",
        HandlerKind::Script => "script",
    }
}

fn handler_kind_from_str(s: &str) -> Result<HandlerKind> {
    match s {
        "winget" => Ok(HandlerKind::Winget),
        "dotfile" => Ok(HandlerKind::Dotfile),
        "script" => Ok(HandlerKind::Script),
        other => anyhow::bail!("unknown handler kind in DB: {:?}", other),
    }
}

fn undo_kind_to_str(u: UndoKind) -> &'static str {
    match u {
        UndoKind::Inverse => "inverse",
        UndoKind::BestEffort => "best_effort",
        UndoKind::OneWay => "one_way",
        UndoKind::Noop => "noop",
    }
}

fn undo_kind_from_str(s: &str) -> Result<UndoKind> {
    match s {
        "inverse" => Ok(UndoKind::Inverse),
        "best_effort" => Ok(UndoKind::BestEffort),
        "one_way" => Ok(UndoKind::OneWay),
        "noop" => Ok(UndoKind::Noop),
        other => anyhow::bail!("unknown undo kind in DB: {:?}", other),
    }
}

/// Return the current UTC time as an RFC3339 string.
fn now_rfc3339() -> Result<String> {
    let now = OffsetDateTime::now_utc();
    now.format(&Rfc3339)
        .map_err(|e| anyhow::anyhow!("timestamp format error: {}", e))
}

// ---- migration -------------------------------------------------------------

/// Apply migration 0001 if the meta table is absent or schema_version < 1.
/// The SQL itself seeds `schema_version=1`, so we only run it once.
fn migrate(conn: &Connection) -> Result<()> {
    // Check whether the meta table already exists and has schema_version >= 1.
    let version: Option<i64> = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None); // table may not exist yet; sqlite error → treat as absent

    if version.unwrap_or(0) < 1 {
        conn.execute_batch(include_str!("../migrations/0001_init.sql"))?;
    }
    Ok(())
}

// ---- row-mapping helper ----------------------------------------------------

/// Map a rusqlite `Row` (columns in the order used by `action_query_cols!`) to
/// a `RecordedAction`.
fn row_to_recorded_action(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordedAction> {
    let id: i64 = row.get(0)?;
    let op_id: i64 = row.get(1)?;
    let seq: u32 = row.get::<_, i64>(2)? as u32;
    let handler_str: String = row.get(3)?;
    let target_str: String = row.get(4)?;
    let status_str: String = row.get(5)?;
    let before_str: String = row.get(6)?;
    let after_str: Option<String> = row.get(7)?;
    let undo_kind_str: String = row.get(8)?;
    let undo_str: String = row.get(9)?;
    let undo_attempts: u32 = row.get::<_, i64>(10)? as u32;
    let undo_error: Option<String> = row.get(11)?;

    // Convert the DB strings to types; map parse errors to rusqlite::Error::InvalidColumnType
    // so callers get a meaningful error without unwrap().
    let handler = handler_kind_from_str(&handler_str).map_err(|e| {
        rusqlite::Error::InvalidColumnType(3, format!("{}", e), rusqlite::types::Type::Text)
    })?;
    let status = action_status_from_str(&status_str).map_err(|e| {
        rusqlite::Error::InvalidColumnType(5, format!("{}", e), rusqlite::types::Type::Text)
    })?;
    let before: Json = serde_json::from_str(&before_str).map_err(|e| {
        rusqlite::Error::InvalidColumnType(6, format!("{}", e), rusqlite::types::Type::Text)
    })?;
    let after: Option<Json> = after_str
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::InvalidColumnType(7, format!("{}", e), rusqlite::types::Type::Text)
        })?;
    let undo_kind = undo_kind_from_str(&undo_kind_str).map_err(|e| {
        rusqlite::Error::InvalidColumnType(8, format!("{}", e), rusqlite::types::Type::Text)
    })?;
    let undo: Json = serde_json::from_str(&undo_str).map_err(|e| {
        rusqlite::Error::InvalidColumnType(9, format!("{}", e), rusqlite::types::Type::Text)
    })?;

    Ok(RecordedAction {
        id,
        op_id,
        seq,
        handler,
        target: Target(target_str),
        status,
        before,
        after,
        undo_kind,
        undo,
        undo_attempts,
        undo_error,
    })
}

// ---- SqliteStore -----------------------------------------------------------

/// Real SQLite-backed store.
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Open the database at `path`, set PRAGMAs on the connection
    /// (`foreign_keys=ON`, `journal_mode=WAL`), and run pending migrations.
    /// `foreign_keys` is per-connection and OFF by default in SQLite.
    pub fn open(path: &Path) -> Result<Self> {
        // Ensure the parent directory exists.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        // Per-connection PRAGMAs (not persisted in the DB file).
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // journal_mode returns the new mode; use execute_batch to ignore the result set.
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        migrate(&conn)?;

        Ok(SqliteStore { conn })
    }
}

impl StateStore for SqliteStore {
    // ---- operations --------------------------------------------------------

    fn begin_op(
        &mut self,
        kind: OpKind,
        profile: Option<&str>,
        profile_hash: Option<&str>,
        parent: Option<i64>,
    ) -> Result<i64> {
        let started_at = now_rfc3339()?;
        let version = env!("CARGO_PKG_VERSION");
        self.conn.execute(
            "INSERT INTO operations
                (kind, profile, profile_hash, parent_op, status, started_at, candylane_version)
             VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6)",
            params![
                op_kind_to_str(kind),
                profile,
                profile_hash,
                parent,
                started_at,
                version,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn set_op_status(&mut self, op: i64, status: OpStatus) -> Result<()> {
        let finished_at = now_rfc3339()?;
        self.conn.execute(
            "UPDATE operations SET status=?1, finished_at=?2 WHERE id=?3",
            params![op_status_to_str(status), finished_at, op],
        )?;
        Ok(())
    }

    // ---- actions -----------------------------------------------------------

    fn insert_action(&mut self, op: i64, seq: u32, action: &NewAction) -> Result<i64> {
        let before_str = serde_json::to_string(&action.before)?;
        // undo_json starts as "{}" for pending actions; filled by set_action_applied.
        let undo_str = "{}";
        self.conn.execute(
            "INSERT INTO actions
                (op_id, seq, handler, target, status, before_json, undo_kind, undo_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                op,
                seq as i64,
                handler_kind_to_str(action.handler),
                &action.target.0,
                action_status_to_str(action.status),
                before_str,
                undo_kind_to_str(action.undo_kind),
                undo_str,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn set_action_applied(&mut self, action: i64, after: &Json, undo: &Json) -> Result<()> {
        let after_str = serde_json::to_string(after)?;
        let undo_str = serde_json::to_string(undo)?;
        self.conn.execute(
            "UPDATE actions SET status='applied', after_json=?1, undo_json=?2 WHERE id=?3",
            params![after_str, undo_str, action],
        )?;
        Ok(())
    }

    fn set_action_status(&mut self, action: i64, status: ActionStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE actions SET status=?1 WHERE id=?2",
            params![action_status_to_str(status), action],
        )?;
        Ok(())
    }

    fn bump_undo_attempt(&mut self, action: i64, err: &str) -> Result<u32> {
        self.conn.execute(
            "UPDATE actions
             SET undo_attempts = undo_attempts + 1,
                 undo_error    = ?1
             WHERE id = ?2",
            params![err, action],
        )?;
        let new_count: i64 = self.conn.query_row(
            "SELECT undo_attempts FROM actions WHERE id=?1",
            params![action],
            |row| row.get(0),
        )?;
        Ok(new_count as u32)
    }

    // ---- queries -----------------------------------------------------------

    fn applied_actions_desc(&self, op: i64) -> Result<Vec<RecordedAction>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, op_id, seq, handler, target, status,
                    before_json, after_json, undo_kind, undo_json,
                    undo_attempts, undo_error
             FROM actions
             WHERE op_id=?1 AND status='applied'
             ORDER BY seq DESC",
        )?;
        let rows = stmt.query_map(params![op], row_to_recorded_action)?;
        rows.map(|r| r.map_err(anyhow::Error::from)).collect()
    }

    fn pending_action(&self, op: i64) -> Result<Option<RecordedAction>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, op_id, seq, handler, target, status,
                    before_json, after_json, undo_kind, undo_json,
                    undo_attempts, undo_error
             FROM actions
             WHERE op_id=?1 AND status='pending'
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![op], row_to_recorded_action)?;
        match rows.next() {
            Some(r) => Ok(Some(r?)),
            None => Ok(None),
        }
    }

    fn action_statuses(&self, op: i64) -> Result<Vec<ActionStatus>> {
        let mut stmt = self
            .conn
            .prepare("SELECT status FROM actions WHERE op_id=?1 ORDER BY seq ASC")?;
        let rows = stmt.query_map(params![op], |row| row.get::<_, String>(0))?;
        rows.map(|r| {
            let s = r?;
            action_status_from_str(&s)
        })
        .collect()
    }

    fn unfinished_op(&self) -> Result<Option<i64>> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM operations WHERE status='pending' ORDER BY id ASC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id)
    }

    fn last_applied_op(&self) -> Result<Option<i64>> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM operations
                 WHERE kind='pull' AND status='applied'
                 ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id)
    }
}
