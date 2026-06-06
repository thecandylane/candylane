# Candylane References & Bibliography

> The shoulders we stand on. Every project here taught us something — an architecture pattern, a tradeoff, a mistake to avoid, or a piece we can integrate directly. This is our research lab in document form.
>
> **How to use this:** clone the Tier 1–2 repos into `~/code/candylane/refs/`, keep a `NOTES.md` per repo recording what you took from it conceptually. When working with a coding agent, point it at *specific files* in these repos rather than describing patterns abstractly — grounded agents are dramatically better.
>
> Legend: 🔬 study deeply · 🔌 integrate directly · 📚 reference when needed · ⚠️ study what *not* to do

---

## Tier 1 — Direct Conceptual Cousins
*The closest things to Candylane that already exist. Understand their tradeoffs before making your own.*

| Repo | Why it matters |
|---|---|
| 🔬 `HeyPuter/puter` | The browser-OS that started this whole line of thinking. Plugin architecture & app sandboxing patterns. |
| 🔬 `NixOS/nix` + `NixOS/nixpkgs` | Gold standard for declarative reproducibility. Steal concepts: generations, atomic upgrades, lockfiles, derivation graphs. Read the Nix Pills. |
| 🔬 `Homebrew/brew` | The brand & ergonomics target. Study formulae DSL, Cellar/Caskroom split, taps (third-party repos), and `brew bundle` (declare a full machine). |
| 🔌 `microsoft/winget-cli` | Your primary packaging integration. Manifest schema, source/repository model, installer-type handling (MSI/EXE/MSIX). |
| 🔌 `ScoopInstaller/Scoop` | Userland Windows packages. "No admin needed" install patterns; `buckets` (their version of lanes). |
| 🔬 `jetify-com/devbox` | Nix made approachable. JSON schema, project-vs-global pattern, bridging power to mortals. |

---

## Tier 2 — Debloat & Hardening Foundation
*Phase 3, gift-wrapped. These do the Windows surgery you'd otherwise reinvent.*

| Repo | Why it matters |
|---|---|
| 🔬🔌 `ChrisTitusTech/winutil` | Most popular Windows utility, active 2026. Modular sources compiled to one PS1, JSON-driven presets, **hard rule that every tweak ships an undo**. Mirror this architecture. Also home to MicroWin (Phase 11). |
| 🔬🔌 `undergroundwires/privacy.sexy` | YAML-driven, declarative, reversible privacy scripts (Win/Mac/Linux). The data model you want. |
| 🔬🔌 `SubconsciousCompute/privacy-sexy-rs` | A **Rust** port of privacy.sexy. Proves your language choice; shows how to consume YAML script definitions in Rust with echo/run/revert subcommands. |
| 🔬 `HotCakeX/Harden-Windows-Security` | MVP-grade Windows hardening, structured by security domain. Reference for tweaks beyond privacy. |
| 🔬 `farag2/Sophia-Script-for-Windows` | Alternative debloater with strong reversibility patterns and excellent per-tweak documentation. |
| ⚠️ `Sycnex/Windows10Debloater` | Older. Study what *not* to do — script-only, no rollback, no state tracking. |

---

## Tier 3 — Dotfiles, Config Sync & Identity
*Phases 1, 2, and 5.*

| Repo | Why it matters |
|---|---|
| 🔬 `twpayne/chezmoi` | The best dotfile manager that exists (Go). Study the state model (`diff`/`apply`), templating, and secret handling via age/gpg/keepass. **One of the three to read end-to-end.** |
| 📚 `TheLocehiliosan/yadm` | Git-based dotfiles with encryption. Simpler contrast to chezmoi. |
| 🔬🔌 `FiloSottile/age` | The file encryption for `.candy` bundles. Read the spec + Go reference. |
| 🔌 `str4d/rage` | The **Rust** port of age. This is what you integrate. Clean, well-tested. |
| 📚 `Yubico/yubikey-manager` | How to talk to YubiKeys properly, even if you target generic FIDO2. |
| 📚 `Mic92/ssh-to-age` | Convert SSH keys to age keys — useful identity-portability pattern. |
| 🔬 `sigstore/cosign` | Modern, developer-friendly artifact signing. Reference for signed profiles/lanes/binaries. |

---

## Tier 4 — CLI & TUI Excellence
*If the tool is a chore to type into, no one uses it.*

| Repo | Why it matters |
|---|---|
| 🔬 `starship/starship` | Themed prompt in Rust. Config model, color, cross-platform terminal handling. |
| 🔬 `jesseduffield/lazygit` + `extrawurst/gitui` | TUI excellence. Reference for `candylane status` / `history` feeling good. |
| 🔌 `ratatui-org/ratatui` | Rust TUI framework (pick this over Go's bubbletea for stack consistency). |
| 📚 `charmbracelet/bubbletea` | The Go TUI gold standard — worth studying even if you don't use it. |
| 🔌 `clap-rs/clap` | Your Rust CLI parser. Study how `cargo` itself uses it. |
| 🔌 `console-rs/indicatif` | Progress bars & spinners. Power users judge you on these. |
| 🔬 `cli/cli` (gh) | GitHub's CLI (Go). Subcommand structure + auth flow. |
| 🔬 `jdx/mise` | Version manager done right (Rust). CLI ergonomics + multi-tool orchestration. **One of the three to read end-to-end.** |

---

## Tier 5 — System Plumbing in Rust on Windows
*Your Rust + Windows reality.*

| Repo | Why it matters |
|---|---|
| 🔌📚 `microsoft/windows-rs` | The official Windows API crate. You will live here. |
| 📚 `PowerShell/PowerShell` | Understand how MS structures PS modules (you'll invoke them, not fork). |
| 🔬 `rust-lang/rustup` | The `rustup-init.exe` installer model is exactly what `candylane.exe` bootstrap should resemble. |
| 📚 `uutils/coreutils` | Rust coreutils port — cross-platform abstraction patterns. |
| 📚 `tauri-apps/tauri` | Modern Rust GUI answer, if you build one for the AI layer (Phase 10). |
| 📚 `oxidecomputer/omicron` | Large-scale Rust systems code, for architecture taste. |

---

## Tier 6 — State, Transactions & Reversibility
*The hardest part of Phase 1.*

| Repo | Why it matters |
|---|---|
| 🔬 `hashicorp/terraform` | `terraform plan` = your `candylane diff`. State file model is directly applicable. |
| 🔬 `pulumi/pulumi` | Another IaC state model worth comparing. |
| ⚠️🔬 `ansible/ansible` | The tool you're replacing — but their idempotency model (what "already done" means per module) is well thought out. |
| 🔌 `rusqlite/rusqlite` | Your state DB layer. Boring, standard, perfect. |
| 🔌 `untitaker/rust-atomicwrites` | Atomic file ops — how you avoid corrupted state mid-transaction. |

---

## Tier 7 — Bootable / OS Imaging
*Phase 11. Bookmark; don't touch yet.*

| Repo | Why it matters |
|---|---|
| 🔬 `ventoy/Ventoy` | Multi-ISO bootable USB — the architecture for the bootable installer. |
| 📚 `pbatard/rufus` | Windows ISO writer (C). Understand the mechanics. |
| 📚 CTT MicroWin (in `ChrisTitusTech/winutil`) | Debloated Windows ISO generation — the exact Phase 11 reference. |
| 🔬 `siderolabs/talos` | Minimal immutable Linux. Host-OS-as-API pattern → Phase 12 hypervisor tier. |
| 📚 `siderolabs/omni` | Talos at scale — minimal-OS tooling structure. |

---

## Tier 8 — Networking & Mesh
*Phase 6.*

| Repo | Why it matters |
|---|---|
| 🔬 `tailscale/tailscale` | Readable Go source. ACL + key management are reference-quality. |
| 🔌 `juanfont/headscale` | Self-hosted Tailscale control plane. Critical for the "no SaaS" user. |
| 🔬 `WireGuard/wireguard-tools` | Config format + how `wg-quick` orchestrates (mostly C). |
| 📚 `zerotier/ZeroTierOne` | Alternative mesh — a Candylane network option. |

---

## Tier 9 — Security Tooling (Lanes You'll Publish)
*Phase 2 curated profiles & Phase 8 lanes. Know the canon.*

| Repo | Role |
|---|---|
| 📚 `BloodHoundAD/BloodHound` | AD attack-path analysis |
| 📚 `BishopFox/sliver` | Modern C2 — the OSS Cobalt Strike replacement |
| 📚 `MythicAgents/Mythic` | Multiplayer C2 framework |
| 📚 `HavocFramework/Havoc` | Newer post-exploitation framework |
| 🔌 `redcanaryco/atomic-red-team` | Purple-team gold. Every "security lab" lane should pull this. |
| 🔌 `wazuh/wazuh` | Open SIEM for the `blueteam-defender` profile |
| 🔌 `Velocidex/velociraptor` | DFIR / endpoint visibility |
| 📚 `enaqx/awesome-pentest` | Curated tool list to mine for lane contents |

---

## Tier 10 — AI Layer
*Phase 10. Keep the integration simple.*

| Repo | Why it matters |
|---|---|
| 🔌 `ollama/ollama` | Local LLM runtime — your default `candylane ask` backend |
| 📚 `ggml-org/llama.cpp` | The engine under Ollama — understand the layer below |
| 🔬 `Mozilla-Ocho/llamafile` | Single-file LLM distributable — possibly the right shape for a bundled local model |
| 📚 `langchain-ai/langgraph` | Multi-step AI workflows, *if* lane generation ever needs them. Don't over-engineer here. |

---

## Prior Art & Reading (not repos)

- **Local-first software** — Ink & Switch's essay. The philosophical backbone for "your data, your keys, sync-optional."
- **The Nix Pills** — the gentlest path into the Nix mental model.
- **MITRE ATT&CK / D3FEND** — the maps of attack and defense. Frame security lanes against these.
- **OWASP Top 10 for LLM Applications** — required reading before shipping the AI layer.
- **W3C DIDs & Verifiable Credentials** — standards for the identity and (eventual) anonymous-credential work.
- **arkenfox/user.js** — the canonical hardened Firefox config your `firefox = "hardened"` profile should build on.

---

## The three to read end-to-end before Phase 1 code

1. **`twpayne/chezmoi`** — for the state / diff / apply model
2. **`SubconsciousCompute/privacy-sexy-rs`** — for the YAML-driven, reversible script pattern *in Rust*
3. **`jdx/mise`** — for CLI ergonomics and multi-tool orchestration

One week of careful reading here saves three months of meandering.

---

*This bibliography grows as the project does. When you take something from a new source, add it here. The record of what you learned from is part of what makes the project trustworthy — and it's how you'll onboard the next contributor.*
