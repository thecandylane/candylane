# 🍭 Candylane

**The power user's bootstrap layer for Windows. Build your perfect machine once. Pull it anywhere.**

```powershell
iwr -useb candylane.sh/win | iex
```

One word. Full power. Anywhere.

---

## The SHAZAM Principle

In the movie, a kid says one word and gains all the power and wisdom of those before him. That's Candylane.

Build your perfect setup once — on a laptop you trust. Then say the magic word on any other machine, and the lightning strikes: your tools, your keys, your hardened config, your network, your whole environment, exactly the way you like it. Online or offline. Fresh metal or hand-me-down ThinkPad. One command. A few minutes. Done.

Every design decision in Candylane is tested against one question: **does this honor SHAZAM?** One word, full power, anywhere.

---

## What is Candylane?

Candylane is the missing layer between a fresh Windows install and the machine you actually want to use.

You know the drill. New laptop, new VM, new workstation — and three hours of debloating, hardening, installing tools, configuring SSH, setting up WSL, hooking into your VPN, syncing dotfiles, removing telemetry, and screaming at Cortana before you can do any real work.

Candylane does it once, then does it everywhere. Build your profile on a machine you trust. Pull it onto any other Windows box with one command. Online, offline, air-gapped, fresh metal — same setup, same tools, same identity, every time.

**Built for devs, security pros, homelabbers, and anyone who lives in computers.**
**No telemetry. No accounts required. No editorial gatekeeping. No guardrails.**

---

## Who Candylane is for

- You set up a lot of machines and you're tired of it.
- You want your Windows debloated, hardened, and stripped of Microsoft's AI bloat the same way every time.
- You live in WSL2 and want it bootstrapped properly from minute one.
- You do offensive or defensive security work and want your toolkit portable.
- You have a homelab and want every new node to phone home automatically.
- You believe the same things we do: defenders need offensive knowledge, open tooling makes the whole ecosystem safer, and your machine is your machine.

If "no guardrails" makes you nervous, Candylane isn't for you. If it makes you exhale, welcome home.

---

## The 60-second pitch

```powershell
# On a fresh Windows install
iwr -useb candylane.sh/win | iex

# Set up your identity
candylane init

# See exactly what's about to happen
candylane diff my-dev-laptop

# Pull your setup
candylane pull my-dev-laptop

# Reboot. Your machine is ready.
```

In that minute, Candylane has:

- Removed Microsoft telemetry, Copilot, Cortana, Edge, Bing, OneDrive, and dozens of background services
- Hardened the OS, swapped DNS to your chosen provider, locked down the firewall
- Installed your full dev stack: git, gh, neovim, VS Code, your terminal, your shell, your fonts
- Bootstrapped WSL2 with your chosen distro and your full Linux toolchain
- Installed your security stack: nmap, burp, ffuf, sliver client, hashcat — whatever you packed
- Restored your SSH keys, GPG keys, and signed identity (decrypted with your hardware key)
- Connected to your Tailscale net and your WireGuard tunnels
- Wiped every identifier the OS will let us touch

And if you don't like the result, `candylane revert` puts you back where you started.

---

## Core commands

```
candylane init              Generate your identity, set up the config dir
candylane install <pkg>     Install a single tool (wraps winget / scoop)
candylane diff <profile>    Show exactly what a pull would change — before it changes
candylane pull <profile>    Apply a full machine profile
candylane revert            Roll back the last operation
candylane history           Show everything Candylane has done on this machine
candylane status            Validate the machine state
candylane doctor            Diagnose problems and suggest fixes
```

Later phases add: `bundle` / `restore` (offline), `lane` (marketplace), `vault` (secrets), `sync` (cross-machine), `ask` / `explain` (AI). See the [Roadmap](./ROADMAP.md).

**Everything has a `--dry-run`. Everything is logged. Nothing touches your machine without showing you what it will do first.**

---

## Profiles

A profile is a git repo (or signed tarball) that describes a complete machine personality.

```toml
# candylane.profile.toml
name = "dev-laptop"
version = "2026.06"
extends = "minimal-base"
signed_by = "did:key:z6Mk..."

[targets.windows]
debloat = "extreme"             # telemetry, Copilot, Edge, Bing, OneDrive...
dns = ["mullvad"]               # privacy-focused resolver
firewall = "hardened"
identifiers = "wipe"            # strip machine name, advertising ID, telemetry IDs

[packages.winget]
install = [
  "Git.Git", "GitHub.cli", "Microsoft.VisualStudioCode",
  "Neovim.Neovim", "wez.wezterm", "junegunn.fzf",
  "BurntSushi.ripgrep.MSVC", "sharkdp.bat", "jesseduffield.lazygit",
  "Mozilla.Firefox", "Brave.Brave", "Google.Chrome"
]

[browsers]
firefox = "hardened"            # arkenfox user.js applied
chrome  = "barebones-dev"
brave   = "dev-defaults"

[wsl]
distro  = "kali-rolling"
profile = "redteam-wsl"         # a nested Candylane profile, applied inside WSL

[network]
tailscale = "vault:tailscale-auth"
wireguard = ["vault:home-tunnel", "vault:lab-tunnel"]

[identity]
ssh_keys = "vault:ssh-bundle"
gpg_keys = "vault:gpg-bundle"
git_config = { name = "...", email = "...", signing_key = "..." }

[dotfiles]
source   = "git@github.com:you/dotfiles"
target   = "$HOME"
strategy = "symlink"

[post_install]
scripts = [
  "./scripts/firefox-harden.ps1",
  "./scripts/extra-tweaks.ps1"
]
```

Profiles support inheritance (`extends`), version pinning (`candylane.lock.toml`), and signing. Pulling someone else's profile shows you who signed it and what it will do — before it touches your machine. **Secrets are never stored in profiles**; they live in your encrypted vault and are referenced by name.

---

## The offline workflow (the killer feature)

You're heading to a site with no connectivity. You'll set up a fresh laptop when you get there.

**At home, on a trusted machine:**

```powershell
candylane bundle my-dev-laptop --offline --output ./mybundle.candy
```

You get a single `.candy` file — every package binary, every dotfile, every config, every VPN credential — encrypted to your hardware key. Drop it on a USB stick.

**At the site, on the new machine, no internet required:**

```powershell
candylane restore .\mybundle.candy
```

Tap your hardware key. Walk away. Come back to a fully configured machine, VPN tunnels staged, ready to connect into *your* network the moment it finds one.

Nobody else does this well. This is where Candylane stops being "a nicer winget wrapper" and becomes the only tool of its kind.

---

## How Candylane compares

| Capability | winget | Scoop | Chocolatey | Ansible | Nix / NixOS | **Candylane** |
|---|---|---|---|---|---|---|
| Windows-first | ✅ | ✅ | ✅ | ⚠️ painful | ❌ WSL only | ✅ |
| Single-command full machine setup | ❌ | ❌ | ⚠️ | ⚠️ | ✅ | ✅ |
| Debloat + harden built in | ❌ | ❌ | ❌ | ⚠️ DIY | ⚠️ DIY | ✅ |
| Dotfiles + secrets + identity | ❌ | ❌ | ❌ | ⚠️ | ⚠️ | ✅ |
| Offline / air-gap bundles | ❌ | ❌ | ⚠️ | ⚠️ heavy | ✅ | ✅ |
| WSL bootstrap as first-class | ❌ | ❌ | ❌ | ⚠️ | ❌ | ✅ |
| Cryptographic identity / signing | ✅ pkgs | ⚠️ | ⚠️ | ❌ | ⚠️ | ✅ |
| Reproducible (lockfile) | ❌ | ⚠️ | ❌ | ⚠️ | ✅ | ✅ |
| Readable config language | ⚠️ YAML | JSON | XML/PS | YAML | ❌ Nix lang | ✅ TOML |
| Personality | ❌ | ⚠️ | ⚠️ | ❌ | ❌ | ✅ |

We don't win on package count — winget and Scoop already solved that, and we wrap them rather than reinvent them. We win on the first five minutes after install, and on everything that happens *around* the packages.

**Why we wrap winget/scoop instead of building our own package manager:** reinventing package management is a multi-year project that adds zero user value. Microsoft maintains winget; the community maintains Scoop's manifests. We orchestrate them and spend our effort on what's actually unique — profiles, identity, offline bundles, debloat, WSL, and the lane ecosystem. Brew didn't reinvent compilers; it orchestrated them. Same play.

---

## Our policy on what you do with your own machine

**Your machine is your machine.** Install whatever you want. Debloat Windows into oblivion. Run offensive security tools. Strip identifiers. Tunnel through anything. Mod anything. We don't ask, we don't log to anyone but you, we don't care.

**What we won't host on the public lane registry:**
- Lanes designed to attack other Candylane users (supply-chain attacks, credential theft)
- Lanes that exfiltrate user data without explicit disclosure in the manifest
- Content that is illegal in essentially every jurisdiction (CSAM and the like)

That's the entire policy — the same line GitHub holds. If your work doesn't violate those three rules, it's welcome, no matter how edgy, niche, or aggressive the use case. See [MANIFESTO.md](./MANIFESTO.md) for the why.

---

## Status

🍭 **Pre-alpha. Built in the open. Built by power users, for power users.**

Time is what it is. **Completion is what matters.** We ship each phase when it's solid, not when a calendar says so. See [ROADMAP.md](./ROADMAP.md) for where we are and where we're going, and [REFERENCES.md](./REFERENCES.md) for the shoulders we stand on.

- **Repo:** github.com/candylane/candylane *(coming soon)*
- **Site:** candylane.sh *(coming soon)*
- **Lanes:** lanes.candylane.sh *(coming soon)*

---

*Your machine. Anywhere. In a minute.*
