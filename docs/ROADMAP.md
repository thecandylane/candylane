# Candylane Roadmap

> **Time is what it is. Completion is what matters.**
>
> These phases are a spine, not a schedule. Each ships when it is solid. Each makes the next one possible. You can stop at the end of any Era and still have something genuinely worth having.

---

## The SHAZAM constraints

Every phase honors these. They are the design DNA of the whole project.

1. **One command does everything.** No multi-step setup. `candylane pull <you>` is the whole ritual.
2. **The word is portable.** Your identity travels with you. Same word, same power, any machine.
3. **The lightning strikes fast.** Under ~5 minutes for a basic profile, under ~15 for a maxed one. Slow automation isn't magic.
4. **No half-states.** Operations are atomic. Either you got the power, or you're back where you started. Never stuck halfway.
5. **The wizard is trustworthy.** Identity, signing, and provenance are foundations, not features.

---

## The architecture, in layers

Each phase builds or strengthens one of these layers. You build bottom-up from orchestration, and top-down from identity, until they meet in the middle.

```
THE SHAZAM SURFACE      one word, full power — candylane pull <you>
IDENTITY LAYER          Ed25519 + FIDO2 + signed manifests + anon credentials
PROFILE LAYER           TOML manifests + lanes + post-install + state DB
ORCHESTRATION LAYER     diff / pull / revert / history (transactional engine)
PACKAGE INTEGRATIONS    winget / scoop / custom / source / WSL distros
SYSTEM LAYER            debloat / firewall / DNS / dotfiles / VPN / WSL
TRANSPORT LAYER         git / GitHub / self-hosted / S3 / Syncthing / .candy
RUNTIME LAYER (v3+)     containers / WASM / browser / native, capability-gated
BOOT LAYER (v4+)        bootable USB / OS install / pre-staged profiles
```

---

# ERA I — THE TOOL

*Candylane works for you. SHAZAM is real on one machine. If you stopped here, it was worth it.*

## Phase 0 — Foundation
*Make the project buildable, testable, and trustworthy from commit one.*

- [ ] Register domains (`candylane.sh`, `.io`, `.dev`) + trademark check
- [x] Rust cargo workspace: `candylane-cli`, `candylane-core` (profile folded in per Decision #1), `candylane-crypto`
- [x] CI from day one: build, test, lint, `cargo-audit`, `cargo-deny`
- [x] Reproducible builds: pinned deps (`Cargo.lock`), locked toolchain (`rust-toolchain.toml` @ 1.96.0)
- [x] `SECURITY.md` and `THREAT_MODEL.md` written **before** feature code
- [x] Public manifesto committed (anchors the ethos so it can't drift)
- [ ] Clean-VM test harness (Hyper-V or QEMU, snapshot/restore automation)

**Exit:** `cargo build` ships a signed `.exe` that runs and exits cleanly on a fresh Win 11 VM. It does nothing yet — but the scaffolding is solid.

## Phase 1 — The Skeleton Loop (v0.1)
*SHAZAM works for one hand-built profile on one machine. Ugly but bulletproof.*

- [~] `candylane init` — generate Ed25519 keypair *(works; full `~/.candylane/` setup + Windows ACL pending — Lane E)*
- [x] `candylane pull <local-profile>` — read TOML, apply it *(Linux: dotfile + script; winget exec pending — Lane B)*
- [x] Minimal profile: winget package list + dotfiles (copy-manage, not symlink) + post-script *(parses all three; dotfile + script execute, winget parsed but stubbed)*
- [x] Transactional state DB (SQLite at `~/.candylane/state.db`), every action logged with before/after hashes where possible
- [x] `candylane diff` — show what `pull` would do, without doing it
- [x] `candylane revert` — undo the last pull from the state DB
- [ ] `candylane history` — list past operations *(CLI stub: "not yet implemented")*
- [ ] One hardcoded official profile: `candylane/minimal-dev` *(the TOML exists in the spec + parser tests; not yet shipped as a bundled profile)*

**Progress (2026-06-06):** The cross-platform half is proven on Linux — `pull`/`revert`/`diff` run the
real engine against dotfile + script profiles with a SQLite state DB, and the money test
([`tests/vertical_slice.rs`](../crates/candylane-core/tests/vertical_slice.rs)) does pull → revert →
functional-clean (both undo paths). Remaining for the Phase-1 exit: **WingetHandler** (Lane B, Windows),
`candylane history`, the bundled `minimal-dev` profile, and the **Hyper-V 10x loop** — the actual
acceptance bar. Full gap list: [FOLLOWUPS.md](./FOLLOWUPS.md).

**Exit:** Fresh Win 11 VM → install → `pull` → 5 minutes later it's the machine you wanted → `revert` → back to vanilla. Run this loop **10 times in a row** without a single break. *This is the one phase that must be perfect.*

## Phase 1.5 — Self-Update + Dogfood Lock
*Boring plumbing and a forced reality check.*

- [ ] Self-updating binary: pull signed binary, verify against pinned pubkey, atomic swap, rollback on failed first run
- [ ] Cross-cutting: structured audit log + human summary per run
- [ ] Cross-cutting: colorful output, spinners, `--verbose`, error messages that suggest fixes
- [ ] **Hard rule:** use ONLY Candylane to set up 2–3 of your real machines for one month. No new features until you've lived on it. Track time saved — that's the real acceptance test.

**Exit:** You trust it on your own daily-driver machines. You have data on how much time it saves.

## Phase 2 — The Real Profile Format (v0.2)
*Profiles express the full power-user setup you actually want.*

- [ ] Full TOML spec: targets, packages, debloat tier, firewall, DNS, dotfiles, post-install, network, dependencies
- [ ] Profile validation with helpful errors (catch typos before damage)
- [ ] Profile inheritance (`extends = "base-dev"`)
- [ ] Lockfile (`candylane.lock.toml`) pinning exact versions
- [ ] `candylane validate <profile>`
- [ ] 5 official profiles: `dev-fullstack`, `redteam-wsl`, `blueteam-defender`, `debloat-extreme`, `minimal-base`

**Exit:** Any setup from your real machines can be expressed as a profile and pulled cleanly.

## Phase 3 — Debloat & Hardening Module (v0.3)
*The Windows transformation that makes the demo video sell itself.*

- [ ] Debloat engine as a first-class subsystem (not a bag of scripts) — modeled on winutil/privacy.sexy data-driven design
- [ ] Tiers: `minimal` / `moderate` / `extreme` / `nuclear`, plus per-component toggles
- [ ] Telemetry kill, Copilot/AI removal (handle provisioned apps so they don't reinstall on update)
- [ ] Edge / Bing / Cortana / OneDrive removal with rollback
- [ ] Firewall hardening profile
- [ ] DNS provider switch (Mullvad / AdGuard / NextDNS / custom)
- [ ] Identifier wipe (machine name, advertising ID, telemetry IDs — where MS allows)
- [ ] Update strategy options: pause / defer / allow / manual
- [ ] Every change tagged `reversible | best-effort | one-way`; `diff` surfaces one-way changes loudly

**Exit:** Fresh Win 11 goes from telemetry-spewing to silent in one command — proven with before/after network captures.

## Phase 4 — WSL2 First-Class Citizen (v0.4)
*The Linux side as smooth as the Windows side.*

- [ ] `wsl` block supports any distro (Ubuntu, Kali, Debian, NixOS-WSL, Arch-WSL)
- [ ] Nested Candylane: a profile applies a Candylane profile *inside* WSL
- [ ] Shared identity host ↔ WSL (SSH/GPG keys propagate)
- [ ] Shared DNS + optional shared VPN tunnel

**Exit:** `candylane pull dev-laptop` produces a configured Windows host AND a working Kali/Ubuntu WSL2 — dotfiles, keys, tools in both.

---

# ERA II — THE TRUST

*Candylane works across all your machines, online or off. Now it's unlike anything else.*

## Phase 5 — Identity & Secrets Vault (v0.5)
- [ ] Hardware key support (FIDO2, via `age-plugin-fido2` or equivalent)
- [ ] Encrypted vault subsystem, separate from profiles
- [ ] Secret references in profiles (`ssh_key = "vault:home-server"`)
- [ ] SSH / GPG / API token / VPN credential management
- [ ] Identity import/export (move your identity to a new machine securely)
- [ ] Signed profile manifests + `candylane verify <profile>`

**Exit:** Secrets never live in plaintext. Identity is portable. A new machine claims your identity only with your hardware key.

## Phase 6 — VPN & Network Mesh Bootstrap (v0.6)
- [ ] WireGuard config bundling and import
- [ ] Tailscale auth key management (vault-encrypted, consumed on apply)
- [ ] Headscale (self-hosted) support
- [ ] Network templates: home-only / home+lab / everywhere-I-trust
- [ ] Split-horizon DNS through the mesh
- [ ] Firewall rules that follow network topology

**Exit:** New laptop in a coffee shop → `pull` → you can SSH your homelab. It joined your network as part of becoming itself.

## Phase 7 — Offline Bundles (.candy) (v0.7)
*SHAZAM without internet. The field-deployment killer feature.*

- [ ] `.candy` format: age-encrypted tarball — manifest + binaries + dotfiles + secrets
- [ ] Bundle includes the Candylane binary itself (true bootstrap-from-USB)
- [ ] `candylane bundle <profile> --offline` and `candylane restore <bundle>` (zero network)
- [ ] Bundle signing + verification
- [ ] Delta bundles (ship only what changed)
- [ ] Optional bundle expiration (for shared sensitive bundles)

**Exit:** Pre-stage a `.candy` on USB. Fresh laptop in a Faraday cage. Plug in. One command. Walk out with a fully configured machine. No internet was harmed.

---

# ERA III — THE COMMUNITY

*Other people use Candylane. The lane ecosystem emerges. AI lowers the floor.*

## Phase 8 — Lanes Marketplace (v0.8)
- [ ] Public registry at `lanes.candylane.sh` (federated/mirrorable)
- [ ] Discovery: `candylane lane search <keyword>`
- [ ] Publisher identity: lanes signed by published keys
- [ ] Reputation surface (user count, publisher track record)
- [ ] Abuse policy enforcement + kill-switch for known-bad lanes
- [ ] `candylane try <lane> --ephemeral` — test in a disposable VM/container first
- [ ] Lane diffing + update notifications

**Exit:** A stranger publishes a lane. You can find it, audit it, test it safely, apply it, revert it — and publish your own.

## Phase 9 — Linux Support (v0.9)
- [ ] Backends: apt, dnf, pacman, nix, flatpak
- [ ] systemd units, kernel params, security hardening
- [ ] Distro detection + per-distro targets
- [ ] WSL profiles run natively (`redteam-wsl` → `redteam`)
- [ ] Headless server profiles for VPS / homelab nodes
- [ ] ARM support (Raspberry Pi, Apple Silicon as Linux target)

**Exit:** Same profile concept, same `pull`, working on Ubuntu, Debian, Fedora, and Arch.

## Phase 10 — The AI Layer (v1.0)
*Lower the floor without lowering the ceiling. AI assists; it never gatekeeps.*

- [ ] `candylane ask "<plain-language description>"` → generated profile draft for review
- [ ] `candylane explain <profile>` → plain-English summary of what it will do (huge for trust)
- [ ] Local-first models (Ollama / llama.cpp / MLX) as default
- [ ] Optional cloud models with explicit per-operation consent
- [ ] AI-assisted failure debugging on a failed `pull`
- [ ] AI-assisted lane authoring
- [ ] **Privacy rule:** profile contents never leave the machine without explicit consent

**Exit:** A non-expert describes what they want in plain English and ends up with a working profile. **This is the v1.0 milestone — the version you'd market.**

---

# ERA IV — THE FRONTIER

*Bare metal, sovereign identity, sandboxed runtimes. Redefining the personal computing environment.*

## Phase 11 — Bootable Installer (v2.0)
*SHAZAM on bare metal. No prior OS required.*

- [ ] `candylane forge-usb` — bootable USB generation
- [ ] Two modes: "live" (leave no trace) and "install" (wipe + install OS + apply profile)
- [ ] Custom Windows image (WIM/DISM + Windows ADK; study CTT MicroWin)
- [ ] Custom Linux image (debootstrap / archiso / NixOS image)
- [ ] Pre-staged profile baked into the USB
- [ ] Hardware-key unlock at boot
- [ ] Optional encrypted-disk install (LUKS / BitLocker keyed to hardware key)
- [ ] Multi-ISO via Ventoy integration

**Exit:** Plug USB into freshly assembled PC. Boot. Tap hardware key. Walk away. Return to a fully configured machine — OS installed, debloated, your tools, your keys, joined to your network. No keyboard beyond the key tap.

## Phase 12 — The Sovereign Tier (v3.0+)
*Optional advanced capabilities. Each becomes its own mini-project once v1.0 users tell you which matters most.*

- [ ] Capability-sandboxed app runtime (any app/container/WASM with declared, OS-enforced capabilities)
- [ ] Anonymous verified identity (BBS+ / anonymous credentials — prove things without revealing yourself)
- [ ] Hypervisor isolation tier (Qubes-style VM separation, optional mode)
- [ ] Mesh-native profile sync (updates propagate through your mesh, no central server)
- [ ] Reproducible builds for everything (Nix-grade determinism, without Nix-grade pain)
- [ ] Air-gap multi-machine choreography (bundle a whole homelab into one sealed delivery)

---

## The four milestones to be proud of

- **End of Era I:** "I built the tool I always wanted. It saves me hours every week."
- **End of Era II:** "This is genuinely unlike anything else that exists."
- **End of Era III:** "This is a movement. Other people build on it."
- **End of Era IV:** "We redefined what a personal computing environment is."

Each is a real, defensible place to stand. Build toward the next only when the current one is solid.

---

## Notes on the hard parts (so future-you isn't surprised)

- **Reversibility debt is real.** Some Windows changes (driver installs, cascading registry edits, removed provisioned apps) can't be cleanly undone. Be honest: tag them `one-way`, warn loudly, never pretend.
- **Rust + Windows interop will need FFI and embedded PowerShell.** The pure-Rust dream dies on contact with the registry, services, DISM, and the WSL API. Plan for `windows-rs` crate usage and audited PS1 subprocess calls early.
- **The test matrix grows fast.** Win 10 vs 11, Home/Pro/Enterprise/LTSC, hardware variety, Windows Update fighting back. Snapshot-based VM testing is non-negotiable, and flaky tests will be your tax.
- **Phase 1 is the keystone.** Every later phase can ship rough and iterate. Phase 1 cannot. If `pull` and `revert` aren't surgical, you'll paper over bugs forever.
- **Resist jumping to Phase 7 or 11.** They're the sexy ones. They only feel magical because the boring foundation underneath is solid. Build in order.
