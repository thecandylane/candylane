//! The transactional engine. This is the keystone: every reviewer-found fix lives
//! here as real control flow.
//!
//!   pull     — intent-before-apply; success is read from probe (inside apply), not exit code
//!   rollback — bounded retries, best-effort CONTINUE, never an infinite loop (CRITICAL #5)
//!   recover  — reconcile the in-flight action BEFORE rolling back (CRITICAL #4 / Finding #1)
//!   finalize — record partially_reverted / revert_failed honestly, clean up backups
//!
//! Implemented and tested on Linux against the real store + dotfile/script handlers.
//! Reboot-pending detection is behind the injectable [`RebootCheck`] seam (mirroring
//! [`HandlerRegistry`] / `WingetExecutor`): the real impl shells PowerShell on Windows,
//! the cross-platform default [`NoRebootCheck`] always reports clear, and tests inject a
//! fake to exercise the abort path. See `crate::reboot`.

use crate::handler::Handler;
use crate::reboot::RebootCheck;
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

/// Whether a tracked target still matches what Candylane recorded. Powers `status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// Probe matches the recorded after-state.
    InSync,
    /// Probe differs from the recorded after-state (changed since the pull).
    Drifted,
    /// No observable state to compare (e.g. scripts probe `null`).
    NotApplicable,
}

/// One row of a `status` report: a tracked target and its current drift state.
pub struct StatusEntry {
    pub handler: HandlerKind,
    pub target: Target,
    pub state: SyncState,
}

pub struct Engine<'a> {
    pub store: &'a mut dyn StateStore,
    pub handlers: &'a dyn HandlerRegistry,
    /// Reboot-pending detector (CRITICAL #5 gate). Inject [`crate::reboot::NoRebootCheck`]
    /// off-Windows / in tests, or [`crate::reboot::PowerShellRebootCheck`] on Windows.
    pub reboot_check: &'a dyn RebootCheck,
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
                        // Honesty: a OneWay action's undo() is a deliberate no-op — nothing
                        // was reversed. Record `UndoSkipped`, distinct from `Reverted`, so
                        // history/diff never claim a reversal that did not happen. The
                        // `undo_kind = one_way` column carries the same truth.
                        let status = if action.undo_kind == UndoKind::OneWay {
                            ActionStatus::UndoSkipped
                        } else {
                            ActionStatus::Reverted
                        };
                        self.store.set_action_status(action.id, status)?;
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
    ///
    /// The recovery is itself recorded as an `OpKind::Recover` op linked to the interrupted
    /// pull, so `history` shows that a crash was recovered rather than the pull silently
    /// changing status on its own.
    pub fn recover(&mut self) -> Result<()> {
        if let Some(op) = self.store.unfinished_op()? {
            let rec = self.store.begin_op(OpKind::Recover, None, None, Some(op))?;
            self.reconcile(op)?;
            self.rollback(op)?;
            let outcome = self.finalize_op(op)?;
            // Mirror the interrupted pull's outcome onto the recovery op.
            self.store.set_op_status(rec, outcome)?;
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
                //
                // Invariant: scripts have a null probe state, so `real == before` always
                // and this branch is unreachable for them. synthesize_undo() on a script
                // bails by design; assert here so a future probe() change breaks loudly in
                // debug builds instead of surfacing as an opaque recovery failure.
                debug_assert_ne!(
                    action.handler,
                    HandlerKind::Script,
                    "script actions must never reach synthesize_undo (null probe state)"
                );
                let applied = handler.synthesize_undo(&action.target, &action.before, &real)?;
                self.store
                    .set_action_applied(action.id, &applied.after, &applied.undo)?;
            } else {
                self.store
                    .set_action_status(action.id, ActionStatus::Skipped)?;
            }
        }
        Ok(())
    }

    /// Record the honest operation outcome after a rollback, and on a fully-clean revert
    /// remove the op's backup directory. Returns the recorded outcome.
    fn finalize_op(&mut self, op: i64) -> Result<OpStatus> {
        let statuses = self.store.action_statuses(op)?;
        // `UndoSkipped` (a one-way action rollback reached) is NOT a failure and NOT a
        // stranded `Applied` — the rollback completed honestly; the irreversibility is
        // surfaced per-action via undo_kind. So it falls through to `Reverted` at the op
        // level. Only a genuinely stuck undo (`UndoFailed`) or a still-`Applied` row
        // downgrades the operation outcome.
        let status = if statuses.contains(&ActionStatus::UndoFailed) {
            OpStatus::RevertFailed
        } else if statuses.contains(&ActionStatus::Applied) {
            OpStatus::PartiallyReverted
        } else {
            OpStatus::Reverted
        };
        self.store.set_op_status(op, status)?;

        // Backups are only needed while an op is applied or partially reverted. After a
        // FULLY clean revert they are dead weight AND a lingering copy of the user's
        // original file bytes — remove them. Keep them for RevertFailed/PartiallyReverted
        // so a manual retry still has the originals. Best-effort: the files are already
        // restored, so a cleanup failure is not fatal to the revert.
        if status == OpStatus::Reverted {
            let dir = self.backups_root.join(op.to_string());
            if dir.exists() {
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
        Ok(status)
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

    /// Validate the machine against the last applied pull: re-probe every applied target
    /// and report whether it still matches the recorded after-state. Read-only. Powers
    /// `candylane status`.
    pub fn status(&self) -> Result<Vec<StatusEntry>> {
        let mut report = Vec::new();
        if let Some(op) = self.store.last_applied_op()? {
            for action in self.store.applied_actions_desc(op)? {
                let handler = self.handlers.get(action.handler);
                let probe = handler.probe(&action.target)?;
                let state = if probe.0.is_null() {
                    SyncState::NotApplicable
                } else if action.after.as_ref() == Some(&probe.0) {
                    SyncState::InSync
                } else {
                    SyncState::Drifted
                };
                report.push(StatusEntry {
                    handler: action.handler,
                    target: action.target,
                    state,
                });
            }
        }
        Ok(report)
    }

    // ---- leaves --------------------------------------------------------------

    /// Abort before we start if a reboot is already pending (CRITICAL #5: a pending
    /// reboot poisons the next install). Delegates to the injected [`RebootCheck`]; the
    /// gate is CBS∨WU (see [`RebootState::must_abort`]). PendingFileRenameOperations is
    /// advisory only — it is True on healthy machines (installers queue file renames as
    /// normal work), so gating on it would refuse pulls on most real systems.
    ///
    /// Spec note: PHASE1_ARCHITECTURE said "reboot-pending → abort"; this defines
    /// reboot-pending as CBS∨WU with PFRO advisory (FOLLOWUPS, locked decisions).
    fn preflight(&self) -> Result<()> {
        let state = self.reboot_check.state()?;
        if state.must_abort() {
            anyhow::bail!(
                "a system reboot is already pending ({}) — reboot, then run the pull",
                state.reasons()
            );
        }
        Ok(())
    }

    /// Mid-pull reboot gate: after an install, did Windows servicing flip a reboot
    /// requirement that would poison the next install? Same CBS∨WU predicate as
    /// [`preflight`] (single source of truth — they must not drift). PFRO is NOT consulted
    /// here: a winget install legitimately queues file renames, so PFRO trips *because the
    /// pull is working* — gating on it would roll back pull #1 of a healthy run.
    fn reboot_pending(&self) -> Result<bool> {
        Ok(self.reboot_check.state()?.must_abort())
    }
}
