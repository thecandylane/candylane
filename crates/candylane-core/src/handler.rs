//! The `Handler` trait every action type implements, plus the `WingetExecutor`
//! subprocess seam that keeps winget logic unit-testable off-Windows (Finding #12).
//!
//! SCAFFOLD: signatures are final; bodies live in the per-handler modules (Lanes B/C/D).

use crate::types::*;
use crate::Result;

/// A unit of work the engine can plan, execute, record, and reverse.
pub trait Handler {
    fn kind(&self) -> HandlerKind;

    /// Pure read of current system state for `target`. Used by diff, recovery,
    /// AND post-apply confirmation. MUST NOT mutate.
    fn probe(&self, target: &Target) -> Result<Probe>;

    /// Decide what to do given desired item + probed state.
    /// `Ok(None)` => already satisfied (idempotency / re-pull no-op).
    fn plan(&self, desired: &Item, probe: &Probe) -> Result<Option<PlannedAction>>;

    /// Execute a planned action, then RE-PROBE to confirm the real effect.
    /// Success is defined by probe state, NOT by a subprocess exit code (Finding #2).
    /// The engine has already written status=pending before this is called.
    fn apply(&self, action: &PlannedAction, ctx: &ApplyCtx) -> Result<Applied>;

    /// Reverse a previously-applied action from its recorded recipe.
    /// MUST be idempotent (re-running on an already-reverted action is a no-op).
    /// For BestEffort actions, verify ownership before destroying state
    /// (don't uninstall a package the user manually upgraded — Finding #8).
    fn undo(&self, action: &RecordedAction, ctx: &ApplyCtx) -> Result<()>;

    // ── LOCKED, pending implementation (do under a compiler — see LANE_A_STATUS.md) ──
    //
    // The reconcile applied-path leaf (CRITICAL #4). After a crash, `engine::reconcile`
    // finds an in-flight action whose probe != before — i.e. apply took effect before the
    // crash. The handler must rebuild the undo recipe from the action's pre-state (`before`)
    // and the post-crash probed state (`probe`) so rollback can reverse it. Returns the same
    // shape as `apply` (after + undo recipe).
    //
    // Signature is decided; uncomment + implement together with the first real handler
    // (Lane B) so a compiler verifies it, and update FakeHandler in
    // tests/engine_transaction.rs + drop the #[should_panic] on the applied-path test.
    // Adding this OBLIGATES every Handler (winget/dotfile/script) to implement it.
    //
    // fn synthesize_undo(&self, target: &Target, before: &Json, probe: &Probe) -> Result<Applied>;
}

/// Raw result of a winget subprocess. Exit code is recorded but is NOT the
/// success signal — the handler re-probes via `list()` to decide truth.
#[derive(Debug, Clone)]
pub struct RawOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Subprocess seam. The real impl shells `winget.exe` (always passing
/// `--accept-source-agreements --accept-package-agreements --silent`, or a fresh
/// VM hangs on the agreement prompt). Tests inject a fake to exercise every branch
/// without a Windows host.
pub trait WingetExecutor: Send + Sync {
    fn install(&self, pkg: &str) -> Result<RawOutput>;
    fn uninstall(&self, pkg: &str) -> Result<RawOutput>;
    fn list(&self, pkg: &str) -> Result<RawOutput>;
}
