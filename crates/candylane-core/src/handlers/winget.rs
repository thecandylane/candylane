//! WingetHandler — Lane B.
//!
//! Installs / uninstalls winget packages as part of a profile pull. The handler shells
//! `winget.exe` through the [`WingetExecutor`] seam (so every branch is unit-testable
//! off-Windows by injecting a fake) and reads truth from `winget list` — **success is
//! defined by the probe, never by a subprocess exit code** (the trait contract, and the
//! reason winget can be `best_effort` honestly).
//!
//! ## Reversibility
//!
//! winget is [`UndoKind::BestEffort`], never `Inverse`. `apply()` records whether the
//! package was **already present before** the pull; `undo()` only uninstalls packages
//! Candylane actually installed (ownership check — never remove something the user had).
//! Registry / PATH / shell crumbs that winget leaves behind are out of scope for undo;
//! `diff`/`history` surface the `best_effort` tag so the residue is never hidden.
//!
//! ## Probe model
//!
//! `probe()` runs `winget list --id <pkg> --exact` and parses the output:
//! - a data row whose Id column equals `<pkg>` ⇒ installed (version captured);
//! - "No installed package found" / no matching row ⇒ not installed.
//!
//! winget's exit code (0 vs 20) is *corroborating* only; the parsed presence of the Id
//! row is the signal, so a winget exit-code quirk can't make us claim a wrong state.
//!
//! ## Recipe shapes
//!
//! Probe:
//! ```json
//! { "installed": true, "version": "1.2.3" }   // or { "installed": false, "version": null }
//! ```
//! After-state (from apply):
//! ```json
//! { "installed": true, "version": "1.2.3" }
//! ```
//! Undo recipe:
//! ```json
//! { "pkg": "Vendor.Pkg", "was_present_before": false, "kind": "best_effort" }
//! ```
//! When `was_present_before` is true the undo is a deliberate no-op: the user already had
//! the package, so reverting our pull must not remove it.

use anyhow::Context;
use serde_json::json;

use crate::handler::{Handler, WingetExecutor};
use crate::types::{
    Applied, ApplyCtx, HandlerKind, Item, Json, PlannedAction, Probe, RecordedAction, Target,
    UndoKind,
};
use crate::Result;

// ── public API ───────────────────────────────────────────────────────────────

/// Installs/uninstalls winget packages. Owns a [`WingetExecutor`] (the subprocess
/// seam) so the handler logic is identical on Windows (real `winget.exe`) and in
/// tests (injected fake).
pub struct WingetHandler {
    exec: Box<dyn WingetExecutor>,
}

impl WingetHandler {
    /// Production constructor: wires the real `winget.exe` executor on Windows, and a
    /// loudly-failing stub off-Windows (Candylane ships for windows-msvc; a non-Windows
    /// build can still construct the handler so `candylane-core` compiles and the
    /// fake-driven unit tests run, but actually invoking winget there is a hard error).
    pub fn new() -> Self {
        WingetHandler {
            exec: default_executor(),
        }
    }

    /// Test/seam constructor: inject any executor (a fake in unit tests).
    pub fn with_executor(exec: Box<dyn WingetExecutor>) -> Self {
        WingetHandler { exec }
    }

    /// Run a probe from a [`RawOutput`]-producing `list` call. Shared by `probe()`
    /// and the apply/undo re-probe so the parse logic lives in exactly one place.
    fn probe_pkg(&self, pkg: &str) -> Result<Probe> {
        let out = self
            .exec
            .list(pkg)
            .with_context(|| format!("winget list failed for {pkg}"))?;
        Ok(Probe(parse_list(&out.stdout, pkg)))
    }
}

impl Default for WingetHandler {
    fn default() -> Self {
        Self::new()
    }
}

// ── Handler impl ─────────────────────────────────────────────────────────────

impl Handler for WingetHandler {
    fn kind(&self) -> HandlerKind {
        HandlerKind::Winget
    }

    /// Pure read: is `target` (a package Id) installed, and at what version?
    fn probe(&self, target: &Target) -> Result<Probe> {
        self.probe_pkg(&target.0)
    }

    /// Plan an install. `Ok(None)` when the package is already installed (idempotent
    /// re-pull). winget actions are always [`UndoKind::BestEffort`].
    fn plan(&self, desired: &Item, probe: &Probe) -> Result<Option<PlannedAction>> {
        let pkg = match desired {
            Item::Winget { pkg } => pkg.clone(),
            _ => anyhow::bail!("WingetHandler.plan called with non-Winget item: {desired:?}"),
        };

        // Already installed → nothing to do. Record nothing; re-pull is a clean no-op.
        if installed(&probe.0) {
            return Ok(None);
        }

        Ok(Some(PlannedAction {
            handler: HandlerKind::Winget,
            target: Target(pkg.clone()),
            before: probe.0.clone(),
            undo_kind: UndoKind::BestEffort,
            // payload carries the package id so apply() never needs the Item again.
            payload: json!({ "pkg": pkg }),
        }))
    }

    /// Install the package, then **re-probe** to confirm. Success is the probe showing
    /// the package installed — NOT winget's exit code (the trait contract). If winget
    /// "succeeds" but the package is still absent, that's a hard error.
    fn apply(&self, action: &PlannedAction, _ctx: &ApplyCtx) -> Result<Applied> {
        let pkg = action
            .payload
            .get("pkg")
            .and_then(|v| v.as_str())
            .context("WingetHandler.apply: payload missing 'pkg' field")?
            .to_owned();

        // What was true before we touched anything (from plan's `before`): did the user
        // already have this package? Drives the undo ownership check.
        let was_present_before = installed(&action.before);

        // Run the install. Exit code is captured but is NOT the success signal.
        let _out = self
            .exec
            .install(&pkg)
            .with_context(|| format!("winget install failed to run for {pkg}"))?;

        // Re-probe: the real, observable truth.
        let after = self.probe_pkg(&pkg)?;

        if !installed(&after.0) {
            anyhow::bail!(
                "winget install of {pkg} did not result in an installed package \
                 (probe after install: {})",
                after.0
            );
        }

        Ok(Applied {
            after: after.0,
            undo: json!({
                "pkg": pkg,
                "was_present_before": was_present_before,
                "kind": "best_effort",
            }),
        })
    }

    /// Best-effort uninstall, with an ownership guard (Finding #8): only remove the
    /// package if Candylane installed it (`was_present_before == false`). Idempotent —
    /// if the package is already gone, that's success.
    fn undo(&self, action: &RecordedAction, _ctx: &ApplyCtx) -> Result<()> {
        let recipe = &action.undo;
        let pkg = recipe
            .get("pkg")
            .and_then(|v| v.as_str())
            .context("WingetHandler.undo: undo recipe missing 'pkg' field")?;

        // Ownership check: the user already had this package before our pull — reverting
        // must NOT uninstall it. Honest no-op.
        let was_present_before = recipe
            .get("was_present_before")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if was_present_before {
            return Ok(());
        }

        // Idempotency: if it's already gone, nothing to do.
        let current = self.probe_pkg(pkg)?;
        if !installed(&current.0) {
            return Ok(());
        }

        // Uninstall. Exit code is not the truth; re-probe to confirm it actually went.
        let _out = self
            .exec
            .uninstall(pkg)
            .with_context(|| format!("winget uninstall failed to run for {pkg}"))?;

        let after = self.probe_pkg(pkg)?;
        if installed(&after.0) {
            anyhow::bail!(
                "winget uninstall of {pkg} did not remove the package \
                 (probe after uninstall: {})",
                after.0
            );
        }
        Ok(())
    }

    /// Crash reconcile (CRITICAL #4). The engine reaches the applied-path branch only
    /// when `probe != before`: the install took effect before the crash. Rebuild the
    /// best-effort undo recipe from the pre-state (`before`) and the post-crash probe.
    ///
    /// The package id is the `target` (a winget Target is always `Target(pkg)`); the
    /// probe shape doesn't carry it. `before` tells us whether the user already had the
    /// package, which drives the undo ownership guard.
    fn synthesize_undo(&self, target: &Target, before: &Json, probe: &Probe) -> Result<Applied> {
        let was_present_before = installed(before);

        Ok(Applied {
            after: probe.0.clone(),
            undo: json!({
                "pkg": target.0,
                "was_present_before": was_present_before,
                "kind": "best_effort",
            }),
        })
    }
}

// ── probe parsing ─────────────────────────────────────────────────────────────

/// True if a probe JSON says the package is installed.
fn installed(probe: &Json) -> bool {
    probe
        .get("installed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Parse `winget list --id <pkg> --exact` stdout into a probe JSON
/// `{ "installed": bool, "version": str|null }`.
///
/// Strategy (deliberately not fixed-column): winget's `--exact` already guarantees the
/// Id match, so we look for a data line whose whitespace-split tokens contain `pkg` as a
/// standalone token (the Id column). The version is the token immediately following the
/// Id. winget decorates output with spinner/progress noise and a header row; both are
/// skipped (header by the literal "Id"/"Version" columns; noise by the absence of the
/// Id token).
fn parse_list(stdout: &str, pkg: &str) -> Json {
    for line in stdout.lines() {
        let line = strip_spinner(line);
        if line.is_empty() {
            continue;
        }
        // Skip the header row (contains the literal column titles, not a real Id).
        // The header has "Id" AND "Version" as tokens; a data row has the package id.
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        // Find the Id token position by exact match against the requested package.
        if let Some(idx) = tokens.iter().position(|t| *t == pkg) {
            // The version is the next token after the Id, when present. Some rows have
            // no version (rare); record null in that case.
            let version = tokens.get(idx + 1).map(|s| s.to_string());
            return json!({ "installed": true, "version": version });
        }
    }
    json!({ "installed": false, "version": null })
}

/// winget prefixes lines with a rotating spinner (`-`, `\`, `|`, `/`) and progress bars
/// (`█`, `▒`) padded with spaces. Strip a leading run of those so the real content is
/// reachable. (The data we care about never starts with those glyphs.)
fn strip_spinner(line: &str) -> &str {
    line.trim_start_matches(|c: char| {
        matches!(c, '-' | '\\' | '|' | '/' | '█' | '▒' | ' ' | '\t' | '\r')
    })
    .trim_end()
}

// ── default executor (real on Windows, loud stub elsewhere) ───────────────────

#[cfg(windows)]
fn default_executor() -> Box<dyn WingetExecutor> {
    Box::new(real::WingetCli::new())
}

#[cfg(not(windows))]
fn default_executor() -> Box<dyn WingetExecutor> {
    Box::new(stub::UnavailableWinget)
}

/// The real `winget.exe` executor. Windows-only.
#[cfg(windows)]
mod real {
    use super::*;
    use crate::handler::RawOutput;
    use std::process::Command;

    /// Shells `winget.exe`. Always passes the agreement-acceptance + `--silent` flags on
    /// install/uninstall — a fresh VM hangs forever on the interactive agreement prompt
    /// otherwise (the reason these flags are non-negotiable, per the trait docs).
    pub struct WingetCli;

    impl WingetCli {
        pub fn new() -> Self {
            WingetCli
        }

        fn run(&self, args: &[&str]) -> Result<RawOutput> {
            let output = Command::new("winget")
                .args(args)
                .output()
                .with_context(|| format!("failed to spawn winget {args:?}"))?;
            Ok(RawOutput {
                code: output.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }

    impl WingetExecutor for WingetCli {
        fn install(&self, pkg: &str) -> Result<RawOutput> {
            self.run(&[
                "install",
                "--id",
                pkg,
                "--exact",
                "--silent",
                "--accept-source-agreements",
                "--accept-package-agreements",
                "--disable-interactivity",
            ])
        }

        fn uninstall(&self, pkg: &str) -> Result<RawOutput> {
            self.run(&[
                "uninstall",
                "--id",
                pkg,
                "--exact",
                "--silent",
                "--accept-source-agreements",
                "--disable-interactivity",
            ])
        }

        fn list(&self, pkg: &str) -> Result<RawOutput> {
            self.run(&[
                "list",
                "--id",
                pkg,
                "--exact",
                "--accept-source-agreements",
                "--disable-interactivity",
            ])
        }
    }
}

/// Off-Windows stub: constructing the handler is fine (so `candylane-core` compiles and
/// fake-driven tests run), but actually shelling winget on a non-Windows host is a hard
/// error rather than a silent wrong answer.
#[cfg(not(windows))]
mod stub {
    use super::*;
    use crate::handler::RawOutput;

    pub struct UnavailableWinget;

    impl WingetExecutor for UnavailableWinget {
        fn install(&self, _pkg: &str) -> Result<RawOutput> {
            anyhow::bail!("winget is only available on Windows")
        }
        fn uninstall(&self, _pkg: &str) -> Result<RawOutput> {
            anyhow::bail!("winget is only available on Windows")
        }
        fn list(&self, _pkg: &str) -> Result<RawOutput> {
            anyhow::bail!("winget is only available on Windows")
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::RawOutput;
    use crate::types::ActionStatus;
    use std::sync::Mutex;

    // ── a scriptable fake executor ──────────────────────────────────────────────

    /// Returns canned outputs. `list` consumes a queued sequence front-to-back (the last
    /// entry repeats), so a test can model "absent → install → present". An empty queue
    /// makes any `list` call panic — used to prove a code path never reaches `list`.
    struct FakeWinget {
        list_outputs: Mutex<Vec<RawOutput>>,
        // std Result (2 args) — the crate's `Result` alias is 1-arg (anyhow).
        install_result: Mutex<std::result::Result<RawOutput, String>>,
        uninstall_result: Mutex<std::result::Result<RawOutput, String>>,
    }

    impl FakeWinget {
        fn new() -> Self {
            FakeWinget {
                list_outputs: Mutex::new(Vec::new()),
                install_result: Mutex::new(Ok(raw(0, "", ""))),
                uninstall_result: Mutex::new(Ok(raw(0, "", ""))),
            }
        }

        /// Queue list outputs, consumed front-to-back across successive `list` calls.
        /// The last one repeats once the queue is down to a single entry.
        fn with_list_sequence(self, outs: Vec<RawOutput>) -> Self {
            *self.list_outputs.lock().unwrap() = outs;
            self
        }
    }

    impl WingetExecutor for FakeWinget {
        fn install(&self, _pkg: &str) -> Result<RawOutput> {
            match &*self.install_result.lock().unwrap() {
                Ok(o) => Ok(o.clone()),
                Err(e) => anyhow::bail!("{e}"),
            }
        }
        fn uninstall(&self, _pkg: &str) -> Result<RawOutput> {
            match &*self.uninstall_result.lock().unwrap() {
                Ok(o) => Ok(o.clone()),
                Err(e) => anyhow::bail!("{e}"),
            }
        }
        fn list(&self, _pkg: &str) -> Result<RawOutput> {
            let mut q = self.list_outputs.lock().unwrap();
            // Empty queue → index panic, by design: it proves a path that must not call
            // list() (e.g. the ownership-guard short-circuit) never reached here.
            let out = if q.len() > 1 {
                q.remove(0)
            } else {
                q[0].clone()
            };
            Ok(out)
        }
    }

    fn raw(code: i32, stdout: &str, stderr: &str) -> RawOutput {
        RawOutput {
            code,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    // Real winget output fixtures (captured from a live machine).
    const LIST_INSTALLED: &str = "   -    \\   \nName                                 Id              Version Source\nRustup: the Rust toolchain installer Rustlang.Rustup 1.29.0  winget";
    const LIST_ABSENT: &str = "   -    \\   \nNo installed package found matching input criteria.";
    // A row whose Name has spaces AND a version-like token, plus an "Available" column
    // (upgrade pending). Captured live: `winget list --id 7zip.7zip --exact`.
    const LIST_NAME_WITH_SPACES: &str = "   -    \\   \nName              Id        Version Available Source\n7-Zip 25.01 (x64) 7zip.7zip 25.01   26.01     winget";

    fn ctx_dir() -> std::path::PathBuf {
        std::env::temp_dir()
    }

    fn apply_ctx(dir: &std::path::Path) -> ApplyCtx<'_> {
        ApplyCtx {
            backups_dir: dir,
            timeout: std::time::Duration::from_secs(5),
            dry_run: false,
            max_undo_attempts: 3,
        }
    }

    // ── parse_list unit tests ───────────────────────────────────────────────────

    #[test]
    fn parse_installed_extracts_version() {
        let p = parse_list(LIST_INSTALLED, "Rustlang.Rustup");
        assert_eq!(p["installed"], true);
        assert_eq!(p["version"], "1.29.0");
    }

    #[test]
    fn parse_absent_is_not_installed() {
        let p = parse_list(LIST_ABSENT, "Rustlang.Rustup");
        assert_eq!(p["installed"], false);
        assert!(p["version"].is_null());
    }

    #[test]
    fn parse_does_not_false_match_on_substring() {
        // A different package whose id is a substring prefix must NOT match.
        let p = parse_list(LIST_INSTALLED, "Rustlang");
        assert_eq!(
            p["installed"], false,
            "substring of an Id must not count as installed"
        );
    }

    /// Regression: a Name column with spaces and a version-like token, plus an extra
    /// "Available" column, must still extract the correct Version (the token right after
    /// the Id), not a token from the Name. The `installed` boolean is what gates revert;
    /// version is informational — but pin it so the parser doesn't silently drift.
    #[test]
    fn parse_name_with_spaces_extracts_correct_version() {
        let p = parse_list(LIST_NAME_WITH_SPACES, "7zip.7zip");
        assert_eq!(p["installed"], true);
        assert_eq!(
            p["version"], "25.01",
            "version must be the token after the Id, not from the Name; got {p}"
        );
    }

    #[test]
    fn parse_ignores_header_row() {
        // A header row with no matching data row → not installed. A real winget package
        // id (Vendor.Name form) never collides with the literal "Id"/"Version" headers.
        let header_only = "Name  Id  Version  Source";
        assert_eq!(parse_list(header_only, "Vendor.Real")["installed"], false);
    }

    // ── probe ───────────────────────────────────────────────────────────────────

    #[test]
    fn probe_installed() {
        let fake = FakeWinget::new().with_list_sequence(vec![raw(0, LIST_INSTALLED, "")]);
        let h = WingetHandler::with_executor(Box::new(fake));
        let p = h.probe(&Target("Rustlang.Rustup".into())).unwrap();
        assert_eq!(p.0["installed"], true);
        assert_eq!(p.0["version"], "1.29.0");
    }

    #[test]
    fn probe_absent() {
        let fake = FakeWinget::new().with_list_sequence(vec![raw(20, LIST_ABSENT, "")]);
        let h = WingetHandler::with_executor(Box::new(fake));
        let p = h.probe(&Target("Vendor.Absent".into())).unwrap();
        assert_eq!(p.0["installed"], false);
    }

    // ── plan ──────────────────────────────────────────────────────────────────

    #[test]
    fn plan_installs_when_absent() {
        let fake = FakeWinget::new();
        let h = WingetHandler::with_executor(Box::new(fake));
        let probe = Probe(json!({ "installed": false, "version": null }));
        let item = Item::Winget {
            pkg: "Vendor.Pkg".into(),
        };
        let planned = h.plan(&item, &probe).unwrap().unwrap();
        assert_eq!(planned.handler, HandlerKind::Winget);
        assert_eq!(planned.undo_kind, UndoKind::BestEffort);
        assert_eq!(planned.payload["pkg"], "Vendor.Pkg");
    }

    #[test]
    fn plan_noop_when_already_installed() {
        let fake = FakeWinget::new();
        let h = WingetHandler::with_executor(Box::new(fake));
        let probe = Probe(json!({ "installed": true, "version": "1.0.0" }));
        let item = Item::Winget {
            pkg: "Vendor.Pkg".into(),
        };
        let planned = h.plan(&item, &probe).unwrap();
        assert!(
            planned.is_none(),
            "an already-installed package must plan to None (idempotent re-pull)"
        );
    }

    #[test]
    fn plan_rejects_non_winget_item() {
        let h = WingetHandler::with_executor(Box::new(FakeWinget::new()));
        let probe = Probe(json!(null));
        let item = Item::Script {
            run: "x".into(),
            undo: None,
        };
        assert!(h.plan(&item, &probe).is_err());
    }

    // ── apply ─────────────────────────────────────────────────────────────────

    /// apply(): install then re-probe shows installed → Ok, after-state + undo recipe.
    #[test]
    fn apply_installs_and_confirms_by_reprobe() {
        // list called once inside apply (the re-probe) → return "installed".
        let fake = FakeWinget::new().with_list_sequence(vec![raw(0, LIST_INSTALLED, "")]);
        let h = WingetHandler::with_executor(Box::new(fake));

        let action = PlannedAction {
            handler: HandlerKind::Winget,
            target: Target("Rustlang.Rustup".into()),
            before: json!({ "installed": false, "version": null }),
            undo_kind: UndoKind::BestEffort,
            payload: json!({ "pkg": "Rustlang.Rustup" }),
        };
        let dir = ctx_dir();
        let applied = h.apply(&action, &apply_ctx(&dir)).unwrap();
        assert_eq!(applied.after["installed"], true);
        assert_eq!(applied.undo["pkg"], "Rustlang.Rustup");
        assert_eq!(applied.undo["was_present_before"], false);
        assert_eq!(applied.undo["kind"], "best_effort");
    }

    /// CRITICAL: a winget that "succeeds" but leaves the package absent is a hard error
    /// (success is read from the probe, not the exit code).
    #[test]
    fn apply_errors_when_reprobe_shows_absent() {
        // install "succeeds" (exit 0) but the re-probe still shows absent.
        let fake = FakeWinget::new().with_list_sequence(vec![raw(0, LIST_ABSENT, "")]);
        let h = WingetHandler::with_executor(Box::new(fake));

        let action = PlannedAction {
            handler: HandlerKind::Winget,
            target: Target("Vendor.Ghost".into()),
            before: json!({ "installed": false, "version": null }),
            undo_kind: UndoKind::BestEffort,
            payload: json!({ "pkg": "Vendor.Ghost" }),
        };
        let dir = ctx_dir();
        let err = h.apply(&action, &apply_ctx(&dir)).unwrap_err().to_string();
        assert!(
            err.contains("did not result in an installed package"),
            "must fail from the probe, not the exit code; got: {err}"
        );
    }

    /// apply() records was_present_before=true when the user already had the package
    /// (drives the undo ownership guard).
    #[test]
    fn apply_records_preexisting_ownership() {
        let fake = FakeWinget::new().with_list_sequence(vec![raw(0, LIST_INSTALLED, "")]);
        let h = WingetHandler::with_executor(Box::new(fake));

        let action = PlannedAction {
            handler: HandlerKind::Winget,
            target: Target("Rustlang.Rustup".into()),
            before: json!({ "installed": true, "version": "1.29.0" }),
            undo_kind: UndoKind::BestEffort,
            payload: json!({ "pkg": "Rustlang.Rustup" }),
        };
        let dir = ctx_dir();
        let applied = h.apply(&action, &apply_ctx(&dir)).unwrap();
        assert_eq!(applied.undo["was_present_before"], true);
    }

    // ── undo ──────────────────────────────────────────────────────────────────

    /// undo() uninstalls a package Candylane installed, confirmed by re-probe.
    #[test]
    fn undo_uninstalls_when_we_installed_it() {
        // sequence: probe(installed) [idempotency check] → probe(absent) [confirm].
        let fake = FakeWinget::new()
            .with_list_sequence(vec![raw(0, LIST_INSTALLED, ""), raw(20, LIST_ABSENT, "")]);
        let h = WingetHandler::with_executor(Box::new(fake));

        let recorded = recorded(json!({
            "pkg": "Rustlang.Rustup",
            "was_present_before": false,
            "kind": "best_effort"
        }));
        let dir = ctx_dir();
        h.undo(&recorded, &apply_ctx(&dir)).unwrap();
    }

    /// Ownership guard (Finding #8): undo must NOT uninstall a package the user already
    /// had (was_present_before=true). Provable with an empty-queue fake: if `undo` ever
    /// reached the idempotency `list` or the `uninstall`, the fake's `list` would panic
    /// on an empty queue. It returning Ok therefore proves no winget call was made.
    #[test]
    fn undo_skips_preexisting_package_without_touching_winget() {
        let fake = FakeWinget::new(); // empty list queue — any list() call panics
        let h = WingetHandler::with_executor(Box::new(fake));

        let recorded = recorded(json!({
            "pkg": "Rustlang.Rustup",
            "was_present_before": true,
            "kind": "best_effort"
        }));
        let dir = ctx_dir();
        // Must be Ok and must not panic — proving the guard short-circuited before list().
        h.undo(&recorded, &apply_ctx(&dir)).unwrap();
    }

    /// undo() is idempotent: an already-absent package is a clean no-op (the idempotency
    /// re-probe shows absent, so uninstall is never called).
    #[test]
    fn undo_idempotent_when_already_gone() {
        // single list → absent; uninstall must not be reached.
        let fake = FakeWinget::new().with_list_sequence(vec![raw(20, LIST_ABSENT, "")]);
        let h = WingetHandler::with_executor(Box::new(fake));
        let recorded = recorded(json!({
            "pkg": "Vendor.Pkg",
            "was_present_before": false,
            "kind": "best_effort"
        }));
        let dir = ctx_dir();
        h.undo(&recorded, &apply_ctx(&dir)).unwrap();
    }

    // ── synthesize_undo ─────────────────────────────────────────────────────────

    /// Crash reconcile: before=absent, post-crash probe=installed → rebuild a
    /// best_effort uninstall recipe with was_present_before=false, pkg from the target.
    #[test]
    fn synthesize_undo_rebuilds_best_effort_recipe() {
        let h = WingetHandler::with_executor(Box::new(FakeWinget::new()));
        let before = json!({ "installed": false, "version": null });
        let probe = Probe(parse_list(LIST_INSTALLED, "Rustlang.Rustup"));
        let applied = h
            .synthesize_undo(&Target("Rustlang.Rustup".into()), &before, &probe)
            .unwrap();
        assert_eq!(applied.after["installed"], true);
        assert_eq!(applied.undo["pkg"], "Rustlang.Rustup");
        assert_eq!(applied.undo["was_present_before"], false);
        assert_eq!(applied.undo["kind"], "best_effort");
    }

    /// Reconcile honesty: if the user already had the package before the crash
    /// (before=installed), the rebuilt recipe must mark was_present_before=true so the
    /// later undo won't uninstall a package Candylane didn't install.
    #[test]
    fn synthesize_undo_preserves_preexisting_ownership() {
        let h = WingetHandler::with_executor(Box::new(FakeWinget::new()));
        let before = json!({ "installed": true, "version": "1.0.0" });
        let probe = Probe(parse_list(LIST_INSTALLED, "Rustlang.Rustup"));
        let applied = h
            .synthesize_undo(&Target("Rustlang.Rustup".into()), &before, &probe)
            .unwrap();
        assert_eq!(applied.undo["was_present_before"], true);
    }

    fn recorded(undo: Json) -> RecordedAction {
        RecordedAction {
            id: 1,
            op_id: 1,
            seq: 0,
            handler: HandlerKind::Winget,
            target: Target("Rustlang.Rustup".into()),
            status: ActionStatus::Applied,
            before: json!({ "installed": false, "version": null }),
            after: Some(json!({ "installed": true, "version": "1.29.0" })),
            undo_kind: UndoKind::BestEffort,
            undo,
            undo_attempts: 0,
            undo_error: None,
        }
    }
}
