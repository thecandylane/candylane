//! Reboot-pending detection — the [`RebootCheck`] seam and its implementations.
//!
//! A pending reboot can poison a package install (CRITICAL #5), so the engine consults a
//! `RebootCheck` before a pull ([`Engine::preflight`]) and after each install (the mid-pull
//! gate). This is an injectable seam — mirroring `WingetExecutor` / `HandlerRegistry` — so
//! the abort path is testable off-Windows with a fake.
//!
//! ## What counts as "reboot pending"
//!
//! Three Windows signals exist; they are NOT equal:
//!
//! | signal | meaning | gates a pull? |
//! |--------|---------|---------------|
//! | CBS `RebootPending` | Component-Based Servicing needs a reboot | **yes** |
//! | WU `RebootRequired` | Windows Update needs a reboot | **yes** |
//! | `PendingFileRenameOperations` | files queued to move on next boot | **no — advisory** |
//!
//! CBS∨WU mean "Windows servicing genuinely requires a reboot before more installs are
//! safe" — exactly the failure CRITICAL #5 guards. `PendingFileRenameOperations` (PFRO) is
//! noisy: installers (winget included) queue file renames as ordinary work, so PFRO is True
//! on healthy machines and *becomes* True mid-pull simply because the pull is installing
//! things. Gating on PFRO would refuse pulls on most real systems and would roll back the
//! first install of an otherwise-clean run. So PFRO is captured in [`RebootState`] but never
//! blocks. (Persisting it to the op log is a tracked follow-up — needs an operations-table
//! column.) The CBS∨WU predicate lives in exactly one place: [`RebootState::must_abort`].
//!
//! [`Engine::preflight`]: crate::engine::Engine

use crate::Result;

/// The three reboot-pending signals, read together. `must_abort()` is the single gate
/// predicate used by both preflight and the mid-pull check (so they cannot drift).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RebootState {
    /// Component-Based Servicing `RebootPending` key present.
    pub cbs: bool,
    /// Windows Update `RebootRequired` key present.
    pub wu: bool,
    /// Session Manager `PendingFileRenameOperations` value present (advisory only).
    pub pfro: bool,
}

impl RebootState {
    /// The gate: a pull must abort iff Windows servicing genuinely needs a reboot. PFRO is
    /// deliberately excluded (noisy — see module docs).
    pub fn must_abort(&self) -> bool {
        self.cbs || self.wu
    }

    /// Human-readable list of the signals that are set, for error/log messages.
    pub fn reasons(&self) -> String {
        let mut parts = Vec::new();
        if self.cbs {
            parts.push("CBS RebootPending");
        }
        if self.wu {
            parts.push("WindowsUpdate RebootRequired");
        }
        if self.pfro {
            parts.push("PendingFileRenameOperations (advisory)");
        }
        if parts.is_empty() {
            "none".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// The seam. The engine reads reboot state through this; production wires
/// [`PowerShellRebootCheck`] on Windows and [`NoRebootCheck`] elsewhere, tests inject a fake.
pub trait RebootCheck {
    fn state(&self) -> Result<RebootState>;
}

/// Always-clear check. The cross-platform default: off-Windows nothing sets these flags,
/// so a pull never aborts for a pending reboot. Also the simplest base for tests that don't
/// care about the reboot gate.
pub struct NoRebootCheck;

impl RebootCheck for NoRebootCheck {
    fn state(&self) -> Result<RebootState> {
        Ok(RebootState::default())
    }
}

/// Real Windows detector: shells PowerShell (no new dependency — matches core's existing
/// "shell subprocesses" pattern, and keeps the `windows-rs` carve-out confined to
/// `candylane-crypto`). Reads the two gating registry keys + the advisory PFRO value.
#[cfg(windows)]
pub use windows_impl::PowerShellRebootCheck;

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use anyhow::Context;
    use std::process::Command;

    /// Probes reboot-pending via `powershell -NoProfile`. PowerShell (not `reg query`)
    /// because the keys live under paths with spaces ("Component Based Servicing",
    /// "Session Manager") that `Test-Path` / `Get-ItemProperty` handle cleanly.
    pub struct PowerShellRebootCheck;

    impl PowerShellRebootCheck {
        pub fn new() -> Self {
            PowerShellRebootCheck
        }
    }

    impl Default for PowerShellRebootCheck {
        fn default() -> Self {
            Self::new()
        }
    }

    // One script prints exactly three lines (CBS / WU / PFRO) as True/False, so parsing is
    // trivial and the three reads are atomic w.r.t. a single PowerShell launch.
    const PROBE_SCRIPT: &str = "\
$cbs = Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Component Based Servicing\\RebootPending';
$wu  = Test-Path 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\WindowsUpdate\\Auto Update\\RebootRequired';
$sm  = Get-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\Session Manager' -Name PendingFileRenameOperations -ErrorAction SilentlyContinue;
Write-Output $cbs; Write-Output $wu; Write-Output ($null -ne $sm)";

    impl RebootCheck for PowerShellRebootCheck {
        fn state(&self) -> Result<RebootState> {
            let out = Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", PROBE_SCRIPT])
                .output()
                .context("failed to spawn powershell for reboot-pending check")?;
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut lines = stdout
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .map(|l| l.eq_ignore_ascii_case("true"));
            let cbs = lines.next().unwrap_or(false);
            let wu = lines.next().unwrap_or(false);
            let pfro = lines.next().unwrap_or(false);
            Ok(RebootState { cbs, wu, pfro })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn must_abort_only_on_cbs_or_wu() {
        assert!(
            !RebootState::default().must_abort(),
            "all-clear must not abort"
        );
        assert!(RebootState {
            cbs: true,
            ..Default::default()
        }
        .must_abort());
        assert!(RebootState {
            wu: true,
            ..Default::default()
        }
        .must_abort());
        // PFRO alone must NOT abort — the whole point of the advisory distinction.
        assert!(
            !RebootState {
                pfro: true,
                ..Default::default()
            }
            .must_abort(),
            "PendingFileRenameOperations alone must not block a pull"
        );
    }

    #[test]
    fn reasons_lists_set_signals() {
        let s = RebootState {
            cbs: true,
            wu: false,
            pfro: true,
        };
        let r = s.reasons();
        assert!(r.contains("CBS"));
        assert!(r.contains("advisory"));
        assert!(!r.contains("WindowsUpdate"));
        assert_eq!(RebootState::default().reasons(), "none");
    }

    #[test]
    fn no_reboot_check_is_always_clear() {
        assert_eq!(NoRebootCheck.state().unwrap(), RebootState::default());
    }
}
