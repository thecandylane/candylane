# CLAUDE.md — Candylane working guide

## What Candylane is
The power user's bootstrap layer for Windows. One command rebuilds your whole machine
personality (debloat + harden + WSL + dotfiles + secrets + VPN mesh + toolkit) from a
signed TOML profile, online or offline, no central server. The promise is **SHAZAM: one
word, full power, any machine.**

Stack: Rust workspace targeting `x86_64-pc-windows-msvc`. Wraps winget/scoop; does **not**
reinvent package management.

Read before non-trivial work:
- [MANIFESTO.md](./MANIFESTO.md) — values. Every tradeoff is measured against it.
- [PHASE1_ARCHITECTURE.md](./PHASE1_ARCHITECTURE.md) — the locked Phase 1 design. **Source of truth for current work.**
- [ROADMAP.md](./ROADMAP.md) — the 12-phase spine (stop at any Era, still worth having).
- [REFERENCES.md](./REFERENCES.md) — what to steal from, per subsystem. Point coding agents at *specific files* in those repos, not abstract descriptions.
- [VOCABULARY.md](./VOCABULARY.md) — the sweet-shop lexicon (box ⊃ tin ⊃ biscuit, lanes, jar, chimney). User-facing names ↔ code names.

## Current state
Pre-alpha. **Phase 1 — the keystone. Milestone 2 reached: the transaction engine compiles and is
proven.** `cargo check` + `clippy -D warnings` clean, **13/13 tests green** on Rust 1.96.0 (Linux).

Built + tested: the state engine ([engine.rs](./crates/candylane-core/src/engine.rs)), `SqliteStore`
([store.rs](./crates/candylane-core/src/store.rs)), the profile parser
([profile.rs](./crates/candylane-core/src/profile.rs)), the `Handler`/`StateStore`/`WingetExecutor`
contracts, the schema, CI, `deny.toml`. Engine tests run against fakes (`FakeStore`/`FakeHandler`),
so the orchestration logic (pull / rollback / reconcile-skipped / bounded-rollback / idempotency) is
verified — but against fakes, not real I/O.

Still stubbed (`todo!()`): the three real handlers (winget/dotfile/script — Lanes B/C/D), a
`SqliteStore` round-trip test (the DB layer compiles but isn't exercised yet), `synthesize_undo`
(reconcile applied-path; its test is `#[should_panic]`), crypto ACL (Lane E), `preflight`/
`reboot_pending`, and the CLI command bodies (Lane F). Next Linux-runnable target: the dotfile +
script handlers + a real `pull`/`revert` vertical slice (winget waits for Windows).

## The prime directive
Phase 1 must be **surgical**. The acceptance bar, non-negotiable:
> fresh Win11 VM → `candylane pull` → the machine you wanted → `candylane revert` →
> **functional-clean vanilla. 10x in a row, zero breaks.**

"Functional-clean vanilla" = `winget list` shows zero managed packages ∧ `probe()` returns the
recorded before-state for every target ∧ managed PATH entries removed. Registry/shell crumbs may
remain, and `diff`/`history` must say so. **Never claim more reversibility than winget can deliver.**

Do not start Phase 2 until the 10x loop is boring. Every later phase can ship rough and iterate;
Phase 1 cannot.

## Architecture (Phase 1) — the one idea
Compute a plan, execute action-by-action, record an undo recipe per action. Revert replays the
recipes in reverse. **Success is read from `probe()`, never from a subprocess exit code.**

3 crates: `candylane-cli` (clap), `candylane-core` (engine + `Handler` trait + handlers + profile),
`candylane-crypto` (Ed25519 + owner-only ACL — the one `windows-rs` carve-out).

Keystone code: [engine.rs](./crates/candylane-core/src/engine.rs) — `reconcile()` → `rollback()` →
`finalize_op()`. Treat it as load-bearing; it encodes the bugs three reviewers found. Full detail
in [PHASE1_ARCHITECTURE.md](./PHASE1_ARCHITECTURE.md).

### The CRITICALs (always designed in, never discovered)
1. **Script timeout** — `ScriptHandler` kills the child on `ApplyCtx.timeout`.
2. **Restore integrity** — `DotfileHandler::undo` sha256-verifies the backup before writing; mismatch refuses.
3. **Key perms** — `candylane-crypto` sets + asserts owner-only ACL on every load. Not best-effort.
4. **Crash reconcile** — `recover` probes the in-flight action before rollback (no stranded packages).
5. **Bounded rollback** — a failing `undo` is capped, marked `undo_failed`, rollback continues. Never an infinite loop.

## Build / test / run
```bash
cargo check --workspace                      # first gate (run on a Rust-capable machine)
cargo clippy --workspace -- -D warnings
cargo fmt --all
cargo build --release                        # produces candylane.exe (windows-msvc)
```
Toolchain pinned in [rust-toolchain.toml](./rust-toolchain.toml) — bump deliberately, never float.
Some code is Windows-only (`candylane-crypto` ACL, winget subprocess). Off-Windows, the
`WingetExecutor` seam + a unix perms fallback keep `candylane-core` building and unit-testable.

## Testing
Framework: **Rust built-in test harness (`cargo test`)**.
```bash
cargo test --workspace                       # unit + integration
```
Goal: 100% coverage of handlers + engine. Case list lives in [PHASE1_ARCHITECTURE.md](./PHASE1_ARCHITECTURE.md)
(Test spec: ~32 unit + 5 E2E). Handlers are unit-tested off-Windows via the `WingetExecutor` seam
(inject a fake). The keystone E2E is the Hyper-V 10x loop (`Checkpoint-VM`/`Restore-VMSnapshot`),
Windows-host only.

## Hard rules
- **Reversibility honesty.** Tag every action `inverse | best_effort | one_way | noop`. winget is
  `best_effort`, never `inverse`. `diff` surfaces `one_way` and residue loudly.
- **No secrets in profiles or the repo.** Secrets live in the encrypted vault (Phase 5), referenced
  by name. `.gitignore` blocks `*.key`, `*.db`, `.candylane/`, `*.candy`.
- **No telemetry, no required account.** Manifesto promise. Never add phone-home.
- **Atomic operations, no half-states.** A pull fully applies or rolls back to clean.
- **Visibility over magic.** Nothing touches the machine with admin rights before `diff` shows it.
- **Enums and SQL move together.** The Rust enums map 1:1 to the CHECK sets in
  [migrations/0001_init.sql](./crates/candylane-core/migrations/0001_init.sql).

## Working model (which AI does what)
Split by **risk, not by phase**:
- **Opus (plan + keystone):** architecture, `engine.rs`, `candylane-crypto`, and review of any
  revert/recover/crypto diff.
- **Sonnet max (volume):** handlers against the frozen `Handler` trait, `SqliteStore` SQL, profile
  parser, CLI wiring, tests. Spec-constrained, test-gated.
- **Verification gate is non-negotiable:** code + its tests, run the suite, strong-model review on
  the revert path. The split is only safe because the spec + test list are tight.
- **Build loop before fan-out:** parallel lanes (B/C/D/E) only after Lane A compiles green.

## Skill routing
Invoke the skill when the request matches; the workflow beats an ad-hoc answer. Highest-value here:
- New feature / "is this worth building" / brainstorm → `/office-hours`
- Scope / strategy / "think bigger" → `/plan-ceo-review`
- Architecture / "lock the design" → `/plan-eng-review`
- **Security audit, threat model, supply chain → `/cso`** (this is a security tool — use it often)
- Bug / crash / "why is this broken" → `/investigate`
- Pre-landing diff review → `/review`; independent second opinion → `/codex`
- Ship / PR → `/ship`, then merge + verify → `/land-and-deploy`
- Post-ship docs sync → `/document-release`; weekly retro → `/retro`; learnings → `/learn`
- Eventual web work (candylane.sh, lanes registry) → `/browse`, `/qa`, `/design-review`

## Tools
- **Context7 MCP** for current Rust crate docs (rusqlite, clap, ed25519-dalek, windows-rs, fs2).
  Prefer it over memory for API details — pins move.
- **codex** for independent read-only review of the keystone.
- **Explore agent** for fan-out codebase searches.

## Conventions
rustfmt + clippy clean (`-D warnings`), edition 2021. Explicit over clever. DRY — flag repetition.
Errors via `anyhow` now → `thiserror` taxonomy as it settles. ASCII diagrams in comments for state
machines and multi-step pipelines (engine, handlers); keep them current — stale diagrams mislead.

**Naming ([VOCABULARY.md](./VOCABULARY.md)):** user-facing "box" = the code `Profile` struct
(never `Box` — std conflict). "chimney" = the secrets subsystem (can be the real module name).
Theme nouns in UX/docs; keep verbs and security primitives plain in code and CLI.

## Not yet wired (don't assume)
The distribution pipeline (GitHub Releases + signed installer, rustup-init model), the Hyper-V
clean-VM E2E harness, the real handlers, and the crypto ACL. (CI, SECURITY/THREAT_MODEL, a pinned
1.96.0 toolchain, and a green build/test are now in place.)
