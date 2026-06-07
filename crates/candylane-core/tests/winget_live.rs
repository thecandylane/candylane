//! Live winget integration tests — Windows-only, `#[ignore]`d.
//!
//! These drive the REAL `WingetHandler` (which wires the real `winget.exe` executor on
//! Windows) against the actual winget on the host. They are:
//!   - `#[cfg(windows)]` — the real executor only exists on Windows; on other targets
//!     this file compiles to nothing.
//!   - `#[ignore]` — they shell out to winget and depend on machine state, so they never
//!     run in a normal `cargo test`. Run explicitly on a Windows host with:
//!     `cargo test -p candylane-core --test winget_live -- --ignored --nocapture`
//!
//! The read-only probes (`probe_*`) are safe to run anywhere winget exists. The
//! mutating round-trip (`install_then_undo_round_trip`) installs and then uninstalls a
//! small, license-clean package; it is additionally guarded so it only runs when the
//! package is NOT already present (so it never removes something the user had).

#![cfg(windows)]

use candylane_core::handlers::WingetHandler;
use candylane_core::types::{ApplyCtx, HandlerKind, Item, Target};
use candylane_core::Handler;
use std::time::Duration;

fn ctx(dir: &std::path::Path) -> ApplyCtx<'_> {
    ApplyCtx {
        backups_dir: dir,
        timeout: Duration::from_secs(600),
        dry_run: false,
        max_undo_attempts: 3,
    }
}

/// A package that is reliably present on this dev box (we installed it to build).
const PRESENT_PKG: &str = "Rustlang.Rustup";
/// An id that cannot exist.
const ABSENT_PKG: &str = "Candylane.Definitely.Not.A.Real.Package";

/// probe() against a known-installed package reports installed + a version string.
#[test]
#[ignore = "hits real winget; run with --ignored on a Windows host"]
fn probe_present_package_is_installed() {
    let h = WingetHandler::new();
    let p = h.probe(&Target(PRESENT_PKG.into())).unwrap();
    assert_eq!(
        p.0["installed"], true,
        "expected {PRESENT_PKG} to be installed; probe = {}",
        p.0
    );
    assert!(
        p.0["version"].is_string(),
        "expected a version string; probe = {}",
        p.0
    );
}

/// probe() against a bogus id reports not-installed (and does not error on winget's
/// non-zero "no package found" exit code).
#[test]
#[ignore = "hits real winget; run with --ignored on a Windows host"]
fn probe_absent_package_is_not_installed() {
    let h = WingetHandler::new();
    let p = h.probe(&Target(ABSENT_PKG.into())).unwrap();
    assert_eq!(
        p.0["installed"], false,
        "expected {ABSENT_PKG} absent; probe = {}",
        p.0
    );
}

/// plan() against an already-installed package is a no-op (idempotent re-pull).
#[test]
#[ignore = "hits real winget; run with --ignored on a Windows host"]
fn plan_present_package_is_noop() {
    let h = WingetHandler::new();
    let probe = h.probe(&Target(PRESENT_PKG.into())).unwrap();
    let item = Item::Winget {
        pkg: PRESENT_PKG.into(),
    };
    let planned = h.plan(&item, &probe).unwrap();
    assert!(
        planned.is_none(),
        "an installed package must plan to None; got {planned:?}"
    );
}

/// Full real round-trip: install a small package, confirm via probe, then undo
/// (uninstall) and confirm it's gone. GUARDED: only runs if the package is absent to
/// begin with, so it never removes a user's pre-existing install.
///
/// Uses ZoomIt (Microsoft.Sysinternals.ZoomIt) — a tiny, Microsoft-published Sysinternals
/// utility: fast to install/remove and license-clean.
#[test]
#[ignore = "MUTATES the machine (installs+removes ZoomIt); run with --ignored on a Windows host"]
fn install_then_undo_round_trip() {
    const PKG: &str = "Microsoft.Sysinternals.ZoomIt";
    let tmp = std::env::temp_dir();
    let h = WingetHandler::new();

    eprintln!("\n========== WINGET LIVE ROUND-TRIP: {PKG} ==========");

    // Precondition: HARD-FAIL (not skip) if the package is already present. A skip-green
    // here would false-pass on a dirty machine — and a prior run that died between install
    // and revert leaves exactly that state. This is an explicitly-invoked `--ignored`
    // mutating test on a dev/CI box, so a dirty precondition is a real problem to surface,
    // not to paper over. (Worse than no test: a mutating test that passes green while
    // leaving the machine dirty.)
    let before = h.probe(&Target(PKG.into())).unwrap();
    eprintln!("[1/4] probe BEFORE      → {}", before.0);
    assert_eq!(
        before.0["installed"], false,
        "PRECONDITION FAILED: {PKG} is already installed at test start. Either a prior run \
         died between install and revert, or it's genuinely on this machine. This mutating \
         test refuses to run on a dirty precondition. Clean up with: \
         winget uninstall --id {PKG} --exact"
    );

    // plan → Some (absent → install)
    let item = Item::Winget { pkg: PKG.into() };
    let planned = h
        .plan(&item, &before)
        .unwrap()
        .expect("absent package must plan to Some");
    assert_eq!(planned.handler, HandlerKind::Winget);
    eprintln!(
        "       plan            → install (undo_kind = {:?})",
        planned.undo_kind
    );

    // apply → installs, re-probes, returns installed after-state.
    eprintln!("[2/4] winget install... (this mutates the machine)");
    let applied = h.apply(&planned, &ctx(&tmp)).unwrap();
    eprintln!("       apply after     → {}", applied.after);
    eprintln!("       undo recipe     → {}", applied.undo);
    assert_eq!(
        applied.after["installed"], true,
        "apply must leave the package installed; after = {}",
        applied.after
    );
    assert_eq!(applied.undo["was_present_before"], false);

    // Confirm independently via a fresh probe (not the apply's own re-probe).
    let mid = h.probe(&Target(PKG.into())).unwrap();
    eprintln!("[3/4] probe AFTER apply → {}", mid.0);
    assert_eq!(
        mid.0["installed"], true,
        "package should be present post-apply"
    );

    // Build a RecordedAction from the applied recipe and undo it.
    let recorded = candylane_core::types::RecordedAction {
        id: 1,
        op_id: 1,
        seq: 0,
        handler: HandlerKind::Winget,
        target: Target(PKG.into()),
        status: candylane_core::types::ActionStatus::Applied,
        before: before.0.clone(),
        after: Some(applied.after.clone()),
        undo_kind: candylane_core::types::UndoKind::BestEffort,
        undo: applied.undo.clone(),
        undo_attempts: 0,
        undo_error: None,
    };
    eprintln!("       winget uninstall... (reverting)");
    h.undo(&recorded, &ctx(&tmp)).unwrap();

    // Confirm it's gone.
    let after = h.probe(&Target(PKG.into())).unwrap();
    eprintln!("[4/4] probe AFTER undo  → {}", after.0);
    assert_eq!(
        after.0["installed"], false,
        "undo must remove the package we installed; after = {}",
        after.0
    );
    eprintln!("========== ROUND-TRIP OK: absent → installed → absent ==========\n");
}

// ============================================================================
// Engine-level: winget through the REAL engine + SqliteStore (closes Lane B)
// ============================================================================

/// The Lane B closure: a full `Engine::pull` (which runs the real `PowerShellRebootCheck`
/// preflight, persists the action + undo recipe to a real `SqliteStore`) installs a winget
/// package, then `Engine::revert_last` reads the recipe back from SQLite and uninstalls it.
/// Proves winget is WIRED end-to-end — handler + engine + store + logging — not just that
/// the handler works in isolation.
///
/// Mutating, with a HARD-FAIL precondition (not a skip): `PKG` must be absent at start. A
/// skip-green would false-pass on a dirty machine — including the exact state a prior run
/// leaves if it dies between pull and revert. Also doubles as the live verification of
/// `PowerShellRebootCheck` (the pull's preflight calls it for real; fail-open means a probe
/// hiccup proceeds, a genuine CBS/WU pending aborts with a clear message).
#[test]
#[ignore = "MUTATES the machine via the engine (installs+reverts ZoomIt); Windows host only"]
fn engine_pull_then_revert_winget_through_store() {
    use candylane_core::engine::{Engine, Profile};
    use candylane_core::reboot::PowerShellRebootCheck;
    use candylane_core::store::{SqliteStore, StateStore};
    use candylane_core::Handlers;

    const PKG: &str = "Microsoft.Sysinternals.ZoomIt";

    // Precondition: HARD-FAIL if already present (a prior run may have died mid-way). A skip
    // here would false-pass while leaving the machine dirty.
    let guard = WingetHandler::new();
    assert_eq!(
        guard.probe(&Target(PKG.into())).unwrap().0["installed"],
        false,
        "PRECONDITION FAILED: {PKG} is already installed at test start (a prior run may have \
         died between pull and revert). Clean up with: winget uninstall --id {PKG} --exact"
    );

    let tmp = tempfile::TempDir::new().unwrap();
    let db_path = tmp.path().join("state.db");
    let backups_root = tmp.path().join("backups");

    let profile = Profile {
        name: "winget-engine-slice".into(),
        hash: "live".into(),
        items: vec![Item::Winget { pkg: PKG.into() }],
    };

    eprintln!("\n========== ENGINE WINGET PULL→REVERT (through SqliteStore): {PKG} ==========");

    // ── PULL ──────────────────────────────────────────────────────────────────
    {
        let mut store = SqliteStore::open(&db_path).unwrap();
        let handlers = Handlers::new();
        let reboot = PowerShellRebootCheck::new();
        let mut engine = Engine {
            store: &mut store,
            handlers: &handlers,
            reboot_check: &reboot,
            backups_root: backups_root.clone(),
            timeout: Duration::from_secs(600),
            max_undo_attempts: 3,
        };
        engine
            .pull(&profile)
            .expect("engine.pull should install the winget package");
    }
    eprintln!("[pull] complete");

    // Independently confirm installed.
    assert_eq!(
        guard.probe(&Target(PKG.into())).unwrap().0["installed"],
        true,
        "package must be installed after engine.pull"
    );

    // The recipe was persisted: there is an applied op to revert.
    {
        let store = SqliteStore::open(&db_path).unwrap();
        assert!(
            store.last_applied_op().unwrap().is_some(),
            "pull must have recorded an applied op in the store"
        );
    }

    // ── REVERT ────────────────────────────────────────────────────────────────
    {
        let mut store = SqliteStore::open(&db_path).unwrap();
        let handlers = Handlers::new();
        let reboot = PowerShellRebootCheck::new();
        let mut engine = Engine {
            store: &mut store,
            handlers: &handlers,
            reboot_check: &reboot,
            backups_root: backups_root.clone(),
            timeout: Duration::from_secs(600),
            max_undo_attempts: 3,
        };
        engine
            .revert_last()
            .expect("engine.revert_last should uninstall the winget package");
    }
    eprintln!("[revert] complete");

    // Independently confirm gone.
    assert_eq!(
        guard.probe(&Target(PKG.into())).unwrap().0["installed"],
        false,
        "package must be uninstalled after engine.revert_last"
    );
    eprintln!("========== ENGINE ROUND-TRIP OK: pull installed → revert removed ==========\n");
}
