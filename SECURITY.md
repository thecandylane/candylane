# Security Policy

Candylane is a tool that, by design, reconfigures your whole machine and runs code with
elevated privileges. We take that responsibility seriously. This document covers how to
report issues and what our security posture is. For the full analysis of assets,
adversaries, and mitigations, see [THREAT_MODEL.md](./docs/THREAT_MODEL.md).

## Status: pre-alpha — read this first

Candylane is **pre-alpha and unaudited.** Do not run it on a machine you cannot afford to
wipe and rebuild. There are no security guarantees yet. We are publishing the security
model early *because* the design is the point — we want it reviewed before there is a user
base to put at risk.

## Reporting a vulnerability

**Do not open a public issue for a security vulnerability.**

- Preferred: GitHub **private security advisory** (`Security → Report a vulnerability`) once
  the repo is public.
- Email: `security@candylane.sh` *(planned — not yet live in pre-alpha; use the GitHub
  advisory until it is)*.
- Encrypt sensitive reports to the project's published age/PGP key *(planned)*.

Please include: affected component, version/commit, a reproduction, and the impact you
believe it has. If you have a fix, a patch is welcome but not required.

### What to expect
- Acknowledgment target: 72 hours.
- Triage + severity assessment: 7 days.
- Coordinated disclosure: we will agree a timeline with you, default 90 days or on fix
  release, whichever is first. Credit is given unless you ask otherwise.

## Supported versions

| Version | Supported |
|---|---|
| `main` (pre-alpha) | Latest commit only. No backports. |
| tagged releases | None yet. |

Until 1.0, only the latest `main` receives security fixes.

## Scope

**In scope** (we want these reports):
- The transaction/revert engine leaving a machine in a corrupt or non-reversible state.
- Chimney (secrets) exposure: secrets readable by another user/process, written to disk
  in plaintext, leaked into logs, a box, a biscuit, or a `.candy` hamper.
- Private identity key exposure or weak permissions.
- Signature/verification bypass: applying a tampered or unsigned biscuit/binary as if trusted.
- Supply-chain: a path by which a malicious biscuit, lane, or release reaches a user.
- The install bootstrap and self-update path.
- Path traversal, command injection, or privilege issues in handlers (winget/dotfile/script).

**Out of scope:**
- A user knowingly running an untrusted biscuit on their own machine after being shown its
  recipe and warnings. That is a feature, not a bug (see "Security philosophy" below).
- Effects of `one_way` actions that the tool warned about before applying.
- Social engineering of a user into disabling protections.
- Physical hardware implants, compromised CPU microcode, or a fully compromised host OS.

## Security philosophy

Candylane's purpose is to run powerful, machine-altering actions on demand, often elevated.
So our model is **not** "prevent code execution" — that is the product. Our model is:

> **Make every powerful action visible, attributable, reversible, and consent-gated before
> it runs.**

The four pillars:
1. **Visible** — `diff` shows exactly what a `pull` will do before anything runs with admin.
   Biscuits ship a **recipe** (ingredients: what they install and touch).
2. **Attributable** — biscuits are **baked-by** a signed identity; provenance is verifiable.
3. **Reversible** — every action records an undo recipe; `revert` returns to functional-clean.
   Irreversible (`one_way`) actions are flagged loudly before they run.
4. **Trial-able** — **taste** a biscuit in a throwaway VM before it touches your real machine.

## The registry line (what we will not host)

The public lane registry hosts almost anything — offensive tooling included. The only
exclusions, the same line GitHub holds:
1. Biscuits designed to attack Candylane users (supply-chain, credential theft).
2. Biscuits that exfiltrate user data without explicit disclosure in their recipe.
3. Content illegal in essentially every jurisdiction.

Known-bad biscuits are subject to a registry kill-switch (Phase 8).

## Cryptography & supply chain

- **Identity:** Ed25519 (`candylane-crypto`), generated locally, never leaves the machine
  without explicit export. Private key stored owner-only, asserted on every load.
- **Secrets (chimney):** encrypted at rest (age/rage), referenced by name, never inlined
  into a box or biscuit, never committed.
- **Provenance:** biscuit/lane manifests and release binaries are signed; clients verify
  against pinned keys before trust.
- **Builds:** pinned Rust toolchain ([rust-toolchain.toml](./rust-toolchain.toml)),
  `cargo-audit` + `cargo-deny` in CI, reproducible builds as a goal, signed releases.

## Hardening this tool against itself

Candylane touches every machine you own. That is exactly why it must never become an attack
vector. The state DB, the chimney, and the identity key are owner-only. The self-update path
verifies signatures against a pinned key before swapping the binary. We hold this without
exception — see [MANIFESTO.md](./docs/MANIFESTO.md).
