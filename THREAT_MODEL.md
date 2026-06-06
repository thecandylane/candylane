# Candylane Threat Model

> Written before feature code, per [MANIFESTO.md](./MANIFESTO.md). This is a living
> document; it is revised as phases land. For reporting and policy, see
> [SECURITY.md](./SECURITY.md). Vocabulary: [VOCABULARY.md](./VOCABULARY.md).

## The central tension (read this first)

Candylane's entire purpose is to run arbitrary, machine-altering actions, often elevated,
on demand. "No guardrails on your own machine" is a stated value. So this threat model does
**not** try to prevent code execution or configuration change — that is the product.

It defends a different thing: that every powerful action is **visible, attributable,
reversible, and consent-gated before it runs**, and that Candylane itself never becomes the
attack vector. The adversary we care about is the one who makes your machine do something
**you did not see, did not authorize, and cannot undo** — or who turns the tool that touches
all your machines against you.

## Assets (what we protect, ranked)

| # | Asset | Why it matters |
|---|---|---|
| A1 | Private identity key (Ed25519) | Root of trust. Compromise = impersonate the user, sign malicious biscuits as them. |
| A2 | Chimney contents (SSH/GPG keys, tokens, VPN creds) | Keys to everything else the user owns. |
| A3 | Machine-state integrity (box applied correctly, revert returns to clean) | The core promise. A corrupted undo log makes "revert to vanilla" a lie. |
| A4 | State DB / undo log (`jar`) | Source of truth for reversal. Tamper it, break revert. |
| A5 | Recipe truth (a biscuit does what its recipe says) | The trust contract of the whole marketplace. |
| A6 | `.candy` hamper | A single encrypted file holding an entire setup + secrets. Concentrated value. |
| A7 | Candylane binary + update channel | Touches every machine. The highest-leverage thing to subvert. |

## Trust boundaries

```
                          UNTRUSTED NETWORK
   ┌────────────────────────────────────────────────────────────┐
   │  lanes.candylane.sh    biscuit downloads    self-update feed │
   └───────────────┬────────────────────────────────────────────┘
                   │   TLS + signature verification
                   │   (baked-by identity, pinned release keys)
   ════════════════╪══════════════ TRUST BOUNDARY ════════════════
                   ▼             THE USER'S MACHINE  (user-trusted)
   ┌────────────────────────────────────────────────────────────┐
   │  candylane.exe  ── runs elevated, by explicit user consent  │
   │     │                                                       │
   │     ├─ box (desired state) ──diff──▶ shown to user BEFORE   │
   │     │                                anything runs elevated │
   │     ├─ jar / state.db (undo log)        owner-only          │
   │     ├─ chimney (secrets)                encrypted+owner-only │
   │     └─ post-scripts ── arbitrary code (RCE BY DESIGN)       │
   └────────────────────────────────────────────────────────────┘
                   ▲
   ════════════════╪══════════════ TRUST BOUNDARY ════════════════
                   │   hardware-key unlock (FIDO2)
   ┌───────────────┴────────────────────────────────────────────┐
   │  .candy hamper on USB   (age-encrypted to the hardware key) │
   └────────────────────────────────────────────────────────────┘
```

The three boundaries: **network ↔ machine** (everything crossing it is verified before
trust), **machine ↔ chimney** (secrets are encrypted + owner-only even from other local
processes), and **USB hamper ↔ machine** (sealed, only the hardware key opens it).

## Adversaries

- **ADV-1 Malicious baker** — publishes a biscuit/lane that attacks users. The supply-chain
  attacker. Manifesto line #1 exists for this one.
- **ADV-2 Network MITM** — tampers with biscuits, lanes, or the install/update feed in transit.
- **ADV-3 Local attacker / malware already on the box** — wants the chimney, the identity key,
  or to tamper with the undo log or the Candylane binary.
- **ADV-4 Physical attacker** — steals the USB holding a `.candy` hamper (and maybe the
  hardware key).
- **ADV-5 Compromised Candylane infrastructure** — our own registry or release pipeline is
  breached (supply chain on *us*).
- **ADV-6 The user's own mistake** — applies an untrusted biscuit without reading its recipe,
  or fat-fingers a destructive action.

## Threats and mitigations (STRIDE-tagged)

Status legend: ✅ designed in Phase 1 · 🔜 planned (phase noted) · ⚠️ accepted residual.

| ID | Threat | STRIDE | Adversary | Mitigation | Status |
|----|--------|--------|-----------|------------|--------|
| T1 | A biscuit forges another baker's identity | Spoofing | ADV-1 | Signed manifests; verify **baked-by** against the baker's published key before apply | 🔜 P5/P8 |
| T2 | Biscuit/binary modified in transit | Tampering | ADV-2 | TLS + content hash + signature check against pinned keys; reject on mismatch | 🔜 P5/P8 |
| T3 | Undo log (`jar`) tampered → revert can't return to clean | Tampering | ADV-3 | Owner-only state DB; atomic writes + WAL; integrity hashes on backed-up bytes | ✅ (perms/atomic) / 🔜 (signed log) |
| T4 | Chimney secrets read by another user/process | Info disclosure | ADV-3 | Encrypted at rest (age/rage); owner-only ACL asserted on every load; never inlined; never logged | 🔜 P5 (✅ ACL model) |
| T5 | Private identity key exposure | Info disclosure | ADV-3 | Owner-only ACL set+asserted on every load (`candylane-crypto`, CRITICAL #3); hardware-key option | ✅ (model) / 🔜 P5 (FIDO2) |
| T6 | Malicious post-script runs with the user's elevation | Elevation | ADV-1, ADV-6 | RCE is by design; bounded by: recipe disclosure, **diff before apply**, **taste** in a throwaway VM, signing, undo recipe | 🔜 P8 (✅ diff/undo) |
| T7 | Secret leaks into a shared box/biscuit/hamper | Info disclosure | ADV-6 | Secrets referenced by name only, resolved from chimney at apply; never serialized into shareable artifacts | 🔜 P5 (✅ design rule) |
| T8 | Self-update swaps in a malicious binary | Tampering/Elev | ADV-2, ADV-5 | Verify signature against a **pinned** pubkey before atomic swap; rollback on failed first run | 🔜 P1.5 |
| T9 | Install bootstrap (`iwr \| iex`) MITM'd or spoofed | Tampering | ADV-2 | Prefer signed installer (rustup-init model); publish checksums; TLS; document the risk loudly | 🔜 P0/P1.5 ⚠️ |
| T10 | Our registry/pipeline compromised, ships bad biscuits/releases | Tampering | ADV-5 | Reproducible builds; signed releases; `cargo-audit`/`cargo-deny`; registry kill-switch; federation/mirroring | 🔜 P0/P8 |
| T11 | Stolen USB hamper → full setup + secrets | Info disclosure | ADV-4 | `.candy` age-encrypted to the hardware key; optional expiry; secrets unusable without the key | 🔜 P7 ⚠️ |
| T12 | Crash mid-pull leaves a half-configured, non-reversible machine | Tampering (integrity) | ADV-6/env | Intent-before-apply, in-flight **reconcile** on recover, bounded best-effort rollback (CRITICALs #4/#5) | ✅ (design) |
| T13 | A biscuit's recipe lies about what it does | Spoofing/Repud. | ADV-1 | Recipe is machine-checkable where possible; diff shows real effects; reputation + kill-switch; signed provenance for repudiation | 🔜 P8 |
| T14 | Path traversal / command injection in a handler | Tampering/Elev | ADV-1 | Validate targets; no shell string interpolation; pass args as arrays; `WingetExecutor` seam keeps subprocess construction in one audited place | 🔜 P1 (handlers) |

## Accepted residual risks (the honest part)

A threat model that claims zero residual risk is lying. These are accepted, by design:

- **R1 — A user can compromise themselves.** Apply an unsigned biscuit from an untrusted lane
  without reading the recipe, and you can be owned. Candylane warns, discloses the ingredients,
  and offers a `taste` in a VM, but it will **not** prevent a determined user from running what
  they choose on their own machine. "No guardrails on your own machine" is the value. The
  defense is informed consent, not prohibition.
- **R2 — The hamper concentrates value.** A `.candy` holds an entire setup, including secrets.
  Lose the USB *and* the hardware key and it is catastrophic. Mitigated by hardware-key
  encryption + optional expiry, but the concentration is inherent to the offline feature. Treat
  a hamper like the keys it contains.
- **R3 — Revert is best-effort at the registry edges.** `winget uninstall` leaves PATH/registry/
  shortcut crumbs (`undo_kind=best_effort`). "Functional-clean vanilla" is the honest bar, not
  byte-identical. `diff`/`history` say so. Tightening this rides the Phase 3 debloat engine.
- **R4 — Trust is bootstrapped from our published keys.** If you cannot establish the
  authenticity of Candylane's release/signing keys out-of-band, you are trusting the channel
  you got them from.

## Out of scope

Nation-state physical hardware implants, compromised CPU microcode/firmware, a fully
pre-compromised host OS, and breaking the underlying crypto primitives (Ed25519, age). If the
attacker already owns the kernel, Candylane cannot save you.

## Review triggers

Revisit this document when: signing/verification lands (P5), the lanes registry opens (P8),
the self-update path ships (P1.5), the `.candy` format is finalized (P7), or any new handler
that executes external code is added.
