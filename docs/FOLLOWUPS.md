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
- **B-PREFLIGHT** (reboot half) — `preflight()` / `reboot_pending()` are real, behind the new
  injectable `RebootCheck` seam (`core::reboot`): `PowerShellRebootCheck` (Windows) probes CBS∨WU
  (hard gate) + PFRO (advisory), `NoRebootCheck` is the cross-platform default. **Spec amendment
  recorded as Decision #9** (reboot-pending = CBS∨WU, PFRO advisory; same predicate at preflight +
  mid-pull via `RebootState::must_abort`). The seam let the former `simulate_pull` test stopgap be
  **deleted** — engine tests now drive the real `Engine::pull`. New abort-path tests prove CBS
  aborts (no action written) and PFRO-only proceeds. Remaining: explicit winget-present preflight
  (B-PREFLIGHT2, low).
- **Reboot-probe hardening** (review follow-up) — `reboot_state_lenient()`: an *unreadable*
  reboot probe (PowerShell spawn/parse failure) now **fails open with a loud advisory** instead of
  propagating as a pull failure (a probe error must not be indistinguishable from a real
  reboot-pending, nor break the 10x loop on a transient hiccup). `reboot_pending()` dropped its
  `Result` accordingly. Real probe uses the **absolute** `%SystemRoot%\System32\…\powershell.exe`
  (stripped-PATH VMs can't hide it). New test `pull_proceeds_when_reboot_probe_errors` locks the
  fail-open policy. The mutating live tests now **hard-fail** on a dirty precondition (package
  present at start) instead of skip-green — a mutating test that false-passes on a dirty machine is
  worse than none.
- **Lane B end-to-end** — `tests/winget_live.rs::engine_pull_then_revert_winget_through_store`
  (`#[cfg(windows)]` + `#[ignore]`): a full `Engine::pull` (real preflight + real `SqliteStore`)
  installs a winget package, `revert_last` reads the persisted recipe back and uninstalls.
  Live-validated (ZoomIt, cross-checked vs raw winget at both ends). WingetHandler is now proven
  **wired through the engine + store**, not just in isolation.

---

## A. Open review findings

| ID | Sev | Status | Item | Where | Notes |
|----|-----|--------|------|-------|-------|
| **F12** | Low | 🟡 | `DotfileHandler::expand()` only handles leading `~` and `$HOME`. `${HOME}`, `%APPDATA%`, `$USERPROFILE`, mid-path vars are unhandled (leading unknown `$VAR` is now *rejected*, so it fails loudly). | `dotfile.rs::expand` | Windows path-var expansion — lands with the Windows work. |
| **F14** | Low | 🔴 | **Reboot state is not persisted to the op log.** Decision #9 / the design note call for recording all three booleans (CBS/WU/PFRO) per operation for later debugging; `preflight`/`reboot_pending` currently read `RebootState` but only gate on it — PFRO is captured in-memory, never written. The fail-open "reboot state unknown" advisory is likewise only an `eprintln`, not persisted. | `engine.rs`, `store.rs`, `migrations/` | Needs an `operations` column (enums-and-SQL-move-together) + a store setter. Deferred from the reboot-seam commit to keep that diff schema-free. The advisory distinction still works (PFRO never blocks); this is purely the audit trail. |
| **F15** | Low | 🔴 | **PowerShell spawn cost in the mid-pull loop.** `reboot_pending()` runs after every install, so an N-package profile spawns PowerShell N times per pull, ×10 in the acceptance loop. Can't cache (the state legitimately changes mid-pull — that's the point). Fine for `minimal-dev`; watch before profiles get large. | `engine.rs`, `reboot.rs` | Possible later win: a lighter probe (direct registry read via the crypto crate's windows API, or a single batched check) if it ever shows up in loop timing. Not a correctness issue. |
| **F16** | Low | 🟡 | **A genuine CBS/WU reboot-pending makes a profile un-pullable in one shot** (correct per Decision #9 — don't poison the next install — but it aborts mid-pull + rolls back). The abort errors are clear (`preflight`: "a system reboot is already pending (…)"; mid-pull: "reboot pending after applying X — rolled back"), but `diff`/`history` don't yet surface *why* after the fact. | `engine.rs`, CLI `diff`/`history` | `minimal-dev` (Git/VSCode/ripgrep) almost certainly never trips CBS, so the acceptance test is safe. Make `history` show the reboot-abort reason so it doesn't read as a mysterious failure when it eventually happens. Ties to F14 (persist reboot state). |
| **F17** | Med | 🔴 | **The msvc suite no longer runs the money test.** `#![cfg(unix)]` on `vertical_slice.rs` (the F13 fix) is correct, but it means dotfile + script **through the engine** are proven only on Linux. The winget engine test covers winget on Windows; nothing covers the other two handlers through the engine on the shipping target. | `tests/vertical_slice.rs` (unix-only), Hyper-V acceptance profile | **Close via the Hyper-V loop, not a unit test:** the acceptance profile must include a **dotfile + a script + winget** (not winget alone), so the 10x loop exercises all three handlers through the engine on Windows — the keystone doubling as the Windows vertical-slice proof. See Hyper-V prep. |
| **F13** | Med | ✅ | **`clippy -D warnings` now passes on the msvc target.** Gated `tests/vertical_slice.rs` with file-level `#![cfg(unix)]` (it's a unix-only money test — script timeout group-kill is `#[cfg(unix)]`), removing the redundant per-fn gates; gated `script.rs` test `tempfile_path` + `Instant` import with `#[cfg(unix)]`. Verified: `cargo clippy --workspace --all-targets -- -D warnings` = 0 on msvc. | `tests/vertical_slice.rs`, `handlers/script.rs` | Resolved 2026-06-06. **CI now enforces this** — the `lint_test_windows` job runs `clippy --all-targets -D warnings` on msvc (no longer a manual-only step). |
| **F18** | Low | 🟡 | **CI supply-chain hardening (incremental).** Done this pass: fixed the `dtolnay/rust-toolchain` missing-`toolchain`-input bug (the real CI red), SHA-pinned that action off floating `@master`, added `--all-targets` clippy on both targets + a Windows clippy lane, ignored RUSTSEC-2025-0119 (number_prefix, unmaintained-not-vuln) in `deny.toml`. **Remaining:** (a) `actions/checkout@v4` runs on Node20, deprecated — bump before Sept 2026; (b) SHA-pin the *other* third-party actions (checkout, Swatinem/rust-cache, taiki-e/install-action) + add Dependabot to bump them. | `.github/workflows/ci.yml` | Not blocking; the build is green. Gold-plating the action pins + Dependabot is the "fully bulletproof" tail. |

---

## B. Phase 1 remaining — Windows work (buildable + testable from this WSL host; see premise note)

| ID | Status | Item | Where | Notes |
|----|--------|------|-------|-------|
| **B-ACL** | 🟡 | Crypto key protection (DPAPI) | `candylane-crypto/src/lib.rs` | Decision locked + DPAPI wired (protect/unprotect, fatal on unprotect failure, zeroize added, Foundation feature). Windows msvc build not yet verified in this session (run native). No owner-grant defense-in-depth implemented yet (DPAPI is primary; claims updated). Windows roundtrip test stub needed. See B-ACL_APPROACH.md (superseded) and portability clarification. |
| **B-PREFLIGHT2** | Low 🟡 | `preflight()` does the reboot gate (done) but not an explicit winget-present check. | `engine.rs` | A missing `winget.exe` already surfaces as a clear errored action (WingetHandler shells `Command::new("winget")` with context), so this is "fail earlier/clearer," not a correctness gap. Add a cheap `where winget` / version probe to preflight when convenient. |
| **B-INIT** | 🟡 | `candylane init` — full `~/.candylane/` setup + key ACL. | `candylane-cli/src/main.rs` | Generates the keypair today; the jar dirs are created lazily on first pull/lock. Windows ACL comes with B-ACL. |

---

## C. Keystone acceptance

| ID | Status | Item | Where | Notes |
|----|--------|------|-------|-------|
| **C-HYPERV** | 🔴 | **Hyper-V 10x clean-VM E2E loop** (`Checkpoint-VM` / `Restore-VMSnapshot`). | (Lane G, Windows host) | The actual Phase-1 acceptance bar: fresh Win11 → pull → functional-clean → revert → vanilla, ×10. The Linux half is proven; this is not. Phase 1 is **not** done without it. **The acceptance profile MUST include a dotfile + a script + winget** (not winget alone) so the loop exercises all three handlers through the engine on Windows — this is also how F17's coverage gap (the unix-only money test) is closed. Needs an *elevated* PowerShell (Restore-VMSnapshot). |
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
