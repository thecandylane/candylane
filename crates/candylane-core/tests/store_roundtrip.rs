//! Integration tests for `SqliteStore` against a real on-disk SQLite file.
//!
//! These are the first tests to actually drive the DB layer end-to-end (the coverage
//! gap flagged in CLAUDE.md). Each test gets its own tempdir so there is no
//! shared state.  `tempfile` is already a dev-dependency.
//!
//! Import path convention: use the crate name, not `crate::`.

use candylane_core::store::{NewAction, SqliteStore, StateStore};
use candylane_core::types::*;
use serde_json::json;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helper: build a minimal NewAction with customisable fields.
// ---------------------------------------------------------------------------

fn new_action(
    handler: HandlerKind,
    target: &str,
    before: serde_json::Value,
    undo_kind: UndoKind,
) -> NewAction {
    NewAction {
        handler,
        target: Target(target.to_string()),
        before,
        undo_kind,
        status: ActionStatus::Pending,
    }
}

// ---------------------------------------------------------------------------
// Test 1: open() creates the DB; a second open() of the same path is idempotent.
// ---------------------------------------------------------------------------

#[test]
fn open_creates_db_and_is_idempotent() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("state.db");

    // First open: file must not exist yet; open() must succeed and create it.
    assert!(!db_path.exists());
    SqliteStore::open(&db_path).expect("first open should succeed");
    assert!(db_path.exists(), "open() must create the DB file");

    // Second open of the same path: must succeed without error (migration is idempotent).
    SqliteStore::open(&db_path).expect("second open of same path should succeed");
}

// ---------------------------------------------------------------------------
// Test 2: begin_op -> unfinished_op (pending) -> set_op_status(Applied)
//         -> last_applied_op returns it -> unfinished_op is now None.
// ---------------------------------------------------------------------------

#[test]
fn op_lifecycle_pull_pending_to_applied() {
    let dir = tempdir().unwrap();
    let mut store = SqliteStore::open(&dir.path().join("state.db")).unwrap();

    // No op yet.
    assert_eq!(store.unfinished_op().unwrap(), None);
    assert_eq!(store.last_applied_op().unwrap(), None);

    let op_id = store
        .begin_op(OpKind::Pull, Some("myprofile"), Some("deadbeef"), None)
        .unwrap();
    assert!(op_id > 0);

    // Pending op must be found by unfinished_op.
    assert_eq!(store.unfinished_op().unwrap(), Some(op_id));

    // last_applied_op looks for status='applied', so still None.
    assert_eq!(store.last_applied_op().unwrap(), None);

    // Transition to Applied.
    store.set_op_status(op_id, OpStatus::Applied).unwrap();

    // Now last_applied_op returns it and unfinished_op is clear.
    assert_eq!(store.last_applied_op().unwrap(), Some(op_id));
    assert_eq!(store.unfinished_op().unwrap(), None);
}

// ---------------------------------------------------------------------------
// Test 3: insert_action with Pending -> pending_action returns it with before intact.
// ---------------------------------------------------------------------------

#[test]
fn insert_action_pending_roundtrip() {
    let dir = tempdir().unwrap();
    let mut store = SqliteStore::open(&dir.path().join("state.db")).unwrap();

    let op_id = store.begin_op(OpKind::Pull, None, None, None).unwrap();

    let before = json!({"installed": false, "version": null});
    let action = new_action(
        HandlerKind::Winget,
        "Microsoft.VisualStudioCode",
        before.clone(),
        UndoKind::BestEffort,
    );

    let action_id = store.insert_action(op_id, 0, &action).unwrap();
    assert!(action_id > 0);

    // pending_action must find it.
    let recorded = store
        .pending_action(op_id)
        .unwrap()
        .expect("should have a pending action");

    assert_eq!(recorded.id, action_id);
    assert_eq!(recorded.op_id, op_id);
    assert_eq!(recorded.seq, 0);
    assert_eq!(recorded.handler, HandlerKind::Winget);
    assert_eq!(recorded.target.0, "Microsoft.VisualStudioCode");
    assert_eq!(recorded.status, ActionStatus::Pending);
    assert_eq!(
        recorded.before, before,
        "before json must round-trip exactly"
    );
    assert_eq!(recorded.undo_kind, UndoKind::BestEffort);
    assert_eq!(recorded.undo_attempts, 0);
    assert!(recorded.undo_error.is_none());
}

// ---------------------------------------------------------------------------
// Test 4: set_action_applied -> applied_actions_desc returns it;
//         after AND undo values round-trip exactly.
// ---------------------------------------------------------------------------

#[test]
fn set_action_applied_roundtrip_after_and_undo() {
    let dir = tempdir().unwrap();
    let mut store = SqliteStore::open(&dir.path().join("state.db")).unwrap();

    let op_id = store.begin_op(OpKind::Pull, None, None, None).unwrap();
    let before = json!({"installed": false});
    let action_id = store
        .insert_action(
            op_id,
            0,
            &new_action(HandlerKind::Winget, "pkg.Id", before, UndoKind::BestEffort),
        )
        .unwrap();

    // Nothing applied yet.
    assert!(store.applied_actions_desc(op_id).unwrap().is_empty());

    let after = json!({"installed": true, "version": "1.2.3", "nested": {"key": [1, 2, 3]}});
    let undo = json!({"action": "uninstall", "pkg": "pkg.Id", "flags": ["--silent"]});

    store.set_action_applied(action_id, &after, &undo).unwrap();

    let applied = store.applied_actions_desc(op_id).unwrap();
    assert_eq!(applied.len(), 1);

    let rec = &applied[0];
    assert_eq!(rec.id, action_id);
    assert_eq!(rec.status, ActionStatus::Applied);
    assert_eq!(
        rec.after.as_ref().expect("after must be set"),
        &after,
        "after json must round-trip exactly"
    );
    assert_eq!(rec.undo, undo, "undo recipe must round-trip exactly");
}

// ---------------------------------------------------------------------------
// Test 5: set_action_status(Reverted) removes it from applied_actions_desc.
// ---------------------------------------------------------------------------

#[test]
fn reverted_action_absent_from_applied_actions_desc() {
    let dir = tempdir().unwrap();
    let mut store = SqliteStore::open(&dir.path().join("state.db")).unwrap();

    let op_id = store.begin_op(OpKind::Pull, None, None, None).unwrap();
    let action_id = store
        .insert_action(
            op_id,
            0,
            &new_action(
                HandlerKind::Dotfile,
                "/etc/hosts",
                json!({}),
                UndoKind::Inverse,
            ),
        )
        .unwrap();

    store
        .set_action_applied(
            action_id,
            &json!({"present": true}),
            &json!({"restore": "backup"}),
        )
        .unwrap();
    assert_eq!(store.applied_actions_desc(op_id).unwrap().len(), 1);

    // Transition to Reverted — must disappear from applied_actions_desc (query filters status='applied').
    store
        .set_action_status(action_id, ActionStatus::Reverted)
        .unwrap();
    assert!(
        store.applied_actions_desc(op_id).unwrap().is_empty(),
        "reverted action must not appear in applied_actions_desc"
    );
}

// ---------------------------------------------------------------------------
// Test 6: bump_undo_attempt increments and persists the error string.
// ---------------------------------------------------------------------------

#[test]
fn bump_undo_attempt_increments_and_persists_error() {
    let dir = tempdir().unwrap();
    let mut store = SqliteStore::open(&dir.path().join("state.db")).unwrap();

    let op_id = store.begin_op(OpKind::Pull, None, None, None).unwrap();
    let action_id = store
        .insert_action(
            op_id,
            0,
            &new_action(
                HandlerKind::Script,
                "setup.ps1",
                json!({}),
                UndoKind::OneWay,
            ),
        )
        .unwrap();

    // Promote to applied first so we can read it back via applied_actions_desc.
    store
        .set_action_applied(action_id, &json!({"ran": true}), &json!({}))
        .unwrap();

    // First bump.
    let count1 = store.bump_undo_attempt(action_id, "first error").unwrap();
    assert_eq!(count1, 1);

    // Verify via applied_actions_desc.
    let applied = store.applied_actions_desc(op_id).unwrap();
    assert_eq!(applied.len(), 1);
    assert_eq!(applied[0].undo_attempts, 1);
    assert_eq!(applied[0].undo_error.as_deref(), Some("first error"));

    // Second bump overwrites the error string and returns 2.
    let count2 = store.bump_undo_attempt(action_id, "second error").unwrap();
    assert_eq!(count2, 2);

    // Read back the updated error via applied_actions_desc.
    let applied2 = store.applied_actions_desc(op_id).unwrap();
    assert_eq!(applied2[0].undo_attempts, 2);
    assert_eq!(applied2[0].undo_error.as_deref(), Some("second error"));
}

// ---------------------------------------------------------------------------
// Test 7: Enum round-trips — one action per HandlerKind; a selection of UndoKind values.
//         Guards the enum<->TEXT mapping ("enums and SQL move together").
// ---------------------------------------------------------------------------

#[test]
fn enum_roundtrips_handler_and_undo_kind() {
    let dir = tempdir().unwrap();
    let mut store = SqliteStore::open(&dir.path().join("state.db")).unwrap();

    let op_id = store.begin_op(OpKind::Pull, None, None, None).unwrap();

    // One entry per HandlerKind combined with a variety of UndoKind values.
    let cases: &[(HandlerKind, &str, UndoKind)] = &[
        (HandlerKind::Winget, "pkg.One", UndoKind::BestEffort),
        (HandlerKind::Dotfile, "/home/u/.zshrc", UndoKind::Inverse),
        (HandlerKind::Script, "teardown.ps1", UndoKind::OneWay),
    ];

    let mut ids: Vec<i64> = Vec::new();
    for (seq, (handler, target, undo_kind)) in cases.iter().enumerate() {
        let id = store
            .insert_action(
                op_id,
                seq as u32,
                &new_action(*handler, target, json!({}), *undo_kind),
            )
            .unwrap();
        // Promote each to applied so they appear in applied_actions_desc.
        store
            .set_action_applied(id, &json!({"ok": true}), &json!({}))
            .unwrap();
        ids.push(id);
    }

    // applied_actions_desc returns newest seq first; we reverse to align with cases ordering.
    let mut applied = store.applied_actions_desc(op_id).unwrap();
    applied.sort_by_key(|r| r.seq); // normalise to seq ASC for comparison

    assert_eq!(applied.len(), cases.len());
    for (rec, (expected_handler, expected_target, expected_undo_kind)) in
        applied.iter().zip(cases.iter())
    {
        assert_eq!(
            rec.handler, *expected_handler,
            "HandlerKind mismatch for target {}",
            expected_target
        );
        assert_eq!(
            rec.target.0, *expected_target,
            "target mismatch for handler {:?}",
            expected_handler
        );
        assert_eq!(
            rec.undo_kind, *expected_undo_kind,
            "UndoKind mismatch for target {}",
            expected_target
        );
    }

    // Also cover UndoKind::Noop via a Skipped-status action read through action_statuses.
    let noop_id = store
        .insert_action(
            op_id,
            cases.len() as u32,
            &NewAction {
                handler: HandlerKind::Winget,
                target: Target("pkg.Noop".to_string()),
                before: json!({}),
                undo_kind: UndoKind::Noop,
                status: ActionStatus::Skipped,
            },
        )
        .unwrap();
    // pending_action won't find it (it's Skipped, not Pending). Verify undo_kind via
    // a fresh pending insert, apply, and read-back instead, to confirm Noop is stored
    // and retrieved without corrupting the TEXT mapping.
    let noop_check_id = store
        .insert_action(
            op_id,
            (cases.len() + 1) as u32,
            &new_action(
                HandlerKind::Dotfile,
                "/tmp/noop-check",
                json!({"x": 1}),
                UndoKind::Noop,
            ),
        )
        .unwrap();
    store
        .set_action_applied(noop_check_id, &json!({"x": 1}), &json!({}))
        .unwrap();

    let all_applied = store.applied_actions_desc(op_id).unwrap();
    let noop_rec = all_applied
        .iter()
        .find(|r| r.id == noop_check_id)
        .expect("noop_check action must be in applied_actions_desc");
    assert_eq!(noop_rec.undo_kind, UndoKind::Noop);

    // Satisfy the compiler — noop_id was written; confirm via action_statuses.
    let statuses = store.action_statuses(op_id).unwrap();
    // The skipped action occupies one of the seq slots; at least one status must be Skipped.
    assert!(
        statuses.contains(&ActionStatus::Skipped),
        "Skipped status must survive a DB round-trip (guards UndoKind::Noop insert path); noop_id={}",
        noop_id
    );
}

// ---------------------------------------------------------------------------
// Test 8: applied_actions_desc ordering — insert seq 0,1,2; returned order is 2,1,0.
// ---------------------------------------------------------------------------

#[test]
fn applied_actions_desc_order_newest_first() {
    let dir = tempdir().unwrap();
    let mut store = SqliteStore::open(&dir.path().join("state.db")).unwrap();

    let op_id = store.begin_op(OpKind::Pull, None, None, None).unwrap();

    // Insert three actions and immediately promote each to applied.
    for seq in 0u32..3 {
        let id = store
            .insert_action(
                op_id,
                seq,
                &new_action(
                    HandlerKind::Dotfile,
                    &format!("/etc/file-{}", seq),
                    json!({"seq": seq}),
                    UndoKind::Inverse,
                ),
            )
            .unwrap();
        store
            .set_action_applied(
                id,
                &json!({"written": true, "seq": seq}),
                &json!({"restore": seq}),
            )
            .unwrap();
    }

    let applied = store.applied_actions_desc(op_id).unwrap();
    assert_eq!(applied.len(), 3, "all three actions must be returned");

    // Verify descending seq: 2, 1, 0.
    let seqs: Vec<u32> = applied.iter().map(|r| r.seq).collect();
    assert_eq!(
        seqs,
        vec![2, 1, 0],
        "applied_actions_desc must return newest seq first"
    );

    // Spot-check the before/after values to confirm the seq ordering maps to the correct rows.
    assert_eq!(applied[0].before, json!({"seq": 2}));
    assert_eq!(applied[1].before, json!({"seq": 1}));
    assert_eq!(applied[2].before, json!({"seq": 0}));
}
