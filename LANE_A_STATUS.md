# Lane A Tail — Status Report

Candylane Phase 1, Lane A tail (store impl, profile parser, engine integration test, CI).
Synthesis of per-file implementation + adversarial review. **No compiler has run** — every
claim below is from reading the frozen contracts (`types.rs`, `handler.rs`, `store.rs` trait,
`engine.rs`, `0001_init.sql`) and matching the new code against them by hand.

---

## 1. WHAT WAS BUILT

- `crates/candylane-core/src/store.rs` — all 11 `StateStore` methods + `SqliteStore::open` implemented over `rusqlite` (WAL, `foreign_keys=ON`, gated migration); exact enum↔TEXT mappings; RFC3339 timestamps; JSON columns via `serde_json`.
- `crates/candylane-core/src/profile.rs` — `parse(path)` / `parse_str(s, name)` deserialize the minimal TOML into ordered `Vec<Item>` (winget → dotfiles → post_install), compute lowercase 64-char hex SHA-256 of raw bytes, return `engine::Profile`; 7 unit tests inline.
- `crates/candylane-core/tests/engine_transaction.rs` — 816-line integration suite: in-memory `FakeStore` + scriptable `FakeHandler` + `FakeRegistry` + `simulate_pull` helper; 6 tests covering all 5 engine scenarios (all-ok, action-k-fails rollback, reconcile applied/skipped paths, bounded rollback, re-pull no-op).
- `.github/workflows/ci.yml` — 3 jobs: `lint_test` (ubuntu: fmt/clippy/test), `integration_windows` (windows-msvc: full test incl. `cfg(windows)` ACL/winget), `audit` (cargo-audit + cargo-deny).

---

## 2. FIX BEFORE COMPILE

Ranked most-severe first. (`store.rs` and `profile.rs` reported **0 compile_blockers** and verified clean against the contracts.)

1. **`.github/workflows/ci.yml` — `cargo deny check` will fail: no `deny.toml` exists**
   *(compile_blocker, confirmed: `deny.toml` is absent from repo root).* Line 75 runs `cargo deny check`; with no config the `audit` job dies at runtime with "failed to parse config file: No such file or directory."
   **Fix:** add a minimal `deny.toml` at repo root (`[advisories]` / `[licenses]` / `[bans]` / `[sources]` sections), or drop/comment the `cargo deny check` step until Lane A/B settle dependencies. `cargo audit` alone is unaffected.

2. **`crates/candylane-core/src/handler.rs` — `Handler` trait is missing `synthesize_undo`**
   *(contract_drift — will become a compile error the moment `reconcile` is fleshed out).* `engine.rs:175` calls `todo!("handler.synthesize_undo(&action.target, &real) -> (after, undo); set_action_applied")` in the `real != before` branch; the method is not on the trait. Today it compiles (it is a `todo!()` string, not a real call) but the reconcile "applied" path is unimplemented, and `tests/engine_transaction.rs` documents the gap with `#[should_panic(expected = "not yet implemented")]` on `recover_reconciles_inflight_applied_path`.
   **Fix:** add to the `Handler` trait `fn synthesize_undo(&self, target: &Target, probe: &Probe) -> Result<(Json, Json)>;` returning `(after_json, undo_json)` from post-crash observed state, then replace the `todo!()` in `reconcile` with a call to it + `store.set_action_applied`. Drop the `#[should_panic]` once landed. Every `Handler` impl (Lanes B/C/D) must then implement it.

No `logic_bug` or `sql_bug` findings were raised, and none surfaced on hand-review:
- `store.rs::migrate` uses `.optional().unwrap_or(None)` — correct: `optional()` only folds `QueryReturnedNoRows`; a "no such table: meta" error is a different variant that `unwrap_or(None)` deliberately collapses to "migrate now." Verified against the schema's seed `INSERT INTO meta ... '1'`.
- All `*_to_str` / `*_from_str` mappings match the `0001_init.sql` CHECK sets and the serde `snake_case` rename exactly (`best_effort`, `one_way`, `undo_failed`, `partially_reverted`, `revert_failed`).
- `begin_op` hardcodes `candylane_version` to `env!("CARGO_PKG_VERSION")` — an accepted layer assumption, not a bug.

---

## 3. VERIFY SEQUENCE

Run in order the moment a Rust toolchain exists (pinned `1.77.0` via `rust-toolchain.toml`):

```
cargo fmt --all
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

The `engine_transaction` integration test and the `profile.rs` pure unit tests run on **Linux now**.
The `winget` real-executor and `cfg(windows)` ACL tests need a **Windows host** (CI `integration_windows`).

---

## 4. STILL STUBBED (`todo!()`)

- **Handlers** — `WingetHandler`, `DotfileHandler`, `ScriptHandler` (Lanes B/C/D): not yet present; `lib.rs` `handlers` module is still commented out.
- **Real `WingetExecutor`** — only the trait exists (`handler.rs`); the `winget.exe`-shelling impl is Lane B.
- **Crypto ACL leaf** — `candylane-crypto` owner-only ACL set/assert (`windows-acl` carve-out, CRITICAL #3): Lane E, not started.
- **Engine leaves** — `engine.rs::preflight()` (`todo!`, winget-present + reboot-pending pre-check) and `engine.rs::reboot_pending()` (`todo!`, CBS/PendingFileRenameOperations probe).
- **Reconcile undo-synthesis** — `engine.rs::reconcile` `real != before` branch is still `todo!("handler.synthesize_undo ...")` (blocked on item #2 above).
- **`Engine::pull_without_preflight`** — referenced in the test's module note as a future test-only entry point; not added (tests currently bypass via the `simulate_pull` helper, so this is optional, not blocking).

---

## 5. CONFIDENCE

This code is **UNVERIFIED — no compiler has run**. The adversarial review (which found 1 CI
runtime blocker + 1 trait contract-drift, both above) is the only gate so far; correctness past
hand-matching the frozen contracts is unproven until the VERIFY SEQUENCE passes on a real toolchain.
