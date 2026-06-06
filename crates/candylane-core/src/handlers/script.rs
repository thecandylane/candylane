//! ScriptHandler — Lane D.
//!
//! Runs arbitrary shell commands as part of a profile pull. Scripts have no
//! natural persistent state: `probe()` always returns `null` and `plan()` always
//! returns `Some` (scripts are not idempotent by probe).
//!
//! CRITICAL #1 lives here: `apply()` kills the child process GROUP when
//! `ctx.timeout` elapses. A runaway post-install script must NEVER hang the engine.
//!
//! Timeout mechanism: a waiter thread sends on an `mpsc` channel after spawning the
//! child; the main thread uses `recv_timeout(ctx.timeout)`. On timeout it kills the
//! negative PID (the process group) via `libc::kill(-pgid, libc::SIGKILL)`, then
//! waits for the child to reap. This is a **group-kill**: it hits every process the
//! script spawned, not just the direct child.
//!
//! On non-Unix targets (compile-time only; runtime is always Linux/Windows):
//!   - the `setsid` / group-kill path is compiled out via `#[cfg(unix)]`.
//!   - a Windows stub bails with an informative message rather than silently ignoring
//!     the CRITICAL.
//!
//! Undo recipe shape:
//! ```json
//! { "undo_script": "<shell command>" | null, "kind": "inverse" | "one_way" }
//! ```
//!
//! After-state shape:
//! ```json
//! { "ran": true, "status": <exit_code: i32> | "signal" }
//! ```

use crate::handler::Handler;
use crate::types::*;
use crate::Result;
use anyhow::{bail, Context};
use serde_json::json;

// ── public API ───────────────────────────────────────────────────────────────

pub struct ScriptHandler;

impl ScriptHandler {
    pub fn new() -> Self {
        ScriptHandler
    }
}

impl Default for ScriptHandler {
    fn default() -> Self {
        ScriptHandler::new()
    }
}

// ── Handler impl ─────────────────────────────────────────────────────────────

impl Handler for ScriptHandler {
    fn kind(&self) -> HandlerKind {
        HandlerKind::Script
    }

    /// Scripts have no persistent observable state. Returns `Probe(null)`.
    fn probe(&self, _target: &Target) -> Result<Probe> {
        Ok(Probe(json!(null)))
    }

    /// Scripts are ALWAYS planned (not idempotent by probe).
    ///
    /// Sets `undo_kind`:
    /// - `Inverse` when an undo script is provided.
    /// - `OneWay`  when no undo script exists; `diff` and `history` will surface this.
    fn plan(&self, desired: &Item, _probe: &Probe) -> Result<Option<PlannedAction>> {
        let (run, undo) = match desired {
            Item::Script { run, undo } => (run.clone(), undo.clone()),
            _ => bail!(
                "ScriptHandler.plan called with non-Script item: {:?}",
                desired
            ),
        };

        // An empty/whitespace `undo` is not a real undo script. Treating it as Inverse
        // would falsely advertise reversibility (the undo would silently no-op at
        // rollback). Normalize it away so the action is honestly OneWay.
        let undo = undo.filter(|s| !s.trim().is_empty());

        let undo_kind = if undo.is_some() {
            UndoKind::Inverse
        } else {
            UndoKind::OneWay
        };

        Ok(Some(PlannedAction {
            handler: HandlerKind::Script,
            target: Target(run.clone()),
            before: json!(null),
            undo_kind,
            // payload carries both the run command and the optional undo command so
            // apply() / undo() never need the original Item again.
            payload: json!({ "run": run, "undo": undo }),
        }))
    }

    /// Run the command, enforcing `ctx.timeout` with a group-kill on Unix.
    ///
    /// Success model: ANY normal exit is `Ok`. Only a timeout-kill is a hard
    /// `Err`. The exit code is recorded in the after-state.
    ///
    /// The undo recipe carries what `undo()` needs; it is self-contained.
    fn apply(&self, action: &PlannedAction, ctx: &ApplyCtx) -> Result<Applied> {
        let run = action
            .payload
            .get("run")
            .and_then(|v| v.as_str())
            .context("ScriptHandler.apply: payload missing 'run' field")?
            .to_owned();

        let undo_script: Option<String> = action
            .payload
            .get("undo")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned());

        let status_json = run_script(&run, ctx.timeout)?;

        // No re-probe: scripts have no observable persistent state (probe() is always
        // null), so there is nothing to confirm by reading back. The honest after-state
        // is "the script ran; here is its exit status".

        let kind_str = match action.undo_kind {
            UndoKind::Inverse => "inverse",
            _ => "one_way",
        };

        Ok(Applied {
            after: json!({ "ran": true, "status": status_json }),
            undo: json!({ "undo_script": undo_script, "kind": kind_str }),
        })
    }

    /// Reverse the script action using the recorded undo recipe.
    ///
    /// Idempotent:
    /// - `kind == "one_way"` or `undo_script` is `null` → `Ok(())` (nothing to do).
    /// - Otherwise run the undo script with the same timeout + group-kill guarantee.
    fn undo(&self, action: &RecordedAction, ctx: &ApplyCtx) -> Result<()> {
        let undo_recipe = &action.undo;

        let kind = undo_recipe
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("one_way");

        if kind == "one_way" {
            // Honesty: no undo possible. undo_kind=OneWay is recorded in the DB;
            // history/diff surface this. Return Ok so rollback continues cleanly.
            return Ok(());
        }

        let undo_script = undo_recipe.get("undo_script").and_then(|v| v.as_str());

        match undo_script {
            None | Some("") => {
                // Null undo script — treat as one-way even if kind said "inverse".
                Ok(())
            }
            Some(script) => {
                run_script(script, ctx.timeout)?;
                Ok(())
            }
        }
    }

    /// Scripts carry no probe state. Crash-reconcile cannot reconstruct the undo
    /// recipe from a `null` probe, so we refuse honestly.
    ///
    /// Engine `reconcile()` will only reach the applied-path branch when
    /// `real != before`. For scripts both values are always `null`, so `real ==
    /// before` always, and this method is never reached in practice. The bail
    /// documents the invariant so future callers get an informative error rather
    /// than a silent wrong answer.
    fn synthesize_undo(&self, _target: &Target, _before: &Json, _probe: &Probe) -> Result<Applied> {
        bail!(
            "script actions are not reconcilable after a crash (no probe state); \
             treat as one-way"
        )
    }
}

// ── internal: script runner with timeout + group-kill ────────────────────────

/// Run `script` as a shell command, killing the process group after `timeout`.
///
/// Returns a JSON value representing the exit status (`i32` or `"signal"`).
/// The ONLY error path is a timeout (the child exceeded its budget and was killed).
/// Any normal exit code — including non-zero — is treated as success at the
/// apply() / undo() level; the exit code is returned for the caller to record.
fn run_script(script: &str, timeout: std::time::Duration) -> Result<serde_json::Value> {
    #[cfg(unix)]
    {
        run_script_unix(script, timeout)
    }

    #[cfg(not(unix))]
    {
        // Non-Unix stub. This code path should never be reached in production
        // (Candylane targets x86_64-pc-windows-msvc, which uses the real winget
        // pathway; the script handler is intended for WSL/Linux). If it ever is,
        // fail loudly rather than silently skip the timeout CRITICAL.
        let _ = (script, timeout);
        bail!(
            "ScriptHandler: non-Unix timeout + group-kill is not implemented; \
             cannot run script safely without CRITICAL #1 guarantee"
        )
    }
}

#[cfg(unix)]
fn run_script_unix(script: &str, timeout: std::time::Duration) -> Result<serde_json::Value> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    // Spawn the child in its own session (setsid) so it becomes the process
    // group leader. On timeout we kill the negative PID (-pgid) which sends
    // SIGKILL to every process in that group — not just the direct child.
    //
    // SAFETY: `setsid()` is async-signal-safe and has no documented failure mode
    // that would leave the process in an inconsistent state. The only error case
    // (EPERM: already a process group leader) cannot occur for a freshly forked
    // child that has not yet exec'd.
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // SAFETY: pre_exec runs in the child after fork() but before exec().
    // setsid() is listed in the POSIX async-signal-safe set.
    unsafe {
        cmd.pre_exec(|| {
            // Create a new session; this child is now its own process group leader.
            libc::setsid();
            Ok(())
        });
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn script: {}", script))?;

    // Capture the child PID before moving child into the waiter thread.
    let child_pid = child.id() as libc::pid_t;

    // Waiter thread: block on wait() and signal completion via the channel.
    let (tx, rx) = mpsc::channel::<std::io::Result<std::process::ExitStatus>>();
    std::thread::spawn(move || {
        let result = child.wait();
        // Ignore send errors — the main thread may have already timed out and
        // moved on; the child's exit status is irrelevant at that point.
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(wait_result) => {
            let exit_status =
                wait_result.with_context(|| format!("wait() failed for script: {}", script))?;

            let status_json = if let Some(code) = exit_status.code() {
                json!(code)
            } else {
                // Killed by a signal (e.g. SIGSEGV in the script itself).
                json!("signal")
            };

            Ok(status_json)
        }
        Err(_recv_timeout_err) => {
            // Timeout elapsed. Kill the entire process group.
            // child_pid == pgid because we called setsid() in the child, making
            // it the process group leader. Sending SIGKILL to -pgid kills every
            // process in the group.
            //
            // SAFETY: child_pid is a valid PID we just spawned; the group was
            // created by our setsid() call and is not shared with the engine process.
            unsafe {
                libc::kill(-child_pid, libc::SIGKILL);
            }

            // Reap the child to avoid a zombie. Ignore the exit status — it will
            // be SIGKILL. Ignore errors from the waiter send.
            let _ = rx.recv(); // waiter thread will finish shortly after SIGKILL

            bail!(
                "script {:?} exceeded timeout {:?} — killed (process group SIGKILL)",
                script,
                timeout
            )
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn make_ctx(_timeout: Duration) -> std::path::PathBuf {
        // We return the backups_dir PathBuf; callers build ApplyCtx inline so the
        // lifetime is correct.
        std::env::temp_dir()
    }

    fn apply_ctx(backups_dir: &std::path::Path, timeout: Duration) -> ApplyCtx<'_> {
        ApplyCtx {
            backups_dir,
            timeout,
            dry_run: false,
            max_undo_attempts: 3,
        }
    }

    // ── plan tests ────────────────────────────────────────────────────────────

    /// plan() always returns Some (scripts are never pre-satisfied by probe).
    #[test]
    fn plan_always_returns_some() {
        let h = ScriptHandler::new();
        let probe = h.probe(&Target("echo hi".into())).unwrap();
        let item = Item::Script {
            run: "echo hi".into(),
            undo: None,
        };
        let result = h.plan(&item, &probe).unwrap();
        assert!(
            result.is_some(),
            "plan() must always return Some for Script items"
        );
    }

    /// plan() sets undo_kind = Inverse when an undo script is present.
    #[test]
    fn plan_undo_kind_inverse_when_undo_present() {
        let h = ScriptHandler::new();
        let probe = h.probe(&Target("run.sh".into())).unwrap();
        let item = Item::Script {
            run: "run.sh".into(),
            undo: Some("undo.sh".into()),
        };
        let planned = h.plan(&item, &probe).unwrap().unwrap();
        assert_eq!(planned.undo_kind, UndoKind::Inverse);
        assert_eq!(planned.payload["run"], "run.sh");
        assert_eq!(planned.payload["undo"], "undo.sh");
    }

    /// plan() sets undo_kind = OneWay when no undo script is provided.
    #[test]
    fn plan_undo_kind_one_way_when_no_undo() {
        let h = ScriptHandler::new();
        let probe = h.probe(&Target("run.sh".into())).unwrap();
        let item = Item::Script {
            run: "run.sh".into(),
            undo: None,
        };
        let planned = h.plan(&item, &probe).unwrap().unwrap();
        assert_eq!(planned.undo_kind, UndoKind::OneWay);
        assert!(
            planned.payload["undo"].is_null(),
            "payload['undo'] must be null when no undo script"
        );
    }

    /// plan() treats an empty/whitespace undo string as OneWay, not Inverse — an empty
    /// undo must not falsely advertise reversibility (regression guard for the
    /// "undo = \"\"" lie).
    #[test]
    fn plan_empty_undo_string_is_one_way() {
        let h = ScriptHandler::new();
        let probe = h.probe(&Target("run.sh".into())).unwrap();
        for blank in ["", "   ", "\t\n"] {
            let item = Item::Script {
                run: "run.sh".into(),
                undo: Some(blank.into()),
            };
            let planned = h.plan(&item, &probe).unwrap().unwrap();
            assert_eq!(
                planned.undo_kind,
                UndoKind::OneWay,
                "empty undo {blank:?} must be OneWay, not a false Inverse"
            );
            assert!(
                planned.payload["undo"].is_null(),
                "empty undo {blank:?} must normalize to null in payload"
            );
        }
    }

    /// plan() bails on non-Script items.
    #[test]
    fn plan_rejects_non_script_item() {
        let h = ScriptHandler::new();
        let probe = Probe(json!(null));
        let item = Item::Winget {
            pkg: "pkg-a".into(),
        };
        let result = h.plan(&item, &probe);
        assert!(result.is_err(), "plan() must bail on non-Script item");
    }

    // ── apply tests ───────────────────────────────────────────────────────────

    /// apply() succeeds for a script that exits 0.
    #[cfg(unix)]
    #[test]
    fn apply_exit_zero_is_ok() {
        let h = ScriptHandler::new();
        let backups = make_ctx(Duration::from_secs(5));
        let ctx = apply_ctx(&backups, Duration::from_secs(5));

        let item = Item::Script {
            run: "exit 0".into(),
            undo: None,
        };
        let probe = h.probe(&Target("exit 0".into())).unwrap();
        let planned = h.plan(&item, &probe).unwrap().unwrap();

        let applied = h.apply(&planned, &ctx).unwrap();
        assert_eq!(applied.after["ran"], true);
        assert_eq!(applied.after["status"], 0);
    }

    /// apply() also succeeds for a non-zero exit (exit code is NOT the success gate).
    #[cfg(unix)]
    #[test]
    fn apply_non_zero_exit_is_still_ok() {
        let h = ScriptHandler::new();
        let backups = make_ctx(Duration::from_secs(5));
        let ctx = apply_ctx(&backups, Duration::from_secs(5));

        let item = Item::Script {
            run: "exit 42".into(),
            undo: None,
        };
        let probe = h.probe(&Target("exit 42".into())).unwrap();
        let planned = h.plan(&item, &probe).unwrap().unwrap();

        let applied = h.apply(&planned, &ctx).unwrap();
        assert_eq!(applied.after["ran"], true);
        assert_eq!(applied.after["status"], 42);
    }

    /// CRITICAL #1: apply() kills the child and returns Err PROMPTLY on timeout.
    ///
    /// `sleep 5` with a 200 ms timeout must return an Err and the elapsed wall-clock
    /// time must be well under 5 s (< 2 s covers any CI noise while proving the kill
    /// actually fired).
    #[cfg(unix)]
    #[test]
    fn apply_timeout_kills_child_promptly() {
        let h = ScriptHandler::new();
        let backups = make_ctx(Duration::from_millis(200));
        let timeout = Duration::from_millis(200);
        let ctx = apply_ctx(&backups, timeout);

        let item = Item::Script {
            run: "sleep 5".into(),
            undo: None,
        };
        let probe = h.probe(&Target("sleep 5".into())).unwrap();
        let planned = h.plan(&item, &probe).unwrap().unwrap();

        let t0 = Instant::now();
        let result = h.apply(&planned, &ctx);
        let elapsed = t0.elapsed();

        assert!(
            result.is_err(),
            "apply() must return Err when the script times out"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("exceeded timeout"),
            "error message must mention 'exceeded timeout'; got: {}",
            msg
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "apply() must return promptly after kill; elapsed = {:?}",
            elapsed
        );
    }

    // ── undo tests ────────────────────────────────────────────────────────────

    /// undo() with kind="one_way" is a no-op Ok (nothing to reverse).
    #[test]
    fn undo_one_way_is_noop() {
        let h = ScriptHandler::new();
        let backups = make_ctx(Duration::from_secs(5));
        let ctx = apply_ctx(&backups, Duration::from_secs(5));

        let recorded = fake_recorded_action(
            UndoKind::OneWay,
            json!({ "undo_script": null, "kind": "one_way" }),
        );

        let result = h.undo(&recorded, &ctx);
        assert!(result.is_ok(), "undo() on a one_way action must be Ok");
    }

    /// undo() runs the undo script when present.
    ///
    /// The run script writes a marker file; the undo script removes it.
    /// After apply() the marker exists; after undo() it is gone.
    #[cfg(unix)]
    #[test]
    fn undo_runs_undo_script() {
        let h = ScriptHandler::new();
        let backups = make_ctx(Duration::from_secs(5));
        let timeout = Duration::from_secs(5);

        // Use a temp file as the marker.
        let tmp = tempfile_path();
        let run_cmd = format!("touch {}", tmp.display());
        let undo_cmd = format!("rm -f {}", tmp.display());

        // Ensure marker does not pre-exist.
        let _ = std::fs::remove_file(&tmp);
        assert!(!tmp.exists(), "marker must not exist before apply");

        // Apply: creates the marker.
        let ctx = apply_ctx(&backups, timeout);
        let item = Item::Script {
            run: run_cmd.clone(),
            undo: Some(undo_cmd.clone()),
        };
        let probe = h.probe(&Target(run_cmd.clone())).unwrap();
        let planned = h.plan(&item, &probe).unwrap().unwrap();
        let applied = h.apply(&planned, &ctx).unwrap();

        assert!(tmp.exists(), "marker must exist after apply");

        // Undo: removes the marker.
        let recorded = fake_recorded_action(UndoKind::Inverse, applied.undo.clone());
        let ctx2 = apply_ctx(&backups, timeout);
        h.undo(&recorded, &ctx2).unwrap();

        assert!(!tmp.exists(), "marker must be removed after undo");
    }

    /// undo() is idempotent: calling it twice on an already-reverted action returns Ok.
    #[cfg(unix)]
    #[test]
    fn undo_idempotent() {
        let h = ScriptHandler::new();
        let backups = make_ctx(Duration::from_secs(5));
        let timeout = Duration::from_secs(5);

        let tmp = tempfile_path();
        let undo_cmd = format!("rm -f {}", tmp.display()); // rm -f is safe even if absent

        // The file does not exist; rm -f on a missing file returns 0 → Ok.
        let recorded = fake_recorded_action(
            UndoKind::Inverse,
            json!({ "undo_script": undo_cmd, "kind": "inverse" }),
        );

        let ctx = apply_ctx(&backups, timeout);
        h.undo(&recorded, &ctx).unwrap(); // first call
        let ctx2 = apply_ctx(&backups, timeout);
        h.undo(&recorded, &ctx2).unwrap(); // second call — must also be Ok
    }

    // ── synthesize_undo test ──────────────────────────────────────────────────

    /// synthesize_undo always bails (scripts have no probe state to reconstruct from).
    #[test]
    fn synthesize_undo_bails() {
        let h = ScriptHandler::new();
        let target = Target("run.sh".into());
        let before = json!(null);
        let probe = Probe(json!(null));
        let result = h.synthesize_undo(&target, &before, &probe);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("not reconcilable"),
            "error must mention 'not reconcilable'; got: {}",
            msg
        );
    }

    // ── probe test ────────────────────────────────────────────────────────────

    /// probe() always returns null (no persistent state for scripts).
    #[test]
    fn probe_returns_null() {
        let h = ScriptHandler::new();
        let p = h.probe(&Target("anything".into())).unwrap();
        assert_eq!(p.0, json!(null));
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    fn fake_recorded_action(undo_kind: UndoKind, undo: Json) -> RecordedAction {
        RecordedAction {
            id: 1,
            op_id: 1,
            seq: 0,
            handler: HandlerKind::Script,
            target: Target("test".into()),
            status: ActionStatus::Applied,
            before: json!(null),
            after: Some(json!({ "ran": true, "status": 0 })),
            undo_kind,
            undo,
            undo_attempts: 0,
            undo_error: None,
        }
    }

    /// Returns a unique path in the system temp directory for use as a marker file.
    /// Does NOT create the file; callers do that as part of the test.
    fn tempfile_path() -> std::path::PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        let pid = std::process::id();
        std::env::temp_dir().join(format!("candylane-script-test-{}-{}", pid, nanos))
    }
}
