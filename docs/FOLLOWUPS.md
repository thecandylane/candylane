# Candylane — Follow-ups & Known Gaps

The single in-repo tracker for everything known-but-not-done. Travels with clones (it's tracked),
so every parallel lane sees the same backlog. Strategic phase work lives in [ROADMAP.md](./ROADMAP.md);
this file is the tactical list. Mirror to GitHub Issues after the first push if you want a board —
this stays the source of truth.

**Status key:** 🔴 open · 🟡 partial · ⏸ deferred-by-design
**Updated:** 2026-06-06 (WingetHandler landed + proven on real winget; see Resolved).

> **Host premise corrected (2026-06-06):** the dev box is a Win11 laptop running WSL2 Ubuntu — it
> **is** a Windows host. winget + a native msvc `cargo build`/`cargo test` of `candylane.exe` both
> work from the WSL shell (rsync the tree to a `C:` dir first; UNC `\\wsl.localhost\` is blocked in
> the interop session). So "Windows-gated" items below are now *buildable + testable here*, not
> blocked on hardware. The Hyper-V loop still needs an elevated shell. (Memory: `windows-build-from-wsl`.)

---

## Resolved 2026-06-06

The hardening pass closed these (all Linux-tractable; verified by `cargo test` — 55 green — + clippy
`-D warnings` + fmt). Kept here for the trail; prune on the next docs pass.

- **F8** — `recover()` now records an `OpKind::Recover` op linked to the interrupted pull (audit trail).
- **F10** — `finalize_op` removes an op's backup directory on a *fully clean* revert (kept on
  RevertFailed/PartiallyReverted for manual retry). Done at op level, not in `undo()`, so undo stays idempotent.
- **F11** — real-store crash-recover test (`tests/vertical_slice.rs::recover_after_simulated_crash_through_real_store`).
- **C-LOCK** — `core::lock::Lock` (fs2 advisory lock); `pull`/`revert`/`recover` are single-writer, fail-fast. Crash-safe via OS flock (no PID check needed).
- **C-ATOMIC** — dotfile target/backup/restore writes go through `atomic_write` (temp + `rename`); no torn-file window.
- **E-ONEWAY** — engine-level test that a one-way action ends `UndoSkipped`, never `Reverted`.
- **B-CLI** — `history` (via `StateStore::list_operations`) and `status` (drift check via re-probe) are implemented.
- **B-LEXICON** — CLI help text + value names use the lexicon (`<BOX>`, jar); README + docs already did.
- **B-PROFILE** — `profiles/minimal-dev/` shipped (box + sample dotfile + paired up/down scripts).
- **B-WINGET** — **WingetHandler is real and proven.** Shells `winget.exe` through the
  `WingetExecutor` seam with `--silent --accept-source-agreements --accept-package-agreements
  --disable-interactivity`; probe via `winget list --id <pkg> --exact` (truth read from the parsed
  Id row, NOT the exit code); `best_effort` undo with an ownership guard (`was_present_before` →
  no-op so we never uninstall a package the user already had); idempotent undo; `synthesize_undo`
  rebuilds the recipe from the target. **17 fake-driven unit tests pass on both Linux and the msvc
  target.** Live-validated on real winget (`tests/winget_live.rs`, `#[cfg(windows)]` + `#[ignore]`):
  read-only probes + a full mutating round-trip (ZoomIt: absent → installed v12.00 → absent),
  cross-checked against raw `winget list` out-of-band at both ends. Folds in **T14** (winget arg
  array is a fixed, validated flag set). Still open: recorded PATH-delta cleanup is deferred to
  C-PATCH (Phase 3 registry machinery) — the undo is artifact-level, as the `best_effort` tag says.
- **Windows build chain** — rustup + MSVC Build Tools installed; native msvc `cargo build` produces
  a runnable `candylane.exe` from WSL (rsync→C: recipe). Toolchain is rustc/cargo 1.96.0 (matches
  the pin). `candylane-crypto` + `windows-acl` compile on the real target for the first time.

---

## A. Open review findings

| ID | Sev | Status | Item | Where | Notes |
|----|-----|--------|------|-------|-------|
| **F12** | Low | 🟡 | `DotfileHandler::expand()` only handles leading `~` and `$HOME`. `${HOME}`, `%APPDATA%`, `$USERPROFILE`, mid-path vars are unhandled (leading unknown `$VAR` is now *rejected*, so it fails loudly). | `dotfile.rs::expand` | Windows path-var expansion — lands with the Windows work. |
| **F13** | Med | 🔴 | **`clippy -D warnings` FAILS on the msvc target** — 10 dead-code/unused-import errors in unix-gated test code (`vertical_slice.rs` fixture + imports, `script.rs::tempfile_path` + `Instant`). The whole `vertical_slice.rs` exercises the unix script path but isn't `#[cfg(unix)]`, so on Windows the test fns compile out and orphan their helpers. The Linux-only CI never sees it. | `tests/vertical_slice.rs`, `handlers/script.rs` (test mods) | Mechanical fix: `#[cfg(unix)]` the unix-only test fns/helpers/imports (or gate the whole file). Surfaced when clippy was first run on msvc (now possible — see host-premise note). winget code itself is clean on both targets. |

---

## B. Phase 1 remaining — Windows work (buildable + testable from this WSL host; see premise note)

| ID | Status | Item | Where | Notes |
|----|--------|------|-------|-------|
| **B-ACL** | 🔴 | **Crypto owner-only ACL** (Lane E / CRITICAL #3). | `candylane-crypto/src/lib.rs` | Windows `windows-acl` carve-out `todo!()`; unix is a 0600 fallback. Must set + assert owner-only on every load. (`windows-acl` now compiles on the real target — see Resolved.) |
| **B-PREFLIGHT** | 🟡 | `preflight()` / `reboot_pending()` real Windows impl. | `engine.rs` | unix cfg-gated to `Ok(())` / `false`. Windows: winget-present check; CBS `RebootPending` + `PendingFileRenameOperations`. |
| **B-INIT** | 🟡 | `candylane init` — full `~/.candylane/` setup + key ACL. | `candylane-cli/src/main.rs` | Generates the keypair today; the jar dirs are created lazily on first pull/lock. Windows ACL comes with B-ACL. |

---

## C. Keystone acceptance

| ID | Status | Item | Where | Notes |
|----|--------|------|-------|-------|
| **C-HYPERV** | 🔴 | **Hyper-V 10x clean-VM E2E loop** (`Checkpoint-VM` / `Restore-VMSnapshot`). | (Lane G, Windows host) | The actual Phase-1 acceptance bar: fresh Win11 → pull → functional-clean → revert → vanilla, ×10. The Linux half is proven; this is not. Phase 1 is **not** done without it. |
| **C-PATCH** | ⏸ | Registry/PATH delta capture for "vanilla." | — | Deferred-by-design to Phase 3 (rides the debloat engine's registry machinery) — Decision #7. Tracked, not forgotten. |

---

## D. Phase 0 leftovers (non-code, external)

| ID | Status | Item | Notes |
|----|--------|------|-------|
| **D-DOMAIN** | 🔴 | Register `candylane.sh` / `.io` / `.dev` + trademark check. | Owner is handling on the side. |
| **D-DIST** | 🔴 | Signed `.exe` distribution / installer pipeline (rustup-init model). | Relates to T8/T9 in [THREAT_MODEL.md](./THREAT_MODEL.md). |
| **LICENSE** | 🔴 | Choose + add a LICENSE (README says "not yet chosen"). | Pick before the first public push. |

---

## E. Smaller gaps

| ID | Status | Item | Notes |
|----|--------|------|-------|
| **E-T14** | ✅ | THREAT_MODEL **T14**: winget arg-array validation. | Resolved with B-WINGET — the winget arg set is a fixed, hardcoded list of flags + the `--id <pkg> --exact` pair (no shell, no user-interpolated args). Dotfile traversal guard already landed. |

---

## How to use this file

- When you start an item, change its status and (optionally) note the branch/PR.
- When you finish one, move it to "Resolved" with a one-line note; prune the Resolved block on the next docs pass (history lives in git).
- New gaps a review or build surfaces go here the same day — a silent gap reads as "covered" when it isn't.
- Keep [CLAUDE.md](../CLAUDE.md) "Current state" and [ROADMAP.md](./ROADMAP.md) Phase 1 in sync with this list.
