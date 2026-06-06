# Candylane Phase 1 — Locked Architecture (v2)

> The keystone phase. `pull` and `revert` must be surgical: fresh Win11 VM → `pull` →
> the machine you wanted → `revert` → **functional-clean vanilla**, **10x in a row,
> zero breaks** ([ROADMAP.md](./ROADMAP.md) Phase 1). Everything here is decided.
> Implementation maps field-for-field and signature-for-signature to what follows.

**Status (2026-06-06):** The cross-platform vertical slice is built and proven on Linux. Engine,
`SqliteStore` (+ round-trip test), profile parser, **DotfileHandler** (CRITICAL #2), **ScriptHandler**
(CRITICAL #1), `HandlerRegistry`, `synthesize_undo`, schema, CI, and a wired CLI
(`pull`/`revert`/`diff`/`recover`/`history`/`status`) compile and pass `cargo test` (**55 green**) + `clippy -D warnings`
+ `fmt` on Rust 1.96.0. [`tests/vertical_slice.rs`](../crates/candylane-core/tests/vertical_slice.rs)
proves `pull` → `revert` → functional-clean (delete + sha256-verified restore) with real handlers and
the real store. Remaining: **WingetHandler** (Lane B, Windows), the crypto owner-only ACL (Lane E,
Windows), Windows `preflight`/`reboot_pending`, CLI `history`/`status`, and the Hyper-V 10x loop.
Known gaps + review follow-ups: [FOLLOWUPS.md](./FOLLOWUPS.md).

**Implementation deltas vs this spec (recorded during the slice):**
- `PlannedAction` gained a `payload: Json` field — `apply()` receives only the `PlannedAction`, not the
  `Item`, so `plan()` stashes handler-specific desired state (dotfile src+sha, script run+undo) there.
- `Handler::synthesize_undo(&self, target, before, probe) -> Result<Applied>` is now a **required**
  trait method (the crash-reconcile leaf), not a TODO. Scripts bail by design (null probe state).
- `ActionStatus::UndoSkipped` added: a `one_way` action that rollback reaches is recorded
  `undo_skipped`, never `reverted` — honesty (status never claims a reversal that didn't happen).
- Exact `before`/`undo` JSON shapes are the doc-comments in `handlers/dotfile.rs` / `handlers/script.rs`
  — **those are the source of truth**; the shapes sketched below are indicative (e.g. script `before`
  is `null`; dotfile keys are `exists`/`sha256`, not `existed`).
- DotfileHandler rejects `..` traversal and unsupported leading variables in targets (THREAT_MODEL T14).

**v2 changelog (three independent reviews — Claude subagent, Grok, +1 — converged on the
revert/recover path):** in-flight reconcile after crash; winget success sourced from
post-install probe, not exit code; rollback-during-rollback is bounded and best-effort;
winget undo is `best_effort` (not `inverse`) and "vanilla" is defined as functional-clean;
reboot-pending modeled; ownership verified before uninstall; `--resume` cut; `WingetExecutor`
seam for off-Windows tests; `windows-acl` carve-out for `candylane-crypto`; `foreign_keys=ON`
and a `meta` table from commit one.

## Locked decisions

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| 1 | Crate layout | **3 crates**: `candylane-cli`, `candylane-core`, `candylane-crypto` (isolated) | Engineered enough. `windows-rs` deferred to Phase 3 **except** the crypto carve-out (#8). |
| 2 | Revert engine | **Inverse/best-effort op log primary**; VSS opt-in (`--restore-point`), never load-bearing | The per-action log powers `diff`/`history`/partial-revert and is air-gap-native (Phase 7). |
| 3 | Post-script reversibility | Paired up/down ⇒ `inverse`. No down-script ⇒ `one_way` (diff warns, revert skips+reports). **Certified `minimal-dev` profile: zero `one_way`.** | Honest reversibility; the test profile is invertible by construction. |
| 4 | Dotfiles | **Copy-manage**, one handler, no symlinks. `candylane add` → Phase 2. | No admin/Dev-Mode dependency; trivial revert (restore bytes). |
| 5 | Crash recovery | **Detect + explicit `candylane recover`**, rollback-to-clean only. **`--resume` cut from Phase 1** (same probe gap as a crash; safe default suffices). Recovery is re-entrant and reconciles the in-flight action first. | "No half-states." Nothing mutates the machine automatically. |
| 6 | Keystone test rig | **Hyper-V + `Checkpoint-VM`/`Restore-VMSnapshot`** | Native Windows-on-Windows. |
| 7 | "Vanilla" bar | **Functional-clean now, tighten later.** Revert asserts: `winget list` shows zero managed packages, `probe()` returns recorded before-state for every target, identifiable managed PATH entries removed. Registry/shell crumbs may remain and `diff`/`history` say so. Registry/PATH delta capture → **tracked Phase 3 TODO** (rides the debloat engine's registry machinery). | winget cannot restore registry/PATH/shortcuts. This bar asserts only what winget can prove, so the 10x loop asserts something true. |
| 8 | `windows-rs` carve-out | **One Windows-API dependency allowed in `candylane-crypto` only** (`windows-acl` or a thin `windows` slice) to set+assert owner-only ACLs. Deferral holds everywhere else (registry/services/DISM). | CRITICAL #3 is unimplementable with `std` on Windows; the deferral was about not doing system surgery early, not about gutting the trust model. The crate boundary contains the exception. |

Obvious calls locked: **delta ownership** (revert only undoes what Candylane installed),
**back up bytes not just hashes**, **single-writer lockfile** (`fs2`, fail-fast, PID
stale-check), **diff is honest** about `best_effort`/`one_way` residue.

## The model

Compute a plan, execute action-by-action, record an undo recipe per action. Revert replays
the recipes in reverse. Success is read from the system, never assumed from an exit code.

```
PROFILE (TOML) ──parse──▶ DESIRED ┐
                                  ├─diff─▶ PLAN (ordered Actions)
SYSTEM ────────probe────▶ ACTUAL ─┘            │
                                               ▼
                       pull = execute Plan, per Action:
   ┌───────────────────────────────────────────────────────────────┐
   │ 1. write intent (status=pending)        ◀── crash-safe point   │
   │ 2. capture before (exists? version? bytes?)                    │
   │ 3. handler.apply()  (winget | copy | run script)              │
   │ 4. handler.probe() AGAIN → confirm real effect (not exit code) │
   │ 5. write after + undo recipe (status=applied)                 │
   │ on Err → status=failed → STOP → rollback this op (seq DESC)    │
   └───────────────────────────────────────────────────────────────┘

revert  = actions WHERE op=last AND status=applied, undo in REVERSE seq
recover = pending op detected → RECONCILE in-flight action (probe vs before,
          synthesize an applied/skipped record) → rollback-to-clean. Re-entrant.
```

## Crate layout

```
candylane-cli      clap parsing, human output, spinners (indicatif)        [bin]
candylane-core     state engine, Handler trait, WingetExecutor seam,       [lib]
                   3 handlers, diff, profile module
candylane-crypto   Ed25519 keygen/sign/verify, key storage,                [lib, isolated]
                   owner-only ACL set+assert (windows-acl carve-out)
```

## State schema (SQLite at `~/.candylane/state.db`, WAL, `foreign_keys=ON`, atomic writes)

```sql
PRAGMA foreign_keys = ON;          -- SQLite does NOT enforce FKs without this. Set every connection.

-- schema version from commit one; painful to retrofit later
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- seed: INSERT INTO meta(key,value) VALUES ('schema_version','1');

-- one row per pull/revert/recover invocation
CREATE TABLE operations (
    id                INTEGER PRIMARY KEY,
    kind              TEXT NOT NULL CHECK (kind IN ('pull','revert','recover')),
    profile           TEXT,
    profile_hash      TEXT,
    parent_op         INTEGER REFERENCES operations(id),
    status            TEXT NOT NULL CHECK (status IN
                        ('pending','applied','failed','reverted',
                         'partially_reverted','revert_failed')),  -- v2: rollback-failure states
    started_at        TEXT NOT NULL,                 -- RFC3339
    finished_at       TEXT,
    candylane_version TEXT NOT NULL
);

-- one row per atomic step, in execution order
CREATE TABLE actions (
    id            INTEGER PRIMARY KEY,
    op_id         INTEGER NOT NULL REFERENCES operations(id),
    seq           INTEGER NOT NULL,                  -- revert replays seq DESC
    handler       TEXT NOT NULL CHECK (handler IN ('winget','dotfile','script')),
    target        TEXT NOT NULL,
    status        TEXT NOT NULL CHECK (status IN
                    ('pending','applied','failed','reverted','skipped',
                     'undo_failed','undo_skipped')), -- v2: undo_failed (gave up); v2.1: undo_skipped (one_way)
    before_json   TEXT NOT NULL,
    after_json    TEXT,
    undo_kind     TEXT NOT NULL CHECK (undo_kind IN
                    ('inverse','best_effort','one_way','noop')),  -- v2: best_effort (winget)
    undo_json     TEXT NOT NULL,
    undo_attempts INTEGER NOT NULL DEFAULT 0,        -- v2: bounds rollback-during-rollback
    undo_error    TEXT,                              -- v2: last undo failure
    error         TEXT,
    UNIQUE (op_id, seq)
);
CREATE INDEX idx_actions_op ON actions(op_id, seq);
```

### Per-handler `before_json` / `undo_json` shapes

```
winget   before {"installed":bool,"version":"x.y.z"|null,"scope":"user|machine"|null}
         undo   {"op":"uninstall","pkg":"Git.Git","version":"x.y.z",   # best_effort
                 "path_entries":["C:\\Program Files\\Git\\cmd"]}        # cleaned on revert if present
                {"op":"noop","reason":"pre-existing"}                   # noop (delta ownership)
         NOTE: undo_kind=best_effort. Revert runs uninstall + removes the managed PATH
               entries we recorded. Registry/shell crumbs may remain (see Decision #7).
               winget invoked with --accept-source-agreements --accept-package-agreements
               --silent (non-interactive; a fresh VM hangs without these).

dotfile  before {"existed":bool,"sha256":"..."|null,"backup":"~/.candylane/backups/<op>/<seq>.bin"|null}
         undo   {"op":"remove","path":"..."}                           # inverse (target was absent)
                {"op":"restore","path":"...","backup":"...","sha256":"..."}  # inverse (target existed)

script   before {}
         undo   {"op":"run","script":"./down.ps1","timeout_s":120}     # inverse (paired)
                {"op":"none","note":"one_way; effects not reverted"}    # one_way
```

## The `Handler` trait + `WingetExecutor` seam

```rust
/// A unit of work the engine can plan, execute, record, and reverse.
/// Implemented by WingetHandler, DotfileHandler, ScriptHandler.
pub trait Handler {
    fn kind(&self) -> HandlerKind;

    /// Pure read of current system state for `target`. Used by diff, recovery,
    /// AND post-apply confirmation. Never mutates.
    fn probe(&self, target: &Target) -> Result<Probe>;

    /// Given desired item + probed state, decide what to do.
    /// Ok(None) when already satisfied (idempotency / re-pull no-op).
    fn plan(&self, desired: &Item, probe: &Probe) -> Result<Option<PlannedAction>>;

    /// Execute a planned action, then RE-PROBE to confirm the real effect.
    /// Success is defined by probe state, not by subprocess exit code.
    /// The engine has already written status=pending BEFORE this is called.
    fn apply(&self, action: &PlannedAction, ctx: &ApplyCtx) -> Result<Applied>;

    /// Reverse a previously-applied action from its recorded recipe.
    /// MUST be idempotent. For best_effort actions, verify ownership before
    /// destroying state (don't uninstall a package the user manually upgraded).
    fn undo(&self, action: &RecordedAction, ctx: &ApplyCtx) -> Result<()>;

    /// Crash-reconcile leaf (CRITICAL #4). When `recover` finds an in-flight action whose
    /// probe != before (apply took effect before the crash), rebuild (after, undo) from the
    /// pre-state + observed probe so rollback can reverse it. Scripts have null probe state
    /// and bail here by design (unreachable in practice).
    fn synthesize_undo(&self, target: &Target, before: &Json, probe: &Probe) -> Result<Applied>;
}

/// Subprocess seam so winget logic is unit-testable off-Windows (Finding #12).
/// Real impl shells `winget.exe`; tests inject a fake.
pub trait WingetExecutor: Send + Sync {
    fn install(&self, pkg: &str) -> Result<RawOutput>;   // adds --accept-* --silent
    fn uninstall(&self, pkg: &str) -> Result<RawOutput>;
    fn list(&self, pkg: &str) -> Result<RawOutput>;      // probe source of truth
}

pub enum HandlerKind   { Winget, Dotfile, Script }
pub enum UndoKind      { Inverse, BestEffort, OneWay, Noop }   // v2: BestEffort
pub enum ActionStatus  { Pending, Applied, Failed, Reverted, Skipped, UndoFailed, UndoSkipped } // v2: UndoFailed; v2.1: UndoSkipped (one_way honesty)
pub enum OpStatus      { Pending, Applied, Failed, Reverted, PartiallyReverted, RevertFailed }

pub struct ApplyCtx<'a> {
    pub backups_dir: &'a Path,      // ~/.candylane/backups/<op>/
    pub timeout:     Duration,      // ScriptHandler enforces (CRITICAL #1)
    pub dry_run:     bool,          // diff = plan()+probe() only; never apply()
    pub max_undo_attempts: u32,     // v2: bounds rollback-during-rollback (CRITICAL #3 sibling)
}
```

### Engine loop (in terms of the trait)

```
pull(profile):
  preflight: winget present? reboot-pending? (abort early if pending)   # v2 (#6)
  op = db.begin(kind=pull, status=pending)
  for (seq, desired) in profile.items().enumerate():
      probe = handler.probe(desired.target)?
      match handler.plan(desired, probe)?:
          None      => db.insert(action, seq, status=skipped)           # idempotent
          Some(pa)  =>
              db.insert(action, seq, status=pending, before=pa.before, undo_kind=pa.undo_kind)
              match handler.apply(pa, ctx):          # apply RE-PROBES; success = real state
                  Ok(applied) =>
                      reboot_pending_check()?         # v2: a package may have set it (#5)
                      db.update(seq, status=applied, after, undo_json=applied.undo)
                  Err(e)      => db.update(seq, status=failed, error=e)
                                 rollback(op); finalize_op(op); return Err(e)
  db.update(op, status=applied)

rollback(op):                  # body of revert() and recover()
  for action in db.actions(op, status=applied) ORDER BY seq DESC:
      loop up to ctx.max_undo_attempts:
          match handler.undo(action, ctx):
              Ok(())  => db.update(action, status=reverted); break
              Err(e)  => db.bump(action.undo_attempts); db.set(action.undo_error=e)
      if still failing: db.update(action, status=undo_failed)   # v2: best-effort CONTINUE,
                                                                  # do NOT abort the whole rollback
  # never loops forever; reports every undo_failed at the end

finalize_op(op):                                                  # v2
  if any action undo_failed → op.status = revert_failed
  elif any action still applied → op.status = partially_reverted
  else → op.status = reverted

recover():                     # on detecting a pending op at startup
  reconcile(pending_op)        # v2 (#1): the big one
  rollback(pending_op)
  finalize_op(pending_op)

reconcile(op):                 # v2 (#1) — the in-flight action's real outcome is UNKNOWN
  a = pending action of op (status=pending), if any
  real = handler.probe(a.target)
  if real != a.before:         # apply actually changed the system before the crash
      synthesize a.after + a.undo_json from `real`; a.status = applied
  else:                        # apply never took effect
      a.status = skipped
  # now rollback() will correctly undo (or skip) it — no stranded packages
```

## The CRITICALs (silent-failure class — designed in, not discovered)

1. **Script timeout** — `ApplyCtx.timeout` enforced in `ScriptHandler::apply`; on elapse kill → `Err` → fail → rollback.
2. **Restore integrity gate** — `DotfileHandler::undo` (`op=restore`) hashes the backup and compares to `undo_json.sha256` **before** writing. Mismatch → refuse + loud error.
3. **Key perms** — `candylane-crypto::Identity::{generate,load}` sets and **asserts on every load** an owner-only ACL via the `windows-acl` carve-out (Decision #8). Not best-effort.
4. **Crash reconcile** (v2) — `recover` reconciles the in-flight action before rollback, so a power-loss mid-install can never strand an untracked package.
5. **Bounded rollback** (v2) — a failing `undo` is retried up to `max_undo_attempts`, then marked `undo_failed`; rollback continues best-effort and reports. No infinite recovery loop.

## Phase 1 minimal profile (`candylane/minimal-dev`)

```toml
name    = "minimal-dev"
version = "0.1"

[packages.winget]
install = ["Git.Git", "Microsoft.VisualStudioCode", "BurntSushi.ripgrep.MSVC"]

[dotfiles]
[[dotfiles.file]]
src    = "./home/.gitconfig"
target = "$HOME/.gitconfig"

[[post_install]]
run  = "./scripts/example-tweak.ps1"
undo = "./scripts/example-tweak.undo.ps1"   # REQUIRED (certified profile: zero one_way)
```

## Test spec (write alongside each handler — ~32 unit + 5 E2E; **55 unit/integration green on Linux today**, Hyper-V E2E still to come)

```
winget   apply: already-installed→skip · ok→best_effort undo
         exit-0-but-probe-says-not-installed→FAIL          # v2 (#2/#8) — the dangerous one
         install-fails→rollback · id-not-found · accept-flags-passed
         undo:  we-own-this-version→uninstall+clean PATH
         uninstall-0-but-probe-still-installed→undo_failed # v2 (#2/#8)
         version-changed-since-install→don't clobber       # v2 ownership (#8)
dotfile  apply: target-absent→copy,undo=remove · target-exists→backup+overwrite,undo=restore · src-missing→err
         undo:  remove · restore+sha256-verify (mismatch→refuse) · target-locked→clear error  # v2 (#6)
         modified-since-pull→detect + warn before overwrite # v2 (reviewer convergence)
script   apply: has-undo→inverse · no-undo→one_way · exit≠0→fail · TIMEOUT→kill→fail
         undo:  inverse→run down · one_way→skip+report
engine   pull all-ok · action-k-fails→rollback 0..k-1 · re-pull→all no-op
         preflight reboot-pending→abort · post-install reboot-pending→fail+rollback  # v2 (#5)
         recover: reconcile in-flight (applied path + skipped path) → rollback     # v2 (#1)
         rollback: undo-fails→undo_failed, rollback CONTINUES, op=revert_failed     # v2 (#3)
         revert reverse-seq · diff exact+best_effort/one_way flagged · history ordered
crypto   init no-key→generate+persist+owner-only-ACL · key-exists→never clobber · load asserts ACL

E2E (Hyper-V)  ★ fresh VM → pull → assert FUNCTIONAL-CLEAN vanilla → ×10
                 vanilla = (winget list: 0 managed) ∧ (probe==before ∀ target) ∧ (managed PATH gone)
               pull → kill mid-install → recover → assert functional-clean      # v2 (#1)
               pull → re-pull → assert no-op, no duplicate rows
               diff → assert preview == actual pull effect
               pull → corrupt a backup → revert → assert refuse-not-clobber     # v2 (#2 CRITICAL)
```

## Failure modes

| Codepath | Realistic failure | Mitigation |
|---|---|---|
| winget apply | exit 0 but not installed (scope fallback / deferred installer) | re-probe after apply; fail on `installed=false` (v2) |
| winget apply | source agreement prompt on fresh VM | `--accept-source-agreements --accept-package-agreements --silent` (v2) |
| winget install | sets reboot-pending → next install aborts | preflight + post-install reboot-pending check (v2) |
| winget undo | user manually upgraded the package | verify owned version before uninstall (v2) |
| dotfile undo | backup bytes missing/corrupt | sha256 gate refuses to write (CRITICAL #2) |
| dotfile undo | target locked by running app (Windows mandatory lock) | distinct error → "close X before reverting" (v2) |
| script apply | post-script hangs forever | timeout kills it (CRITICAL #1) |
| crypto | private key readable by others | owner-only ACL + assert-on-load via `windows-acl` (CRITICAL #3) |
| rollback | an `undo` permanently fails (Defender lock) | bounded retries → `undo_failed` → continue → report (CRITICAL #5) |
| recover | in-flight action's real state unknown | reconcile (probe vs before) before rollback (CRITICAL #4) |
| engine write | disk full mid-write → DB corruption | `rust-atomicwrites` + WAL |

## NOT in scope (Phase 1)

Profile signing/verification (Phase 5), vault/secrets, `extends`/inheritance, lockfile (the
TOML one), debloat engine (Phase 3), WSL (Phase 4), multi-profile, `candylane add` (Phase 2),
`windows-rs` outside the crypto carve-out (Phase 3), **`--resume` (cut)**, and
**registry/PATH delta capture** — deferred to Phase 3, ride the debloat engine's registry
machinery (TRACKED, not forgotten). Phase 1 = one local profile, one local identity, the 10x
functional-clean loop.

## What we reuse (don't reinvent)

`twpayne/chezmoi` (copy-manage + diff/apply), `SubconsciousCompute/privacy-sexy-rs`
(reversible-action pattern in Rust), `hashicorp/terraform` (plan = diff), `rusqlite`,
`untitaker/rust-atomicwrites`, `fs2` (lockfile), `clap`, `console-rs/indicatif`,
`windows-acl` (crypto carve-out). See [REFERENCES.md](./REFERENCES.md).

## Build order (parallelizable)

```
Lane A (must land first):  schema migration + engine + Handler/WingetExecutor traits + types
        ▼
Lane B:  WingetHandler (+ real WingetExecutor)  ─┐
Lane C:  DotfileHandler                          ├─ parallel, each implements Handler
Lane D:  ScriptHandler                           ─┘
Lane E:  candylane-crypto (identity + ACL)        ─ parallel with B/C/D
        ▼
Lane F:  cli wiring (init/pull/diff/revert/recover/history)
        ▼
Lane G:  Hyper-V E2E harness + the 10x functional-clean loop
```
