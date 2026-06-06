# Candylane Vocabulary

The sweet-shop lexicon. Rule: **theme the nouns that are ours; keep verbs and security
ops plain.** That's Homebrew's lesson (formula/cask/tap are themed; install/upgrade are
not). Whimsy where it delights, plain words where people need to trust or type fast.

## The containment hierarchy

```
box  ⊃  tin  ⊃  biscuit
(your machine)   (a bundle)   (one packaged thing)
```

A **box** holds **tins** and loose **biscuits**. Bakers share them on **lanes**.

## Terms

| Term | What it is | Code / technical name |
|---|---|---|
| **biscuit** | the atom: one package + its config + its undo recipe, signed, with a recipe card | a named, sealed group of Actions (Phase 2/8) |
| **tin** | a curated bundle of biscuits ("the redteam tin", "the homelab tin") | bundle manifest (Phase 8) |
| **box** | your whole machine, the way you like it — the desired state, portable + shareable | `Profile` struct (NOT `Box` — std conflict) |
| **lane** | a registry/repo of biscuits & tins (a street of bakers) | tap / bucket equivalent (Phase 8) |
| **jar** | this machine's local store: what's actually installed + tracked right now | `~/.candylane/` + `state.db` (the Cellar) |
| **recipe** / **ingredients** | a biscuit's manifest + disclosure: what it installs, what it touches, does it phone home, its undo honesty | the label / manifest |
| **baker** | someone who shares biscuits; the community | signed-by identity |
| **baked by** + sealed | the signature + provenance on a biscuit | `candylane-crypto` |
| **bake** | author a biscuit | `candylane bake` (Phase 8) |
| **taste** | try a biscuit or lane in a throwaway VM before it touches your real machine | `candylane try --ephemeral` (Phase 8) |
| **chimney** | your encrypted secrets store (SSH/GPG keys, tokens, VPN creds) | secrets subsystem (Phase 5) |
| **.candy** (a hamper) | a sealed, encrypted, offline edition of your box | offline bundle (Phase 7) |

## box vs jar — not the same thing

- **box** = what you WANT (the declaration; shareable; portable to any machine).
- **jar** = what's actually ON this machine (the state).
- `candylane diff` = the gap between your **box** and this machine's **jar**.

## chimney — read this before relying on the name

"Chimney" doesn't broadcast "secrets here" the way "vault" does. That's a free bit of
theme and mild defense against dumb string-scrapers. It is **not** the protection.

The chimney is safe because it is:
- encrypted at rest (age/rage),
- owner-only (`candylane-crypto` ACL, asserted on every load),
- never written into a box or biscuit (secrets are referenced by name, never inlined),
- never committed (`.gitignore` blocks `*.key`, `.candylane/`).

An attacker already on the machine finds a `chimney` as easily as a `vault`. The lock is
the crypto. The name just doesn't shout. **Never let the cozy name substitute for the
real locks.**

## Stays plain (never themed)

- **Verbs:** `pull`, `diff`, `revert`, `recover`, `history`. Clear beats cute when typing.
- **Security primitives:** sign, verify, encrypt. Gravity, not whimsy.
