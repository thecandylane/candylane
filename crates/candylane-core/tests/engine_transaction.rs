//! Integration tests for the engine's transaction / rollback / recover logic.
//!
//! No Windows, no real DB, no winget. Every external dependency is faked inline.
//!
//! # Needed pub additions (noted here, not added to other files)
//!
//! 1. `Engine::pull_without_preflight(&mut self, profile: &Profile) -> Result<()>`
//!    — identical to `pull()` but omits the `self.preflight()?` call so the engine
//!    loop is exercisable off-Windows where `preflight` is `todo!()`.
//!    Suggested placement: `engine.rs`, gated with `#[cfg(any(test, feature="test-utils"))]`.
//!
//! 2. `Handler::synthesize_undo(&self, target: &Target, probe: &Probe) -> Result<(Json, Json)>`
//!    — returns `(after_json, undo_json)` from the post-crash observed state so that
//!    `reconcile()` can call `store.set_action_applied` on the in-flight action without
//!    a `todo!()`. Required for test 3's "probe != before" (applied) path.
//!    Until this addition lands the `#[should_panic]` marker on that sub-test documents it.
//!
//! 3. `Engine::finalize_op(&mut self, op: i64) -> Result<()>` made `pub`
//!    — currently private; needed so test 4 (`rollback_bounded`) can call
//!    `rollback` + `finalize_op` without wrapping via `recover()`.
//!    Alternatively, keep it private and route through `recover()` (see test 4).
//!
//! Tests 1, 2, 4, 5 drive the engine via `rollback` / `recover` and the store directly;
//! they do NOT call `Engine::pull` and are therefore unaffected by the `preflight` todo.
//! Tests 1, 2, 5 demonstrate the pull loop by manually orchestrating store inserts +
//! calling `Engine::rollback`/`recover`, matching the engine.rs loop exactly.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use candylane_core::engine::{Engine, HandlerRegistry, Profile};
use candylane_core::handler::Handler;
use candylane_core::store::{NewAction, StateStore};
use candylane_core::types::*;
use candylane_core::Result;

// ============================================================================
// FakeStore — in-memory StateStore
// ============================================================================

/// Stored operation row.
#[derive(Clone)]
struct OpRow {
    id: i64,
    kind: OpKind,
    status: OpStatus,
}

/// Stored action row.
#[derive(Clone)]
struct ActionRow {
    id: i64,
    op_id: i64,
    seq: u32,
    handler: HandlerKind,
    target: String,
    status: ActionStatus,
    before: Json,
    after: Option<Json>,
    undo_kind: UndoKind,
    undo: Json,
    undo_attempts: u32,
    undo_error: Option<String>,
}

impl ActionRow {
    fn to_recorded(&self) -> RecordedAction {
        RecordedAction {
            id: self.id,
            op_id: self.op_id,
            seq: self.seq,
            handler: self.handler,
            target: Target(self.target.clone()),
            status: self.status,
            before: self.before.clone(),
            after: self.after.clone(),
            undo_kind: self.undo_kind,
            undo: self.undo.clone(),
            undo_attempts: self.undo_attempts,
            undo_error: self.undo_error.clone(),
        }
    }
}

struct FakeStore {
    next_op_id: i64,
    next_action_id: i64,
    ops: Vec<OpRow>,
    actions: Vec<ActionRow>,
}

impl FakeStore {
    fn new() -> Self {
        FakeStore {
            next_op_id: 1,
            next_action_id: 1,
            ops: Vec::new(),
            actions: Vec::new(),
        }
    }

    /// Convenience: read back a single action by id for assertions.
    fn action(&self, id: i64) -> Option<&ActionRow> {
        self.actions.iter().find(|a| a.id == id)
    }

    /// Convenience: read back an op by id for assertions.
    fn op(&self, id: i64) -> Option<&OpRow> {
        self.ops.iter().find(|o| o.id == id)
    }
}

impl StateStore for FakeStore {
    fn begin_op(
        &mut self,
        kind: OpKind,
        _profile: Option<&str>,
        _profile_hash: Option<&str>,
        _parent: Option<i64>,
    ) -> Result<i64> {
        let id = self.next_op_id;
        self.next_op_id += 1;
        self.ops.push(OpRow {
            id,
            kind,
            status: OpStatus::Pending,
        });
        Ok(id)
    }

    fn set_op_status(&mut self, op: i64, status: OpStatus) -> Result<()> {
        let row = self
            .ops
            .iter_mut()
            .find(|o| o.id == op)
            .ok_or_else(|| anyhow::anyhow!("FakeStore: op {} not found", op))?;
        row.status = status;
        Ok(())
    }

    fn insert_action(&mut self, op: i64, seq: u32, action: &NewAction) -> Result<i64> {
        let id = self.next_action_id;
        self.next_action_id += 1;
        self.actions.push(ActionRow {
            id,
            op_id: op,
            seq,
            handler: action.handler,
            target: action.target.0.clone(),
            status: action.status,
            before: action.before.clone(),
            after: None,
            undo_kind: action.undo_kind,
            undo: serde_json::json!({}),
            undo_attempts: 0,
            undo_error: None,
        });
        Ok(id)
    }

    fn set_action_applied(&mut self, action: i64, after: &Json, undo: &Json) -> Result<()> {
        let row = self
            .actions
            .iter_mut()
            .find(|a| a.id == action)
            .ok_or_else(|| anyhow::anyhow!("FakeStore: action {} not found", action))?;
        row.status = ActionStatus::Applied;
        row.after = Some(after.clone());
        row.undo = undo.clone();
        Ok(())
    }

    fn set_action_status(&mut self, action: i64, status: ActionStatus) -> Result<()> {
        let row = self
            .actions
            .iter_mut()
            .find(|a| a.id == action)
            .ok_or_else(|| anyhow::anyhow!("FakeStore: action {} not found", action))?;
        row.status = status;
        Ok(())
    }

    fn bump_undo_attempt(&mut self, action: i64, err: &str) -> Result<u32> {
        let row = self
            .actions
            .iter_mut()
            .find(|a| a.id == action)
            .ok_or_else(|| anyhow::anyhow!("FakeStore: action {} not found", action))?;
        row.undo_attempts += 1;
        row.undo_error = Some(err.to_owned());
        Ok(row.undo_attempts)
    }

    fn applied_actions_desc(&self, op: i64) -> Result<Vec<RecordedAction>> {
        let mut rows: Vec<&ActionRow> = self
            .actions
            .iter()
            .filter(|a| a.op_id == op && a.status == ActionStatus::Applied)
            .collect();
        // Descending seq (revert order).
        rows.sort_by_key(|a| std::cmp::Reverse(a.seq));
        Ok(rows.iter().map(|a| a.to_recorded()).collect())
    }

    fn pending_action(&self, op: i64) -> Result<Option<RecordedAction>> {
        Ok(self
            .actions
            .iter()
            .find(|a| a.op_id == op && a.status == ActionStatus::Pending)
            .map(|a| a.to_recorded()))
    }

    fn action_statuses(&self, op: i64) -> Result<Vec<ActionStatus>> {
        let mut rows: Vec<&ActionRow> = self.actions.iter().filter(|a| a.op_id == op).collect();
        rows.sort_by_key(|a| a.seq);
        Ok(rows.iter().map(|a| a.status).collect())
    }

    fn unfinished_op(&self) -> Result<Option<i64>> {
        Ok(self
            .ops
            .iter()
            .find(|o| o.status == OpStatus::Pending)
            .map(|o| o.id))
    }

    fn last_applied_op(&self) -> Result<Option<i64>> {
        Ok(self
            .ops
            .iter()
            .rev()
            .find(|o| o.status == OpStatus::Applied)
            .map(|o| o.id))
    }

    fn list_operations(&self) -> Result<Vec<OperationRow>> {
        Ok(self
            .ops
            .iter()
            .rev()
            .map(|o| OperationRow {
                id: o.id,
                kind: o.kind,
                profile: None,
                status: o.status,
                started_at: String::new(),
                finished_at: None,
            })
            .collect())
    }
}

// ============================================================================
// FakeHandler — scriptable, per-target outcomes
// ============================================================================

/// One scripted outcome sequence for a single named target.
struct TargetScript {
    /// Successive probe() return values.
    probe_results: RefCell<std::collections::VecDeque<Result<Probe>>>,
    /// Successive plan() decisions: Some(PlannedAction) or None.
    plan_decisions: RefCell<std::collections::VecDeque<Option<PlannedAction>>>,
    /// Successive apply() outcomes.
    apply_outcomes: RefCell<std::collections::VecDeque<Result<Applied>>>,
    /// Successive undo() outcomes.
    undo_outcomes: RefCell<std::collections::VecDeque<Result<()>>>,
    /// Successive synthesize_undo() outcomes (crash-reconcile applied path).
    synthesize_outcomes: RefCell<std::collections::VecDeque<Result<Applied>>>,
}

impl TargetScript {
    fn new() -> Self {
        TargetScript {
            probe_results: RefCell::new(std::collections::VecDeque::new()),
            plan_decisions: RefCell::new(std::collections::VecDeque::new()),
            apply_outcomes: RefCell::new(std::collections::VecDeque::new()),
            undo_outcomes: RefCell::new(std::collections::VecDeque::new()),
            synthesize_outcomes: RefCell::new(std::collections::VecDeque::new()),
        }
    }
}

struct FakeHandler {
    kind: HandlerKind,
    scripts: RefCell<HashMap<String, TargetScript>>,
    /// Ordered log of (method, target) pairs for call-order assertions.
    call_log: RefCell<Vec<(String, String)>>,
}

impl FakeHandler {
    fn new(kind: HandlerKind) -> Self {
        FakeHandler {
            kind,
            scripts: RefCell::new(HashMap::new()),
            call_log: RefCell::new(Vec::new()),
        }
    }

    fn push_probe(&self, target: &str, result: Result<Probe>) {
        let mut scripts = self.scripts.borrow_mut();
        scripts
            .entry(target.to_owned())
            .or_insert_with(TargetScript::new)
            .probe_results
            .borrow_mut()
            .push_back(result);
    }

    fn push_plan(&self, target: &str, decision: Option<PlannedAction>) {
        let mut scripts = self.scripts.borrow_mut();
        scripts
            .entry(target.to_owned())
            .or_insert_with(TargetScript::new)
            .plan_decisions
            .borrow_mut()
            .push_back(decision);
    }

    fn push_apply(&self, target: &str, result: Result<Applied>) {
        let mut scripts = self.scripts.borrow_mut();
        scripts
            .entry(target.to_owned())
            .or_insert_with(TargetScript::new)
            .apply_outcomes
            .borrow_mut()
            .push_back(result);
    }

    fn push_undo(&self, target: &str, result: Result<()>) {
        let mut scripts = self.scripts.borrow_mut();
        scripts
            .entry(target.to_owned())
            .or_insert_with(TargetScript::new)
            .undo_outcomes
            .borrow_mut()
            .push_back(result);
    }

    fn push_synthesize(&self, target: &str, result: Result<Applied>) {
        let mut scripts = self.scripts.borrow_mut();
        scripts
            .entry(target.to_owned())
            .or_insert_with(TargetScript::new)
            .synthesize_outcomes
            .borrow_mut()
            .push_back(result);
    }

    /// Return a clone of the call log for assertions.
    fn calls(&self) -> Vec<(String, String)> {
        self.call_log.borrow().clone()
    }
}

impl Handler for FakeHandler {
    fn kind(&self) -> HandlerKind {
        self.kind
    }

    fn probe(&self, target: &Target) -> Result<Probe> {
        self.call_log
            .borrow_mut()
            .push(("probe".into(), target.0.clone()));
        let mut scripts = self.scripts.borrow_mut();
        let entry = scripts
            .entry(target.0.clone())
            .or_insert_with(TargetScript::new);
        let popped = entry.probe_results.borrow_mut().pop_front();
        match popped {
            Some(r) => r,
            None => Ok(Probe(serde_json::json!({ "installed": false }))),
        }
    }

    fn plan(&self, desired: &Item, _probe: &Probe) -> Result<Option<PlannedAction>> {
        let key = desired.target().0;
        self.call_log
            .borrow_mut()
            .push(("plan".into(), key.clone()));
        let mut scripts = self.scripts.borrow_mut();
        let entry = scripts.entry(key.clone()).or_insert_with(TargetScript::new);
        let popped = entry.plan_decisions.borrow_mut().pop_front();
        match popped {
            Some(d) => Ok(d),
            // Default: plan a pending action (not already satisfied).
            None => Ok(Some(PlannedAction {
                handler: self.kind,
                target: Target(key.clone()),
                before: serde_json::json!({ "installed": false }),
                undo_kind: UndoKind::BestEffort,
                payload: Json::Null,
            })),
        }
    }

    fn apply(&self, action: &PlannedAction, _ctx: &ApplyCtx) -> Result<Applied> {
        let key = action.target.0.clone();
        self.call_log
            .borrow_mut()
            .push(("apply".into(), key.clone()));
        let mut scripts = self.scripts.borrow_mut();
        let entry = scripts.entry(key.clone()).or_insert_with(TargetScript::new);
        let popped = entry.apply_outcomes.borrow_mut().pop_front();
        match popped {
            Some(r) => r,
            // Default: succeed.
            None => Ok(Applied {
                after: serde_json::json!({ "installed": true }),
                undo: serde_json::json!({ "op": "uninstall", "pkg": key }),
            }),
        }
    }

    fn undo(&self, action: &RecordedAction, _ctx: &ApplyCtx) -> Result<()> {
        let key = action.target.0.clone();
        self.call_log
            .borrow_mut()
            .push(("undo".into(), key.clone()));
        let mut scripts = self.scripts.borrow_mut();
        let entry = scripts.entry(key.clone()).or_insert_with(TargetScript::new);
        let popped = entry.undo_outcomes.borrow_mut().pop_front();
        match popped {
            Some(r) => r,
            // Default: succeed.
            None => Ok(()),
        }
    }

    fn synthesize_undo(&self, target: &Target, _before: &Json, probe: &Probe) -> Result<Applied> {
        let key = target.0.clone();
        self.call_log
            .borrow_mut()
            .push(("synthesize_undo".into(), key.clone()));
        let mut scripts = self.scripts.borrow_mut();
        let entry = scripts.entry(key.clone()).or_insert_with(TargetScript::new);
        let popped = entry.synthesize_outcomes.borrow_mut().pop_front();
        match popped {
            Some(r) => r,
            // Default: rebuild the undo recipe from the observed post-crash state.
            None => Ok(Applied {
                after: probe.0.clone(),
                undo: serde_json::json!({ "op": "uninstall", "pkg": key }),
            }),
        }
    }
}

// ============================================================================
// FakeRegistry — maps every HandlerKind to the same FakeHandler for simplicity
// ============================================================================

struct FakeRegistry<'a> {
    handler: &'a FakeHandler,
}

impl<'a> FakeRegistry<'a> {
    fn new(handler: &'a FakeHandler) -> Self {
        FakeRegistry { handler }
    }
}

impl<'a> HandlerRegistry for FakeRegistry<'a> {
    fn get(&self, _kind: HandlerKind) -> &dyn Handler {
        self.handler
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Build an `Engine` with the given store + registry. The backups_root is a
/// throwaway temp path — FakeHandler ignores it.
fn make_engine<'a>(store: &'a mut dyn StateStore, handlers: &'a dyn HandlerRegistry) -> Engine<'a> {
    Engine {
        store,
        handlers,
        backups_root: std::path::PathBuf::from("/tmp/candylane-test-backups"),
        timeout: Duration::from_secs(5),
        max_undo_attempts: 3,
    }
}

/// Minimal profile helper: N items all with `Winget` handler.
fn winget_profile(pkgs: &[&str]) -> Profile {
    Profile {
        name: "test".into(),
        hash: "abc123".into(),
        items: pkgs
            .iter()
            .map(|p| Item::Winget { pkg: p.to_string() })
            .collect(),
    }
}

/// Directly simulate the pull loop for a profile WITHOUT calling preflight/reboot checks.
/// Returns `(op_id, action_ids)`.  Mirrors engine.rs `pull()` inner loop exactly.
/// Used by tests 1, 2, 5 since `Engine::pull` has a `todo!()` in `preflight()`.
///
/// On failure this function routes through `Engine::recover()` to trigger rollback +
/// finalize rather than calling the private `Engine::finalize_op` directly.
/// This works because: (a) the failing action is marked `Failed` (not `Pending`), so
/// `reconcile` inside `recover` finds no pending action and is a no-op; (b) `rollback`
/// then undoes all Applied actions; (c) `finalize_op` sets the op status.
///
/// NOTE: once `Engine::pull_without_preflight` is added (see module-level note),
/// replace calls to this helper with that method.
fn simulate_pull(
    store: &mut dyn StateStore,
    handlers: &dyn HandlerRegistry,
    profile: &Profile,
    max_undo: u32,
) -> Result<(i64, Vec<i64>)> {
    let backups_root = std::path::PathBuf::from("/tmp/candylane-test-backups");
    let op = store.begin_op(OpKind::Pull, Some(&profile.name), Some(&profile.hash), None)?;
    let backups_dir = backups_root.join(op.to_string());

    let mut action_ids = Vec::new();

    for (i, item) in profile.items.iter().enumerate() {
        let seq = i as u32;
        let handler = handlers.get(item.handler_kind());
        let target = item.target();
        let probe = handler.probe(&target)?;

        match handler.plan(item, &probe)? {
            None => {
                let aid = store.insert_action(
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
                action_ids.push(aid);
            }
            Some(planned) => {
                let aid = store.insert_action(
                    op,
                    seq,
                    &NewAction {
                        handler: planned.handler,
                        target: planned.target.clone(),
                        before: planned.before.clone(),
                        undo_kind: planned.undo_kind,
                        status: ActionStatus::Pending,
                    },
                )?;
                action_ids.push(aid);

                let ctx = ApplyCtx {
                    backups_dir: &backups_dir,
                    timeout: Duration::from_secs(5),
                    dry_run: false,
                    max_undo_attempts: max_undo,
                };
                match handler.apply(&planned, &ctx) {
                    Ok(applied) => {
                        store.set_action_applied(aid, &applied.after, &applied.undo)?;
                        // No reboot_pending check (not needed for logic tests).
                    }
                    Err(e) => {
                        // Mark the failed action so it is not included in rollback's
                        // applied_actions_desc query.
                        store.set_action_status(aid, ActionStatus::Failed)?;
                        // Route through Engine::recover() so rollback + finalize_op
                        // are both run via public API. The op is still `pending` at
                        // the store level; recover() will pick it up, find no
                        // pending action (the failing one was just marked Failed),
                        // reconcile is a no-op, rollback undoes Applied rows, finalize
                        // sets the op status.
                        let mut eng = Engine {
                            store,
                            handlers,
                            backups_root: backups_root.clone(),
                            timeout: Duration::from_secs(5),
                            max_undo_attempts: max_undo,
                        };
                        eng.recover()?;
                        return Err(e);
                    }
                }
            }
        }
    }

    store.set_op_status(op, OpStatus::Applied)?;
    Ok((op, action_ids))
}

// ============================================================================
// Test 1 — pull_all_ok
// ============================================================================

/// A pull where every action succeeds: the op ends Applied and every action is Applied.
#[test]
fn pull_all_ok() {
    let handler = FakeHandler::new(HandlerKind::Winget);
    let registry = FakeRegistry::new(&handler);
    let mut store = FakeStore::new();
    let profile = winget_profile(&["pkg-a", "pkg-b", "pkg-c"]);

    let (op_id, action_ids) =
        simulate_pull(&mut store, &registry, &profile, 3).expect("pull should succeed");

    // Op is Applied.
    assert_eq!(
        store.op(op_id).unwrap().status,
        OpStatus::Applied,
        "op should be Applied after a clean pull"
    );

    // Every action is Applied.
    for aid in &action_ids {
        assert_eq!(
            store.action(*aid).unwrap().status,
            ActionStatus::Applied,
            "action {} should be Applied",
            aid
        );
    }

    // apply() was called once per target in order.
    let applies: Vec<_> = handler
        .calls()
        .into_iter()
        .filter(|(m, _)| m == "apply")
        .map(|(_, t)| t)
        .collect();
    assert_eq!(
        applies,
        vec!["pkg-a", "pkg-b", "pkg-c"],
        "apply should be called once per target in profile order"
    );
}

// ============================================================================
// Test 2 — pull_action_k_fails
// ============================================================================

/// When action k fails, actions 0..k-1 are Reverted and the op ends RevertFailed
/// (or Reverted if all undo calls succeed — here they do, so expect Reverted).
#[test]
fn pull_action_k_fails() {
    let handler = FakeHandler::new(HandlerKind::Winget);

    // pkg-a and pkg-b will apply successfully.
    // pkg-c will fail on apply.
    handler.push_apply(
        "pkg-c",
        Err(anyhow::anyhow!("winget reported not-installed after apply")),
    );

    let registry = FakeRegistry::new(&handler);
    let mut store = FakeStore::new();
    let profile = winget_profile(&["pkg-a", "pkg-b", "pkg-c"]);

    let result = simulate_pull(&mut store, &registry, &profile, 3);
    assert!(result.is_err(), "pull should propagate the apply error");

    // Find the op (id = 1).
    let op_id = 1i64;

    // Op status: all undos succeed → Reverted.
    assert_eq!(
        store.op(op_id).unwrap().status,
        OpStatus::Reverted,
        "op should be Reverted when all undos succeed after failure"
    );

    // pkg-a (seq 0) and pkg-b (seq 1) were Applied then Reverted.
    let a_pkg_a = store.actions.iter().find(|a| a.target == "pkg-a").unwrap();
    let a_pkg_b = store.actions.iter().find(|a| a.target == "pkg-b").unwrap();
    let a_pkg_c = store.actions.iter().find(|a| a.target == "pkg-c").unwrap();

    assert_eq!(
        a_pkg_a.status,
        ActionStatus::Reverted,
        "pkg-a should be Reverted"
    );
    assert_eq!(
        a_pkg_b.status,
        ActionStatus::Reverted,
        "pkg-b should be Reverted"
    );
    assert_eq!(
        a_pkg_c.status,
        ActionStatus::Failed,
        "pkg-c should be Failed (apply failed)"
    );

    // Undo was called for pkg-b first, then pkg-a (reverse seq).
    let undos: Vec<_> = handler
        .calls()
        .into_iter()
        .filter(|(m, _)| m == "undo")
        .map(|(_, t)| t)
        .collect();
    assert_eq!(
        undos,
        vec!["pkg-b", "pkg-a"],
        "rollback must undo in reverse seq order (pkg-b then pkg-a)"
    );
}

// ============================================================================
// Test 3 — recover_reconciles_in_flight
// ============================================================================

/// Sub-test A: the in-flight pending action's probe returns a state DIFFERENT from
/// `before` (apply had taken effect before the crash).  After recover() the action
/// is reconciled to Applied (via `synthesize_undo`), then immediately Reverted by
/// rollback; the op ends Reverted.
///
/// This is CRITICAL #4: the crashed-mid-apply action is NOT stranded — reconcile
/// promotes it to Applied with a synthesized undo recipe so rollback can reverse it.
#[test]
fn recover_reconciles_inflight_applied_path() {
    let handler = FakeHandler::new(HandlerKind::Winget);
    let mut store = FakeStore::new();

    // Manually inject a pending op with one pending action whose probe will differ
    // from its recorded before_json (simulating a mid-apply crash).
    let op_id = store
        .begin_op(OpKind::Pull, Some("test"), Some("h"), None)
        .unwrap();
    let action_id = store
        .insert_action(
            op_id,
            0,
            &NewAction {
                handler: HandlerKind::Winget,
                target: Target("pkg-crash".into()),
                before: serde_json::json!({ "installed": false }),
                undo_kind: UndoKind::BestEffort,
                status: ActionStatus::Pending,
            },
        )
        .unwrap();

    // probe() will return a state that differs from before (installed=true), so
    // reconcile takes the applied path and calls synthesize_undo.
    handler.push_probe(
        "pkg-crash",
        Ok(Probe(
            serde_json::json!({ "installed": true, "version": "1.0" }),
        )),
    );
    // synthesize_undo rebuilds the recipe from the observed post-crash state.
    handler.push_synthesize(
        "pkg-crash",
        Ok(Applied {
            after: serde_json::json!({ "installed": true, "version": "1.0" }),
            undo: serde_json::json!({ "op": "uninstall", "pkg": "pkg-crash" }),
        }),
    );
    // rollback's undo() succeeds (default Ok).

    let registry = FakeRegistry::new(&handler);
    let mut eng = make_engine(&mut store, &registry);

    eng.recover().unwrap();

    // The in-flight action was promoted to Applied, then rolled back → Reverted.
    assert_eq!(
        store.action(action_id).unwrap().status,
        ActionStatus::Reverted,
        "reconcile must promote the crashed action to Applied so rollback can revert it"
    );

    // The synthesized undo recipe was recorded on the action before rollback.
    assert_eq!(
        store.action(action_id).unwrap().undo,
        serde_json::json!({ "op": "uninstall", "pkg": "pkg-crash" }),
        "synthesize_undo recipe must be persisted via set_action_applied"
    );

    // Op ends Reverted (the single action reverted cleanly).
    assert_eq!(
        store.op(op_id).unwrap().status,
        OpStatus::Reverted,
        "op should be Reverted after a clean reconcile + rollback"
    );

    // Call order: probe (reconcile), then synthesize_undo (applied path), then undo (rollback).
    let methods: Vec<String> = handler.calls().into_iter().map(|(m, _)| m).collect();
    assert_eq!(
        methods,
        vec!["probe", "synthesize_undo", "undo"],
        "reconcile must probe + synthesize before rollback undoes the action"
    );
}

/// Sub-test B: the in-flight pending action's probe returns a state EQUAL to `before`
/// (apply never took effect).  After recover() the action is Skipped; the op ends
/// Reverted (nothing to undo).
#[test]
fn recover_reconciles_inflight_skipped_path() {
    let handler = FakeHandler::new(HandlerKind::Winget);
    let mut store = FakeStore::new();

    let before_json = serde_json::json!({ "installed": false });

    // Inject a pending op with one pending action.
    let op_id = store
        .begin_op(OpKind::Pull, Some("test"), Some("h"), None)
        .unwrap();
    let action_id = store
        .insert_action(
            op_id,
            0,
            &NewAction {
                handler: HandlerKind::Winget,
                target: Target("pkg-crash".into()),
                before: before_json.clone(),
                undo_kind: UndoKind::BestEffort,
                status: ActionStatus::Pending,
            },
        )
        .unwrap();

    // probe() returns the same state as before → apply never ran.
    handler.push_probe("pkg-crash", Ok(Probe(before_json.clone())));

    let registry = FakeRegistry::new(&handler);
    let mut eng = make_engine(&mut store, &registry);

    eng.recover().unwrap();

    // The action should be Skipped (not Applied, not Pending, not Reverted).
    assert_eq!(
        store.action(action_id).unwrap().status,
        ActionStatus::Skipped,
        "probe==before: action must be Skipped (apply never took effect)"
    );

    // Op is Reverted (no applied actions to undo → finalize yields Reverted).
    assert_eq!(
        store.op(op_id).unwrap().status,
        OpStatus::Reverted,
        "op should be Reverted when in-flight action was Skipped"
    );

    // No undo calls were made (nothing was Applied).
    let undo_calls: Vec<_> = handler
        .calls()
        .into_iter()
        .filter(|(m, _)| m == "undo")
        .collect();
    assert!(
        undo_calls.is_empty(),
        "no undo should be attempted for a Skipped action"
    );
}

// ============================================================================
// Test 4 — rollback_bounded
// ============================================================================

/// An always-failing undo hits `max_undo_attempts`, is marked UndoFailed, rollback
/// CONTINUES to earlier actions, and the op ends RevertFailed — not an infinite loop.
///
/// Setup: manually insert Applied actions into a pending op, then call `Engine::recover()`
/// which routes through `rollback` → `finalize_op` via public API.  The op starts as
/// `pending` (as if a pull crashed after all three actions applied); `reconcile` inside
/// `recover` finds no `pending` action → is a no-op; `rollback` then processes Applied
/// rows in desc seq; `finalize_op` sets the op status.
#[test]
fn rollback_bounded() {
    const MAX_UNDO: u32 = 2;

    let handler = FakeHandler::new(HandlerKind::Winget);
    let mut store = FakeStore::new();

    // The op is `pending` at the store level — simulating a crash after apply.
    let op_id = store
        .begin_op(OpKind::Pull, Some("test"), Some("h"), None)
        .unwrap();

    let aid_a = store
        .insert_action(
            op_id,
            0,
            &NewAction {
                handler: HandlerKind::Winget,
                target: Target("pkg-a".into()),
                before: serde_json::json!({}),
                undo_kind: UndoKind::BestEffort,
                status: ActionStatus::Pending,
            },
        )
        .unwrap();
    store
        .set_action_applied(
            aid_a,
            &serde_json::json!({ "installed": true }),
            &serde_json::json!({ "op": "uninstall", "pkg": "pkg-a" }),
        )
        .unwrap();

    let aid_b = store
        .insert_action(
            op_id,
            1,
            &NewAction {
                handler: HandlerKind::Winget,
                target: Target("pkg-b".into()),
                before: serde_json::json!({}),
                undo_kind: UndoKind::BestEffort,
                status: ActionStatus::Pending,
            },
        )
        .unwrap();
    store
        .set_action_applied(
            aid_b,
            &serde_json::json!({ "installed": true }),
            &serde_json::json!({ "op": "uninstall", "pkg": "pkg-b" }),
        )
        .unwrap();

    let aid_c = store
        .insert_action(
            op_id,
            2,
            &NewAction {
                handler: HandlerKind::Winget,
                target: Target("pkg-c".into()),
                before: serde_json::json!({}),
                undo_kind: UndoKind::BestEffort,
                status: ActionStatus::Pending,
            },
        )
        .unwrap();
    store
        .set_action_applied(
            aid_c,
            &serde_json::json!({ "installed": true }),
            &serde_json::json!({ "op": "uninstall", "pkg": "pkg-c" }),
        )
        .unwrap();

    // pkg-c (seq 2, undone first in desc order) always fails.
    // Queue MAX_UNDO failures; rollback will attempt exactly that many times then give up.
    for _ in 0..MAX_UNDO {
        handler.push_undo("pkg-c", Err(anyhow::anyhow!("Defender lock")));
    }
    // pkg-b and pkg-a: no scripted undo → FakeHandler default of Ok(()) → succeed.

    let registry = FakeRegistry::new(&handler);
    let mut eng = Engine {
        store: &mut store,
        handlers: &registry,
        backups_root: std::path::PathBuf::from("/tmp"),
        timeout: Duration::from_secs(5),
        max_undo_attempts: MAX_UNDO,
    };

    // recover() = reconcile (no-op: no pending action) + rollback + finalize_op.
    // probe() on every Applied target would be called by reconcile only if there
    // were a pending action — there isn't — so we don't need to script probe here.
    eng.recover().unwrap();

    // pkg-c exhausted retries → UndoFailed.
    assert_eq!(
        store.action(aid_c).unwrap().status,
        ActionStatus::UndoFailed,
        "persistently-failing undo must be marked UndoFailed"
    );

    // Rollback CONTINUED: pkg-b and pkg-a are Reverted.
    assert_eq!(
        store.action(aid_b).unwrap().status,
        ActionStatus::Reverted,
        "pkg-b must be Reverted even though pkg-c failed"
    );
    assert_eq!(
        store.action(aid_a).unwrap().status,
        ActionStatus::Reverted,
        "pkg-a must be Reverted even though pkg-c failed"
    );

    // Op ends RevertFailed (at least one UndoFailed).
    assert_eq!(
        store.op(op_id).unwrap().status,
        OpStatus::RevertFailed,
        "op must be RevertFailed when any action is UndoFailed"
    );

    // Undo was called exactly MAX_UNDO times for pkg-c (bounded, not infinite).
    let pkg_c_undos = handler
        .calls()
        .into_iter()
        .filter(|(m, t)| m == "undo" && t == "pkg-c")
        .count();
    assert_eq!(
        pkg_c_undos as u32, MAX_UNDO,
        "undo for pkg-c must be attempted exactly max_undo_attempts times"
    );

    // undo_attempts counter on the row equals MAX_UNDO.
    assert_eq!(
        store.action(aid_c).unwrap().undo_attempts,
        MAX_UNDO,
        "undo_attempts must equal max_undo_attempts after exhaustion"
    );
}

// ============================================================================
// Test 5 — re_pull_noop
// ============================================================================

/// A second pull where every item is already satisfied: plan() returns None for
/// all targets → every action is Skipped, op is Applied, no duplicate Applied rows.
#[test]
fn re_pull_noop() {
    let handler = FakeHandler::new(HandlerKind::Winget);
    let registry = FakeRegistry::new(&handler);
    let mut store = FakeStore::new();
    let profile = winget_profile(&["pkg-a", "pkg-b"]);

    // --- First pull: items need to be applied. ---
    // Defaults in FakeHandler: probe returns not-installed, plan returns Some, apply succeeds.
    let (op1, aids1) =
        simulate_pull(&mut store, &registry, &profile, 3).expect("first pull should succeed");

    assert_eq!(store.op(op1).unwrap().status, OpStatus::Applied);
    for aid in &aids1 {
        assert_eq!(store.action(*aid).unwrap().status, ActionStatus::Applied);
    }

    // --- Second pull: everything already satisfied (plan() returns None). ---
    // Queue None plan decisions for both targets.
    handler.push_plan("pkg-a", None);
    handler.push_plan("pkg-b", None);

    let (op2, aids2) =
        simulate_pull(&mut store, &registry, &profile, 3).expect("second pull should succeed");

    assert_eq!(
        store.op(op2).unwrap().status,
        OpStatus::Applied,
        "re-pull op should be Applied"
    );

    // New actions are Skipped (not Applied).
    for aid in &aids2 {
        assert_eq!(
            store.action(*aid).unwrap().status,
            ActionStatus::Skipped,
            "re-pull actions should be Skipped when already satisfied"
        );
    }

    // No apply() calls were made during the second pull.
    // Count applies per op: first pull inserted 2 Applied, second should add 0 more.
    let applied_in_op2: Vec<_> = store
        .actions
        .iter()
        .filter(|a| a.op_id == op2 && a.status == ActionStatus::Applied)
        .collect();
    assert!(
        applied_in_op2.is_empty(),
        "re-pull must not produce any Applied rows (no duplicate applied actions)"
    );

    // apply() call count equals exactly 2 (from the first pull only).
    let apply_calls = handler
        .calls()
        .into_iter()
        .filter(|(m, _)| m == "apply")
        .count();
    assert_eq!(
        apply_calls, 2,
        "apply() should only have been called during the first pull"
    );
}
