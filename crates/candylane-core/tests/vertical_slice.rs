//! THE MONEY TEST — the Phase 1 promise, proven on Linux.
//!
//!   profile → `pull` → the machine you wanted → `revert` → functional-clean vanilla
//!
//! Real `DotfileHandler` + real `ScriptHandler` + real `SqliteStore`, all in a tempdir,
//! NO winget (no Windows). Asserts:
//!   * pull writes the dotfile to its (previously-absent) target AND runs the script
//!     (a marker file appears).
//!   * revert DELETES the dotfile (before-absent → deleted) AND runs the script's undo
//!     (the marker file disappears), and the op ends `Reverted`.
//!
//! Everything lives under absolute temp paths — the real `$HOME` is never touched.

use std::path::Path;
use std::time::Duration;

use candylane_core::engine::Engine;
use candylane_core::profile;
use candylane_core::registry::Handlers;
use candylane_core::store::{SqliteStore, StateStore};
use tempfile::TempDir;

/// Build a profile with one dotfile (absent target) and one paired post_install script.
/// Returns (profile_toml, dotfile_target_path, script_marker_path).
struct Fixture {
    _tmp: TempDir,
    db_path: std::path::PathBuf,
    backups_root: std::path::PathBuf,
    profile_toml: String,
    dotfile_target: std::path::PathBuf,
    dotfile_content: &'static str,
    script_marker: std::path::PathBuf,
}

fn build_fixture() -> Fixture {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Source dotfile with known content.
    let dotfile_src = root.join("src_gitconfig");
    let dotfile_content = "[user]\n\tname = candylane\n";
    std::fs::write(&dotfile_src, dotfile_content).unwrap();

    // Target that does NOT pre-exist (nested dir is created by apply()).
    let dotfile_target = root.join("dest").join(".gitconfig");
    assert!(!dotfile_target.exists());

    // Script: run touches a marker; undo removes it (Inverse).
    let script_marker = root.join("marker.flag");
    assert!(!script_marker.exists());
    let run_cmd = format!("touch {}", script_marker.display());
    let undo_cmd = format!("rm -f {}", script_marker.display());

    // ABSOLUTE paths only — never $HOME.
    let profile_toml = format!(
        r#"
name = "vertical-slice"
version = "0.1"

[dotfiles]
[[dotfiles.file]]
src    = "{src}"
target = "{target}"

[[post_install]]
run  = "{run}"
undo = "{undo}"
"#,
        src = dotfile_src.display(),
        target = dotfile_target.display(),
        run = run_cmd,
        undo = undo_cmd,
    );

    Fixture {
        db_path: root.join("state.db"),
        backups_root: root.join("backups"),
        profile_toml,
        dotfile_target,
        dotfile_content,
        script_marker,
        _tmp: tmp,
    }
}

/// pull → revert: the box ends functional-clean vanilla.
#[cfg(unix)]
#[test]
fn pull_then_revert_is_functional_clean() {
    let fx = build_fixture();

    let parsed = profile::parse_str(&fx.profile_toml, "vertical-slice").unwrap();
    // 1 dotfile + 1 script.
    assert_eq!(parsed.items.len(), 2, "expected exactly two items");

    // ── PULL ────────────────────────────────────────────────────────────────
    {
        let mut store = SqliteStore::open(&fx.db_path).unwrap();
        let handlers = Handlers::new();
        let mut engine = Engine {
            store: &mut store,
            handlers: &handlers,
            backups_root: fx.backups_root.clone(),
            timeout: Duration::from_secs(10),
            max_undo_attempts: 3,
        };
        engine.pull(&parsed).expect("pull should succeed");
    }

    // The dotfile now exists at its target with the source content.
    assert!(
        fx.dotfile_target.exists(),
        "pull must create the dotfile at its target"
    );
    let got = std::fs::read_to_string(&fx.dotfile_target).unwrap();
    assert_eq!(
        got, fx.dotfile_content,
        "dotfile content must match the source"
    );

    // The script ran: the marker exists.
    assert!(
        fx.script_marker.exists(),
        "pull must run the post_install script (marker file created)"
    );

    // ── REVERT ──────────────────────────────────────────────────────────────
    let op_id;
    {
        let mut store = SqliteStore::open(&fx.db_path).unwrap();
        // Sanity: there is an applied pull to revert.
        op_id = store
            .last_applied_op()
            .unwrap()
            .expect("there must be an applied pull op to revert");

        let handlers = Handlers::new();
        let mut engine = Engine {
            store: &mut store,
            handlers: &handlers,
            backups_root: fx.backups_root.clone(),
            timeout: Duration::from_secs(10),
            max_undo_attempts: 3,
        };
        engine.revert_last().expect("revert should succeed");
    }

    // The dotfile is GONE (before-absent → undo deletes it).
    assert!(
        !fx.dotfile_target.exists(),
        "revert must delete the dotfile we created (before-absent → delete)"
    );

    // The script marker is GONE (undo script ran).
    assert!(
        !fx.script_marker.exists(),
        "revert must run the script's undo (marker file removed)"
    );

    // The op final status is Reverted.
    {
        let store = SqliteStore::open(&fx.db_path).unwrap();
        let statuses = store.action_statuses(op_id).unwrap();
        // Every action of the reverted op must be Reverted (nothing stranded Applied).
        use candylane_core::ActionStatus;
        assert!(
            statuses.iter().all(|s| *s == ActionStatus::Reverted),
            "every action must be Reverted after a clean revert; got {statuses:?}"
        );
    }

    assert_op_reverted(&fx.db_path, op_id);
}

/// CRITICAL #2 through the full engine: a PRE-EXISTING dotfile is overwritten by pull,
/// then `revert` must restore the original bytes EXACTLY (sha256-verified backup path).
///
/// The handler unit test covers backup-then-restore in isolation; this proves the recipe
/// survives the round-trip through `SqliteStore` (serialize → persist → read back → undo)
/// — the part the money test's before-absent path never exercises.
#[cfg(unix)]
#[test]
fn pull_then_revert_restores_preexisting_dotfile() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Target ALREADY EXISTS with original content.
    let target = root.join("existing").join(".gitconfig");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    let original = "[user]\n\tname = before\n";
    std::fs::write(&target, original).unwrap();

    // Source carries new content.
    let src = root.join("src_gitconfig");
    let new_content = "[user]\n\tname = after\n";
    std::fs::write(&src, new_content).unwrap();

    let db_path = root.join("state.db");
    let backups_root = root.join("backups");
    let profile_toml = format!(
        r#"
name = "restore-slice"
version = "0.1"

[dotfiles]
[[dotfiles.file]]
src    = "{src}"
target = "{target}"
"#,
        src = src.display(),
        target = target.display(),
    );
    let parsed = profile::parse_str(&profile_toml, "restore-slice").unwrap();

    // ── PULL: overwrites the existing file ────────────────────────────────────
    {
        let mut store = SqliteStore::open(&db_path).unwrap();
        let handlers = Handlers::new();
        let mut engine = Engine {
            store: &mut store,
            handlers: &handlers,
            backups_root: backups_root.clone(),
            timeout: Duration::from_secs(10),
            max_undo_attempts: 3,
        };
        engine.pull(&parsed).expect("pull should succeed");
    }
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        new_content,
        "pull must overwrite the existing dotfile with the source content"
    );

    // ── REVERT: must restore the ORIGINAL bytes ───────────────────────────────
    let op_id;
    {
        let mut store = SqliteStore::open(&db_path).unwrap();
        op_id = store.last_applied_op().unwrap().expect("an applied pull");
        let handlers = Handlers::new();
        let mut engine = Engine {
            store: &mut store,
            handlers: &handlers,
            backups_root: backups_root.clone(),
            timeout: Duration::from_secs(10),
            max_undo_attempts: 3,
        };
        engine.revert_last().expect("revert should succeed");
    }
    assert!(
        target.exists(),
        "revert must NOT delete a file that pre-existed"
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        original,
        "revert must restore the original bytes exactly (CRITICAL #2 through the store)"
    );

    assert_op_reverted(&db_path, op_id);
}

/// Confirm the operations row reached `Reverted`. Read it directly via a fresh store +
/// a small query helper that re-derives status from action_statuses is not enough — we
/// want the op row itself. We open the DB and check via the public store surface: an op
/// that is Reverted is no longer `last_applied_op` (which only returns 'applied' pulls).
fn assert_op_reverted(db_path: &Path, reverted_op: i64) {
    let store = SqliteStore::open(db_path).unwrap();
    // After a successful revert the op is no longer 'applied', so last_applied_op must
    // not return it.
    let still_applied = store.last_applied_op().unwrap();
    assert_ne!(
        still_applied,
        Some(reverted_op),
        "the reverted op must no longer be reported as the last applied op"
    );
    // And there is no unfinished (pending) op left behind.
    assert_eq!(
        store.unfinished_op().unwrap(),
        None,
        "no operation may be left pending after revert"
    );
    // Belt-and-braces: confirm the op status text is exactly 'reverted'.
    assert_eq!(
        op_status_text(db_path, reverted_op),
        "reverted",
        "operations.status must be 'reverted' after revert_last()"
    );
}

/// Read the raw `operations.status` TEXT for an op id straight from SQLite, bypassing the
/// store trait (which has no op-status getter). Keeps the money test honest about the
/// final persisted status without widening the public API.
fn op_status_text(db_path: &Path, op_id: i64) -> String {
    let conn = rusqlite::Connection::open(db_path).unwrap();
    conn.query_row(
        "SELECT status FROM operations WHERE id = ?1",
        rusqlite::params![op_id],
        |row| row.get::<_, String>(0),
    )
    .unwrap()
}
