# Candylane — Follow-ups & Known Gaps

The single in-repo tracker for everything known-but-not-done. Travels with clones (it's tracked),
so every parallel lane sees the same backlog. Strategic phase work lives in [ROADMAP.md](./ROADMAP.md);
this file is the tactical list: deferred review findings, stubs, and quality gaps under the current
phase. Mirror to GitHub Issues after the first push if you want a board — this stays the source of truth.

**Status key:** 🔴 open · 🟡 partial · 🟢 done (kept briefly for history) · ⏸ deferred-by-design
**Updated:** 2026-06-06 (after the Phase 1 vertical slice + adversarial review).

---

## A. Deferred review findings (2026-06-06 adversarial review of the vertical slice)

The review confirmed the revert path sound enough to build on; these were consciously deferred (not
blockers). Severity is the reviewer's.

| ID | Sev | Status | Item | Where | Why it matters / fix |
|----|-----|--------|------|-------|----------------------|
| **F8** | Med | 🔴 | `recover()` never writes an `OpKind::Recover` row — the crash recovery has no audit trail and the `OpKind::Recover` variant is currently dead code. | `engine.rs::recover` | History can't show "a crash was recovered." Fix: `begin_op(Recover, parent=op)` around the reconcile+rollback, or stamp the pull op a distinct terminal status. |
| **F10** | Med | 🔴 | Dotfile backups under `~/.candylane/backups/<op>/` are never cleaned after a successful restore or a terminal op — resource leak **and** lingering copies of the user's original file bytes. | `dotfile.rs::undo`, `engine.rs::finalize_op` | **Do NOT** clean up inside `undo()` — that breaks `undo()` idempotency (a second undo would find the backup gone and bail). Correct fix is **op-level** cleanup at a terminal-clean state (`finalize_op` / after `recover`). |
| **F11** | Med | 🔴 | No real-store integration test for `recover()` / crash-reconcile — only the fakes (`engine_transaction.rs`) cover it. The `synthesize_undo` → `SqliteStore` round-trip is unproven. | `tests/` | The 10x loop needs crash-recover proven through the real store. Fix: a `vertical_slice`-style test that marks an applied action `pending`, calls `recover()`, asserts honest terminal state + consistent files. |
| **F12** | Low | 🟡 | `DotfileHandler::expand()` only handles leading `~` and `$HOME`. `${HOME}`, `%APPDATA%`, `$USERPROFILE`, mid-path vars are unhandled. (Leading non-`$HOME` vars are now **rejected**, so it fails loudly rather than deploying to a literal `$FOO` path.) | `dotfile.rs::expand` | Windows path expansion is a real gap for Windows dotfile targets — lands with the Windows work. |

---

## B. Phase 1 remaining — Windows / lane work (the other half of the keystone)

The cross-platform half (engine, dotfile, script, store, CLI) is proven on Linux. These need a
Windows host and finish Phase 1.

| ID | Status | Item | Where | Notes |
|----|--------|------|-------|-------|
| **B-WINGET** | 🔴 | **WingetHandler** (Lane B) — all five trait methods `todo!()`. | `handlers/winget.rs` | Needs a real `WingetExecutor` (`winget.exe` with `--accept-source-agreements --accept-package-agreements --silent`), probe via `winget list` (success from probe, never exit code), `best_effort` undo + recorded managed-PATH cleanup, and ownership check before uninstall. Windows-only. |
| **B-ACL** | 🔴 | **Crypto owner-only ACL** (Lane E / CRITICAL #3). | `candylane-crypto/src/lib.rs` | Windows `windows-acl` carve-out is `todo!()`; unix is a 0600 fallback. Must **set + assert** owner-only ACL on every key load — not best-effort. The one sanctioned `windows-rs` dependency. |
| **B-PREFLIGHT** | 🟡 | `preflight()` / `reboot_pending()` real Windows impl. | `engine.rs` | unix is cfg-gated to `Ok(())` / `false` so the engine runs off-Windows. Windows needs: winget-present check; CBS `RebootPending` + Session Manager `PendingFileRenameOperations`. |
| **B-CLI** | 🟡 | CLI `history` + `status`. | `candylane-cli/src/main.rs` | Currently `eprintln!("not yet implemented")` + `Ok(())`. `history` needs a `StateStore::list_operations`; `status` validates machine state vs the state DB. |
| **B-INIT** | 🟡 | `candylane init` — full `~/.candylane/` setup. | `candylane-cli/src/main.rs` | Keypair generation works; needs the full directory layout + (Windows) the owner-only ACL from B-ACL. |
| **B-PROFILE** | 🔴 | Ship `candylane/minimal-dev` as a bundled official profile. | (new) | The TOML exists in the spec + parser tests; not yet a shipped artifact. |
| **B-LEXICON** | 🟡 | CLI help text + arg names still use code-names (`profile`, `vault`); align user-facing strings to the lexicon (`box`, `chimney`) per [VOCABULARY.md](./VOCABULARY.md). | `candylane-cli/src/main.rs` | README + docs already use the lexicon; the binary's `--help` lags. Do before first release. |

---

## C. Keystone quality & acceptance (the exit bar + robustness the spec calls for)

| ID | Status | Item | Where | Notes |
|----|--------|------|-------|-------|
| **C-HYPERV** | 🔴 | **Hyper-V 10x clean-VM E2E loop** (`Checkpoint-VM` / `Restore-VMSnapshot`). | (new, Lane G) | The actual Phase-1 acceptance bar: fresh Win11 → pull → functional-clean → revert → vanilla, ×10. The Linux half is proven; this is unproven. Phase 1 is **not** done without it. |
| **C-LOCK** | 🔴 | Single-writer lockfile (`fs2`, fail-fast, PID stale-check). | `engine.rs` / CLI | Spec calls it an "obvious call." Not implemented — two concurrent `candylane` runs could corrupt the state DB. |
| **C-ATOMIC** | 🔴 | Atomic writes for dotfile + DB writes (`rust-atomicwrites`). | `dotfile.rs::apply`, `store.rs` | `apply()` uses `std::fs::write` — a crash mid-write leaves a torn target. Spec calls for atomic writes + WAL. |
| **C-PATCH** | ⏸ | Registry/PATH delta capture for "vanilla." | — | Deferred-by-design to Phase 3 (rides the debloat engine's registry machinery) — Decision #7. Tracked, not forgotten. |

---

## D. Phase 0 leftovers (foundation, non-code)

| ID | Status | Item | Notes |
|----|--------|------|-------|
| **D-DOMAIN** | 🔴 | Register `candylane.sh` / `.io` / `.dev` + trademark check. | ROADMAP Phase 0. |
| **D-DIST** | 🔴 | Signed `.exe` distribution / installer pipeline (rustup-init model). | ROADMAP Phase 0 exit; relates to T8/T9 in [THREAT_MODEL.md](./THREAT_MODEL.md). |

---

## E. Smaller test-coverage gaps

| ID | Status | Item | Notes |
|----|--------|------|-------|
| **E-ONEWAY** | 🔴 | No E2E proving a `OneWay` action ends `UndoSkipped` through the real engine. | Unit-covered in `script.rs`; the engine-level path (rollback → `UndoSkipped`) is logic-only. |
| **E-T14** | 🟡 | THREAT_MODEL **T14**: winget arg-array validation pending (handler not built). | Folds into **B-WINGET**. Dotfile traversal guard already landed. |

---

## How to use this file

- When you start an item, change its status and (optionally) note the branch/PR.
- When you finish one, flip it 🟢 and delete it on the next docs pass (history lives in git).
- New gaps a review or build surfaces go here the same day — a silent gap reads as "covered" when it isn't.
- Keep [CLAUDE.md](../CLAUDE.md) "Current state" and [ROADMAP.md](./ROADMAP.md) Phase 1 in sync with this list.
