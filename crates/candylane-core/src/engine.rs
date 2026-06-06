//! The transactional engine. This is the keystone: every reviewer-found fix lives
//! here as real control flow.
//!
//!   pull     — intent-before-apply; success is read from probe (inside apply), not exit code
//!   rollback — bounded retries, best-effort CONTINUE, never an infinite loop (CRITICAL #5)
//!   recover  — reconcile the in-flight action BEFORE rolling back (CRITICAL #4 / Finding #1)
//!   finalize — record partially_reverted / revert_failed honestly
//!
//! SCAFFOLD: orchestration is written; leaves (`preflight`, `reboot_pending_check`,
//! reconcile's undo synthesis) are `todo!()`. NOT yet compiled (no toolchain on host).

use crate::handler::Handler;
use crate::store::{NewAction, StateStore};
use crate::types::*;
use crate::Result;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Resolves a `HandlerKind` to its handler. Built once at startup with the three
/// concrete handlers (winget needs a `WingetExecutor` injected).
pub trait HandlerRegistry {
    fn get(&self, kind: HandlerKind) -> &dyn Handler;
}

/// A parsed profile: an ordered list of desired items.
pub struct Profile {
    pub name: String,
    pub hash: String,
    pub items: Vec<Item>,
}

pub struct Engine<'a> {
    pub store: &'a mut dyn StateStore,
    pub handlers: &'a dyn HandlerRegistry,
    pub backups_root: PathBuf, // ~/.candylane/backups
    pub timeout: Duration,
    pub max_undo_attempts: u32,
}

impl<'a> Engine<'a> {
    fn ctx<'c>(&self, backups_dir: &'c Path) -> ApplyCtx<'c> {
        ApplyCtx {
            backups_dir,
            timeout: self.timeout,
            dry_run: false,
            max_undo_attempts: self.max_undo_attempts,
        }
    }

    /// Apply a profile. Atomic at the operation level: either it fully applies, or it
    /// rolls back to clean and returns the error.
    pub fn pull(&mut self, profile: &Profile) -> Result<()> {
        self.preflight()?; // winget present? reboot already pending? (abort early)

        let op =
            self.store
                .begin_op(OpKind::Pull, Some(&profile.name), Some(&profile.hash), None)?;
        let backups_dir = self.backups_root.join(op.to_string());

        for (i, item) in profile.items.iter().enumerate() {
            let seq = i as u32;
            let handler = self.handlers.get(item.handler_kind());
            let target = item.target();
            let probe = handler.probe(&target)?;

            match handler.plan(item, &probe)? {
                // Already satisfied — record a no-op so history is complete (idempotency).
                None => {
                    self.store.insert_action(
                        op,
                        seq,
                        &NewAction {
                            handler: item.handler_kind(),
                            target,
                            before: probe.0,
                            undo_kind: UndoKind::Noop,
                            status: ActionStatus::Skipped,
                        },
                    )?;
                }
                Some(planned) => {
                    let aid = self.store.insert_action(
                        op,
                        seq,
                        &NewAction {
                            handler: planned.handler,
                            target: planned.target.clone(),
                            before: planned.before.clone(),
                            undo_kind: planned.undo_kind,
                            status: ActionStatus::Pending, // intent BEFORE apply
                        },
                    )?;

                    let ctx = self.ctx(&backups_dir);
                    match handler.apply(&planned, &ctx) {
                        // apply() re-probed internally: Ok means the system really changed.
                        Ok(applied) => {
                            self.store
                                .set_action_applied(aid, &applied.after, &applied.undo)?;
                            // A package may have set reboot-pending. If so, abort before it
                            // poisons the next install (Finding #5). The action is already
                            // recorded applied, so rollback can undo it.
                            if self.reboot_pending()? {
                                self.rollback(op)?;
                                self.finalize_op(op)?;
                                anyhow::bail!(
                                    "reboot pending after applying {:?} — rolled back",
                                    planned.target
                                );
                            }
                        }
                        Err(e) => {
                            self.store.set_action_status(aid, ActionStatus::Failed)?;
                            self.rollback(op)?;
                            self.finalize_op(op)?;
                            return Err(e);
                        }
                    }
                }
            }
        }

        self.store.set_op_status(op, OpStatus::Applied)?;
        Ok(())
    }

    /// Undo every applied action of `op` in reverse order. Best-effort: a permanently
    /// failing undo is capped, marked `undo_failed`, and rollback CONTINUES. Never loops
    /// forever, never aborts the whole rollback on one stuck action (CRITICAL #5).
    pub fn rollback(&mut self, op: i64) -> Result<()> {
        let actions = self.store.applied_actions_desc(op)?;
        for action in actions {
            let handler = self.handlers.get(action.handler);
            let backups_dir = self.backups_root.join(op.to_string());
            let ctx = self.ctx(&backups_dir);

            let mut attempts = action.undo_attempts;
            let mut reverted = false;
            while attempts < self.max_undo_attempts {
                match handler.undo(&action, &ctx) {
                    Ok(()) => {
                        self.store
                            .set_action_status(action.id, ActionStatus::Reverted)?;
                        reverted = true;
                        break;
                    }
                    Err(e) => {
                        attempts = self.store.bump_undo_attempt(action.id, &e.to_string())?;
                    }
                }
            }
            if !reverted {
                // Give up on THIS action, keep rolling back the rest.
                self.store
                    .set_action_status(action.id, ActionStatus::UndoFailed)?;
            }
        }
        Ok(())
    }

    /// Recover from an interrupted pull. The order matters: reconcile the in-flight
    /// action FIRST (its real-world outcome is unknown after a crash), then roll back.
    pub fn recover(&mut self) -> Result<()> {
        if let Some(op) = self.store.unfinished_op()? {
            self.reconcile(op)?;
            self.rollback(op)?;
            self.finalize_op(op)?;
        }
        Ok(())
    }

    /// Finding #1 — the catch that matters most. After a crash, the one `pending`
    /// action may or may not have taken effect. Probe the real system and decide:
    ///   real state changed  → it applied; synthesize after+undo, mark Applied
    ///   real state == before → it never ran; mark Skipped
    /// Without this, rollback (which only touches Applied rows) silently strands it.
    fn reconcile(&mut self, op: i64) -> Result<()> {
        if let Some(action) = self.store.pending_action(op)? {
            let handler = self.handlers.get(action.handler);
            let real = handler.probe(&action.target)?;
            if real.0 != action.before {
                // It applied before the crash. Build the undo recipe from observed state
                // so rollback can reverse it. Handler-specific (e.g. winget: uninstall the
                // version now present). This is the one genuinely new leaf reconcile needs.
                let _ = &real;
                todo!("handler.synthesize_undo(&action.target, &real) -> (after, undo); set_action_applied");
            } else {
                self.store
                    .set_action_status(action.id, ActionStatus::Skipped)?;
            }
        }
        Ok(())
    }

    /// Record the honest operation outcome after a rollback.
    fn finalize_op(&mut self, op: i64) -> Result<()> {
        let statuses = self.store.action_statuses(op)?;
        let status = if statuses.contains(&ActionStatus::UndoFailed) {
            OpStatus::RevertFailed
        } else if statuses.contains(&ActionStatus::Applied) {
            OpStatus::PartiallyReverted
        } else {
            OpStatus::Reverted
        };
        self.store.set_op_status(op, status)?;
        Ok(())
    }

    /// Explicit user-invoked revert of the last applied pull.
    pub fn revert_last(&mut self) -> Result<()> {
        match self.store.last_applied_op()? {
            Some(op) => {
                self.rollback(op)?;
                self.finalize_op(op)?;
                Ok(())
            }
            None => anyhow::bail!("nothing to revert"),
        }
    }

    /// Dry-run: compute the plan without touching the machine. Powers `candylane diff`.
    pub fn diff(&self, profile: &Profile) -> Result<Vec<PlannedAction>> {
        let mut plan = Vec::new();
        for item in &profile.items {
            let handler = self.handlers.get(item.handler_kind());
            let probe = handler.probe(&item.target())?;
            if let Some(pa) = handler.plan(item, &probe)? {
                plan.push(pa);
            }
        }
        Ok(plan)
    }

    // ---- leaves (Lane A tail / Lane B) -----------------------------------------

    /// winget present? a reboot already pending before we start? (abort early)
    fn preflight(&self) -> Result<()> {
        todo!("check winget.exe on PATH; bail if reboot already pending")
    }

    /// Read the OS reboot-pending flags (CBS RebootPending + Session Manager
    /// PendingFileRenameOperations). Windows-specific.
    fn reboot_pending(&self) -> Result<bool> {
        todo!("probe reboot-pending registry/state")
    }
}
