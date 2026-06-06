# 🍭 Candylane

**Rebuild your whole Windows machine — debloated, hardened, tooled, and yours — from a single profile. Online or off.**

One word, full power, any machine.

---

## What it is

The layer between a fresh Windows install and the machine you actually work on.

You build your setup once, on a machine you trust — packages, dotfiles, WSL, debloat and hardening, DNS and firewall, keys and network. That becomes a **box**: your whole machine, the way you like it, in one portable file. On any other machine, one command makes the box real, and one command puts it back exactly how it was.

Secrets — SSH and GPG keys, tokens, VPN credentials — live in your **chimney**, an encrypted store referenced by name and never written into a box. Candylane wraps `winget` and `scoop` instead of reinventing them; the effort goes into everything *around* the packages: the transactional pull/revert engine, debloat, WSL, identity, and offline editions.

No telemetry. No account. No central server. Your machine is yours.

## How it works

Compute a plan, apply it action by action, record an undo recipe for each. `revert` replays the recipes in reverse. Success is read from the system, never assumed from an exit code — so a pull either lands completely or rolls back to clean. `diff` shows the gap between your box and what's actually on the machine (its **jar**), and nothing touches anything before it does.

A box is just TOML:

```toml
name = "minimal-dev"

[packages.winget]
install = ["Git.Git", "Microsoft.VisualStudioCode", "BurntSushi.ripgrep.MSVC"]

[[dotfiles.file]]
src    = "./home/.gitconfig"
target = "$HOME/.gitconfig"

[[post_install]]
run  = "./scripts/setup.ps1"
undo = "./scripts/setup.undo.ps1"   # a paired undo keeps revert honest
```

Dotfiles are copy-managed — the original bytes are backed up and restored on revert, no symlink sprawl. Every action is tagged for how reversible it is, and `diff` and `history` say so plainly. Candylane never claims more reversibility than it can deliver.

## Commands

```
candylane diff <box>     show what a pull would change, before it changes anything
candylane pull <box>     apply a profile
candylane revert             roll back the last pull
candylane recover            reconcile and roll back an interrupted pull
candylane init               generate your identity
```

More arrive with the roadmap: **.candy** hampers (sealed, offline editions of your box), **lanes** where bakers share **biscuits** (a package with its config and undo) and **tins** (curated bundles), and local AI you can ask to draft or explain a box.

## Install

Pre-alpha — build from source:

```bash
git clone https://github.com/candylane/candylane && cd candylane
cargo build --release
```

The one-word install lands with the first release:

```powershell
iwr -useb candylane.sh/win | iex
```

## Status

Pre-alpha, built in the open. Working today: the engine, the dotfile and script handlers, and the full `pull` → `revert` loop, proven on Linux. In progress: the Windows half — winget, debloat, hardening, the owner-only key ACL — and everything past Phase 1.

- Architecture & decisions — [docs/PHASE1_ARCHITECTURE.md](./docs/PHASE1_ARCHITECTURE.md)
- Where it's going — [docs/ROADMAP.md](./docs/ROADMAP.md)
- What's left — [docs/FOLLOWUPS.md](./docs/FOLLOWUPS.md)
- Why it exists, and the one line we won't cross — [docs/MANIFESTO.md](./docs/MANIFESTO.md)
- The lexicon — box, chimney, jar, lane, tin, biscuit — [docs/VOCABULARY.md](./docs/VOCABULARY.md)

## License

Not yet chosen.
