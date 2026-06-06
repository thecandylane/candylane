//! TOML profile parser. Converts the Phase 1 minimal-profile format into an
//! `engine::Profile` (ordered `Vec<Item>`) and computes the SHA-256 hash of the
//! raw TOML bytes.
//!
//! Item ordering is fixed: winget packages first, then dotfiles, then post_install
//! scripts — matching the engine iteration order defined in the architecture spec.

use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::engine::Profile;
use crate::types::Item;
use crate::Result;

// ---------------------------------------------------------------------------
// TOML shape — private serde structs
// ---------------------------------------------------------------------------

/// Top-level profile document.
#[derive(Debug, Deserialize)]
struct RawProfile {
    name: String,
    /// Recorded in the DB but not otherwise used by the core.
    #[allow(dead_code)]
    version: Option<String>,
    packages: Option<RawPackages>,
    dotfiles: Option<RawDotfiles>,
    post_install: Option<Vec<RawScript>>,
}

/// `[packages]` table.
#[derive(Debug, Deserialize)]
struct RawPackages {
    winget: Option<RawWinget>,
}

/// `[packages.winget]`
#[derive(Debug, Deserialize)]
struct RawWinget {
    install: Option<Vec<String>>,
}

/// `[dotfiles]` — contains an array-of-tables under key `file`.
#[derive(Debug, Deserialize)]
struct RawDotfiles {
    file: Option<Vec<RawDotfile>>,
}

/// `[[dotfiles.file]]`
#[derive(Debug, Deserialize)]
struct RawDotfile {
    src: String,
    target: String,
}

/// `[[post_install]]`
#[derive(Debug, Deserialize)]
struct RawScript {
    run: String,
    undo: Option<String>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a profile from a TOML file on disk.
pub fn parse(path: &Path) -> Result<Profile> {
    let raw_bytes = std::fs::read(path)?;
    let source = std::str::from_utf8(&raw_bytes)
        .map_err(|e| anyhow::anyhow!("profile is not valid UTF-8: {e}"))?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_owned();
    build_profile(source, &name, &raw_bytes)
}

/// Parse a profile from an in-memory TOML string. The `name` argument is used as
/// `Profile::name` when the string does not supply one (test convenience; real
/// profiles always have a `name` field and the field value wins).
pub fn parse_str(s: &str, name: &str) -> Result<Profile> {
    build_profile(s, name, s.as_bytes())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn build_profile(source: &str, fallback_name: &str, hash_bytes: &[u8]) -> Result<Profile> {
    let raw: RawProfile =
        toml::from_str(source).map_err(|e| anyhow::anyhow!("failed to parse profile TOML: {e}"))?;

    let profile_name = raw.name.clone();
    let _ = fallback_name; // the TOML `name` field is always present and wins

    let mut items: Vec<Item> = Vec::new();

    // 1. winget packages
    if let Some(pkgs) = raw.packages.and_then(|p| p.winget).and_then(|w| w.install) {
        for pkg in pkgs {
            items.push(Item::Winget { pkg });
        }
    }

    // 2. dotfiles
    if let Some(files) = raw.dotfiles.and_then(|d| d.file) {
        for f in files {
            items.push(Item::Dotfile {
                src: f.src,
                target: f.target,
            });
        }
    }

    // 3. post_install scripts
    if let Some(scripts) = raw.post_install {
        for s in scripts {
            items.push(Item::Script {
                run: s.run,
                undo: s.undo,
            });
        }
    }

    let hash = hex_sha256(hash_bytes);

    Ok(Profile {
        name: profile_name,
        hash,
        items,
    })
}

/// Return the lowercase hex SHA-256 of `bytes`.
/// `Sha256::digest` returns a `GenericArray`, which does NOT implement `LowerHex`,
/// so `{:x}` won't work — format each byte.
fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_DEV: &str = r#"
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
undo = "./scripts/example-tweak.undo.ps1"
"#;

    #[test]
    fn parse_minimal_dev_name_and_item_count() {
        let p = parse_str(MINIMAL_DEV, "ignored").unwrap();
        assert_eq!(p.name, "minimal-dev");
        // 3 winget + 1 dotfile + 1 script = 5
        assert_eq!(p.items.len(), 5);
    }

    #[test]
    fn ordering_winget_dotfile_script() {
        let p = parse_str(MINIMAL_DEV, "ignored").unwrap();
        assert!(matches!(&p.items[0], Item::Winget { pkg } if pkg == "Git.Git"));
        assert!(matches!(&p.items[1], Item::Winget { pkg } if pkg == "Microsoft.VisualStudioCode"));
        assert!(matches!(&p.items[2], Item::Winget { pkg } if pkg == "BurntSushi.ripgrep.MSVC"));
        assert!(matches!(&p.items[3], Item::Dotfile { src, target }
            if src == "./home/.gitconfig" && target == "$HOME/.gitconfig"));
        assert!(matches!(&p.items[4], Item::Script { run, undo }
            if run == "./scripts/example-tweak.ps1"
            && undo.as_deref() == Some("./scripts/example-tweak.undo.ps1")));
    }

    #[test]
    fn hash_is_lowercase_hex_64_chars() {
        let p = parse_str(MINIMAL_DEV, "ignored").unwrap();
        assert_eq!(p.hash.len(), 64);
        assert!(p.hash.chars().all(|c| c.is_ascii_hexdigit()));
        // Uppercase letters must not appear (lowercase requirement).
        assert!(!p.hash.chars().any(|c| c.is_ascii_uppercase()));
    }

    #[test]
    fn hash_is_deterministic() {
        let p1 = parse_str(MINIMAL_DEV, "ignored").unwrap();
        let p2 = parse_str(MINIMAL_DEV, "ignored").unwrap();
        assert_eq!(p1.hash, p2.hash);
    }

    #[test]
    fn different_content_different_hash() {
        let other = MINIMAL_DEV.replace("minimal-dev", "other-profile");
        let p1 = parse_str(MINIMAL_DEV, "a").unwrap();
        let p2 = parse_str(&other, "b").unwrap();
        assert_ne!(p1.hash, p2.hash);
    }

    #[test]
    fn profile_with_no_optional_sections() {
        let toml = r#"name = "empty""#;
        let p = parse_str(toml, "empty").unwrap();
        assert_eq!(p.name, "empty");
        assert!(p.items.is_empty());
    }

    #[test]
    fn script_without_undo_is_none() {
        let toml = r#"
name = "no-undo"
[[post_install]]
run = "./up.ps1"
"#;
        let p = parse_str(toml, "no-undo").unwrap();
        assert_eq!(p.items.len(), 1);
        assert!(matches!(&p.items[0], Item::Script { undo, .. } if undo.is_none()));
    }
}
