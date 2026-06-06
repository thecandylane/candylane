//! WingetHandler — Lane B (Windows-only).
//!
//! STUB. The real implementation shells `winget.exe` through the [`WingetExecutor`]
//! seam and re-probes via `winget list` (success is read from the probe, never from the
//! subprocess exit code). None of that exists yet — every method is `todo!()`.
//!
//! This stub exists only so the `Handler` trait is satisfied for `HandlerKind::Winget`
//! and the registry can hold a concrete winget handler. On the Linux vertical slice no
//! profile contains winget items, so none of these methods is ever called. The moment a
//! winget item reaches a handler method here it will panic loudly — by design, until
//! Lane B lands.
//!
//! [`WingetExecutor`]: crate::handler::WingetExecutor

use crate::handler::Handler;
use crate::types::{
    Applied, HandlerKind, Item, Json, PlannedAction, Probe, RecordedAction, Target,
};
use crate::ApplyCtx;
use crate::Result;

pub struct WingetHandler;

impl WingetHandler {
    pub fn new() -> Self {
        WingetHandler
    }
}

impl Default for WingetHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl Handler for WingetHandler {
    fn kind(&self) -> HandlerKind {
        HandlerKind::Winget
    }

    fn probe(&self, _target: &Target) -> Result<Probe> {
        todo!("Lane B — Windows winget probe (winget list <pkg>)")
    }

    fn plan(&self, _desired: &Item, _probe: &Probe) -> Result<Option<PlannedAction>> {
        todo!("Lane B — Windows winget plan")
    }

    fn apply(&self, _action: &PlannedAction, _ctx: &ApplyCtx) -> Result<Applied> {
        todo!("Lane B — Windows winget install + re-probe")
    }

    fn undo(&self, _action: &RecordedAction, _ctx: &ApplyCtx) -> Result<()> {
        todo!("Lane B — Windows winget uninstall (best-effort)")
    }

    fn synthesize_undo(&self, _target: &Target, _before: &Json, _probe: &Probe) -> Result<Applied> {
        todo!("Lane B — Windows winget crash reconcile")
    }
}
