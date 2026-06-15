# B-ACL — Owner-Only Key ACL: Implementation Approach

> **STATUS: SUPERSEDED (2026-06).**  
> Decision: DPAPI (user-scope) is the primary protection for the private Ed25519 signing key on Windows.  
> See the threat-model comparison and product clarification in project history (portability of *bundles* vs. *the authoring identity key*).  
> The private key is author-machine-bound in practice (signs .candy on the laptop; bundles + signatures travel on USB or channels; targets only verify).  
> DPAPI gives ciphertext-at-rest (stolen/copied file is useless) with far less code. Most of the heavy ACL enumeration, drift detection, refuse-hard machinery, and repair valve are not required when the bytes themselves are protected.  
> A simple owner-only grant on the ciphertext file is kept as cheap defense-in-depth.  
> The detailed ACL-negative-predicate logic below is retained for reference / future use on other files if ever needed, but is no longer the plan for `identity.key`.  
> Unix path remains the honest 0600 fallback.  
> Portability of the *private key* between author machines (laptop died → new laptop, same identity) is handled explicitly in Phase 5 via export/re-wrap (or fresh identity). Bundle portability (.candy) is unaffected and was never the private key's job.
> CRITICAL #3. Policy decided: **refuse-hard on load + explicit repair valve.**
> This is the trust anchor for the entire signed-profile/lane story (Phase 5+). Not best-effort.

---

## 0. What this protects — and what it does NOT (honesty first)

The private Ed25519 signing key (`~/.candylane/identity.key`) must not be readable by **other
local unprivileged users**. A leaked key = forged profiles/lanes once signing lands (Phase 5).

It does **not** protect against a local **administrator** (Windows) or **root** (unix): both can
take ownership and rewrite any ACL. Owner-only raises the bar to "other users," not "admin/root."
We say this plainly in code + docs — the same reversibility-honesty rule we apply to winget, applied
to the trust model. (Mitigation that *does* survive admin: the load-time **owner check** turns an
admin take-ownership into detectable tamper — see §3.)

Adjacent, out of scope for B-ACL (tracked under **B-INIT**): locking the *directory* `~/.candylane`.
Locking the key file but not its parent leaves a key-swap vector (an attacker who can write the dir
can replace the key). Note it; don't solve it here.

---

## 1. Two code paths, two guarantees (never collapse them)

| Function | When | Guarantee | May modify? |
|----------|------|-----------|-------------|
| `set_owner_only(path)` | generate / repair | **Correctness** — set the DACL, then verify the set took | yes (sets) |
| `assert_owner_only(path)` | **every load** | **Tamper-evidence** — verify, refuse on mismatch | **no** |

`load()` calls **only** `assert`. It must never re-tighten — silent re-tighten is "ensure" wearing a
disguise, and it hides a possible breach. There is no shared `ensure_acl()` helper. (The current
scaffold already separates `enforce_owner_only` (set) from `assert_owner_only` (verify) and `load()`
already calls only `assert` — keep that; rename `enforce_*` → `set_*` for clarity.)

`set` ends by calling the *same predicate* `assert` uses, so "set succeeded" means "the negative
assertion now holds," not "the API returned OK."

---

## 2. The assert predicate — verify the NEGATIVE (the part that matters)

The load-time check refuses unless **all** hold:

1. **Owner == current user.** (Catches admin take-ownership as tamper.)
2. **DACL is PROTECTED** (inheritance disabled). An inheriting DACL can gain ACEs from the parent
   dir → other-user access without the file's own ACL showing it. Not-protected → refuse.
3. **Exactly one ACE**, and it is: `ACCESS_ALLOWED`, SID == current user, and it is the *only* grant.
   **No other SIDs at all** — not Administrators, not SYSTEM, not Users, not Everyone.
4. **DACL is present and non-NULL.** A NULL DACL grants everyone → refuse. (An empty-but-present
   DACL denies everyone incl. owner → key unreadable → treat as anomaly → refuse + repair.)

The classic bug is checking "owner can read" and stopping. That's theater. The entire property lives
in **3**'s *"and nothing else."* The assert enumerates the actual ACEs and counts them.

---

## 3. Refuse-hard + repair valve (the decided policy)

- `load()` → `assert` → on **any** mismatch, return a typed `AclError` (no auto-fix). The CLI maps it
  to: *"the key's permissions are wrong — it may have been exposed. If you know this is benign drift
  (restored from backup, copied between machines, or created by a pre-B-ACL Candylane), run
  `candylane init --repair-acl` to re-lock it."*
- `candylane init --repair-acl` → after an explicit acknowledgement, calls `set_owner_only` then
  `assert`. **Deliberate, never automatic.** This is the line between "papering over a possible
  breach" (forbidden) and "user knowingly repairs known-benign drift" (allowed). Security property
  preserved; dead-end avoided.

---

## 4. Error taxonomy (introduce `thiserror` here)

This is exactly where the "anyhow now → thiserror as it settles" convention settles — the trust
boundary deserves typed, matchable errors, not strings.

```rust
#[derive(thiserror::Error, Debug)]
pub enum AclError {
    #[error("key ACL grants access to other principals: {extra}")]
    TooPermissive { extra: String },          // → repair-valve message
    #[error("key owner is {found}, expected current user {expected}")]
    OwnerMismatch { found: String, expected: String }, // → repair-valve (+ tamper warning)
    #[error("key DACL is not protected (inherits parent ACEs)")]
    NotProtected,                              // → repair-valve
    #[error("key has a NULL/again-permissive DACL")]
    NullDacl,                                  // → repair-valve
    #[error("could not read key security info: {0}")]
    Unreadable(String),                        // → hard error (not a repair case)
    #[error("failed to set owner-only ACL: {0}")]
    SetFailed(String),                         // generate/repair path
    #[error("ACL verification after set failed: {0}")]
    VerifyAfterSet(String),                    // set succeeded per API but predicate fails
}
```

`TooPermissive | OwnerMismatch | NotProtected | NullDacl` → CLI shows the repair guidance.
`Unreadable | SetFailed | VerifyAfterSet` → hard failure (something is structurally wrong).

---

## 5. Windows implementation (the Decision #8 carve-out)

Mechanism in **stable Win32 terms** (the binding is an open decision — §8):

**set_owner_only:**
- Build a SID for the current user: `OpenProcessToken(TOKEN_QUERY)` → `GetTokenInformation(TokenUser)`.
- Build a DACL with a single `ACCESS_ALLOWED_ACE`: current-user SID, `FILE_ALL_ACCESS`.
- `SetNamedSecurityInfo(path, SE_FILE_OBJECT, DACL_SECURITY_INFORMATION |
  PROTECTED_DACL_SECURITY_INFORMATION [| OWNER_SECURITY_INFORMATION], owner, NULL, dacl, NULL)`.
  `PROTECTED_DACL_SECURITY_INFORMATION` is what disables inheritance.
- Then call `assert_owner_only` and propagate `VerifyAfterSet` on failure.

**assert_owner_only:**
- `GetNamedSecurityInfo(path, SE_FILE_OBJECT, OWNER | DACL)`.
- Owner SID == current-user SID (`EqualSid`) → else `OwnerMismatch`.
- `GetSecurityDescriptorControl` → `SE_DACL_PROTECTED` set → else `NotProtected`.
- DACL present & non-NULL → else `NullDacl`.
- Enumerate ACEs (`AceCount` + `GetAce`): exactly one, `ACCESS_ALLOWED_ACE_TYPE`, SID `EqualSid`
  current user. Any other ACE → `TooPermissive { extra: <sid string(s)> }`.

No shelling to `icacls` (fragile, localized, can't assert structurally).

---

## 6. Unix fallback — honestly weaker (no parity claim)

- `set_owner_only`: `chmod 0600` (exists).
- `assert_owner_only`: `mode & 0o077 == 0` (exists) **AND** `metadata.uid() == geteuid()` (**ADD** —
  currently missing; the negative assertion needs the owner check too). Same refuse-hard.
- Code comments + docs state explicitly: **not** parity with the Windows DACL. `root` reads anything;
  same-uid processes are not isolated from each other; there is no per-process boundary. The 0600
  model protects against *other unprivileged users* — the same scope as Windows-vs-other-users — but
  the root caveat is real and stated. No "equivalent to owner-only ACL" language anywhere.

---

## 7. CLI surface

- `candylane init` — generate identity; `set_owner_only` on create (partly exists).
- `candylane init --repair-acl` — the repair valve (new). Requires explicit acknowledgement.
- All `Identity::load` callers (sign/verify now; profile verify in Phase 5) surface the typed error.

---

## 8. OPEN DECISIONS (yours)

1. **Strict owner-only vs. the OpenSSH set (owner + SYSTEM + Administrators).** Your constraint said
   "nothing else, no other SIDs" → strict. But the most battle-tested implementation of this exact
   check — **Win32-OpenSSH** (it refuses a key whose ACL is too loose) — deliberately *allows* owner +
   `SYSTEM` + `Administrators` and refuses everything else. They chose that at scale to avoid
   real-world breakage. The sharpened tradeoff:
   - *Strict (owner-only):* an admin must **take ownership** to read the key → the load-time owner
     check catches it as tamper. Allowing `Administrators` in the DACL **forfeits that detection** —
     an admin reads silently, owner unchanged, undetected. So strict is the only variant that is
     actually tamper-evident against a local admin.
   - *OpenSSH set:* battle-tested compatibility (backup/VSS-as-SYSTEM, indexing, EDR all expect
     SYSTEM/Admin). But silent admin/SYSTEM read, no detection.
   - **Revised lean: strict, *because* Candylane's case differs from OpenSSH's.** OpenSSH targets
     multi-admin servers where excluding Administrators breaks ops; Candylane's key is a single-user
     signing anchor where tamper-evidence is the whole point. Diverging from the canonical impl is
     justified *here* — but it's a reasoned divergence, not an oversight, and the EDR/backup friction
     is real → watch it on the acceptance VM. If friction bites, the OpenSSH set is the fallback.
     **Your call** — this is the one decision where the prior art and our threat model point different
     directions.
2. **Binding: `windows-acl` (pinned) vs a thin `windows` slice.** Context7 has the official, maintained
   `windows` crate (130k snippets) but **not** `windows-acl` (niche, ~2018, winapi-based). The "verify
   the negative" path wants direct `GetAce` + `GetSecurityDescriptorControl` (the PROTECTED bit) —
   cleaner via the `windows` crate. Decision #8 already permits "windows-acl **or** a thin windows
   slice." **My lean: the `windows` slice.** Confirms a one-line Cargo swap.
3. **Repair command name.** `candylane init --repair-acl` vs `candylane repair-key` vs a future
   `candylane doctor`. Minor; flagging for naming consistency (VOCABULARY).

---

## 9. Test plan (the refuse-on-loosened test is the one that matters)

**Windows** (`#[cfg(windows)]` + `#[ignore]`, live, mirroring `winget_live.rs`):
- generate → `assert` passes.
- **loosen the ACL** (`icacls <key> /grant Users:R`) → `assert` **refuses** with `TooPermissive`.
  ← proves the negative assertion; without this test the whole thing is theater.
- `--repair-acl` → `assert` passes again.
- if feasible: take-ownership → `OwnerMismatch`; enable inheritance → `NotProtected`.

**Unix:**
- 0600 → ok; `chmod 0644` → refuse; repair → ok.

**Cross-platform:** generate → load → sign → verify roundtrip unchanged.

These need a real filesystem + (Windows) real ACLs → live/ignored tests, not pure unit tests.

---

## 10. Build sequence

0. **Decide the binding** (§8.2) — confirm windows-acl vs `windows` slice; swap Cargo dep if needed.
   (Grounding: the `windows` crate docs via Context7; clone `microsoft/windows-rs` if needed.)
1. Typed `AclError` (`thiserror`) in candylane-crypto.
2. Windows `set_owner_only` + verify-after-set.
3. Windows `assert_owner_only` — the negative predicate (§2).
4. Unix `assert_owner_only`: add the owner-uid check.
5. CLI `init --repair-acl`.
6. Live tests (loosen → refuse → repair) on Windows; perms tests on unix.
7. **`/cso` on the diff** — independent security review before merge.

---

---

## 11. Prior art — we are not the first to do this

Per REFERENCES.md ("point agents at *specific files*, not abstract descriptions"), the canonical
implementations of "refuse to load a private key whose perms/ACL are too loose":

- **OpenSSH / Win32-OpenSSH** — *the* reference. The famous `UNPROTECTED PRIVATE KEY FILE!!!` refusal,
  ported to Windows ACLs. It enumerates the key file's DACL and refuses any trustee outside
  **owner + SYSTEM + Administrators**. Read `openssh-portable`'s Windows perm code (the
  `w32-*fileperm*` / `check_secure...` area — *verify exact symbol names in source*) and the
  `OpenSSHUtils` PowerShell module (`Repair-UserKeyPermission` / `Repair-AuthorizedKeyPermission`,
  which set the same ACL we'd set in `--repair-acl`). This is the single best grounding clone for the
  B-ACL build — more directly relevant than the `windows-acl` crate docs. It also *is* §8.1: their
  allow-list is the pragmatic alternative to our strict lean.
- **tailscale** (already in REFERENCES Tier 8) — Go, sets restricted Windows ACLs on its state/key
  store under `C:\ProgramData\Tailscale`. Reference for constructing a protected DACL in practice
  (their `ipn/store` + atomic-file + Windows ACL setup).
- **.NET `System.Security.AccessControl`** (`FileSecurity` / `FileSystemAccessRule`,
  `SetAccessRuleProtection`) — the documented semantic model for owner-only + disable-inheritance.
  We're in Rust, but it maps 1:1 to the Win32 calls in §5 and the MS docs explain the model cleanly.

**A different axis the prior art surfaces (note, don't adopt now):** several tools don't rely on file
ACLs at all — they **encrypt the key at rest**: `sigstore/cosign` (passphrase-encrypted private keys),
`chezmoi` (secrets via age/gpg), Git Credential Manager (DPAPI / Windows Credential Manager),
`age`/`rage` with a passphrase. That sidesteps the ACL problem entirely (a stolen key file is
ciphertext). Phase 1 keeps the key plaintext-at-rest + ACL-protected for simplicity; a passphrase or
**DPAPI** (`CryptProtectData`, user-bound) wrap is the natural Phase 5 hardening. Worth a one-line
mention in THREAT_MODEL so the choice is explicit, not accidental.

---

*Decisions in §8 gate the build. Once you settle them, this becomes the build spec for a clean
Opus session.*
