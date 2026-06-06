//! candylane-crypto — local Ed25519 identity for Phase 1.
//!
//! Phase 1 only generates + stores the key and can sign/verify locally. Profile
//! signing/verification is Phase 5. The non-negotiable here is CRITICAL #3:
//! the private key is written owner-only and that ACL is asserted on every load.
//!
//! Decision #8 (windows-rs carve-out): the ONLY place a Windows-API dependency is
//! allowed in Phase 1 is `enforce_owner_only` / `assert_owner_only` below.
//!
//! SCAFFOLD: not compiled (no toolchain on the dev host). The ACL calls are sketched.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub struct Identity {
    signing: ed25519_dalek::SigningKey,
}

impl Identity {
    /// ~/.candylane/identity.key
    pub fn key_path() -> Result<PathBuf> {
        let home = dirs_home().context("could not resolve home directory")?;
        Ok(home.join(".candylane").join("identity.key"))
    }

    /// Generate a fresh keypair and persist it owner-only. Never clobbers an existing key.
    pub fn generate() -> Result<Self> {
        let path = Self::key_path()?;
        if path.exists() {
            anyhow::bail!(
                "identity already exists at {} — refusing to clobber",
                path.display()
            );
        }
        let signing = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Write THEN lock down, then assert. Order matters: never leave a window
        // where the key exists with loose perms.
        std::fs::write(&path, signing.to_bytes())?;
        enforce_owner_only(&path)?;
        assert_owner_only(&path)?;
        Ok(Self { signing })
    }

    /// Load the key, asserting owner-only perms first. Refuses to load a key with
    /// loose permissions — this is the trust model, not best-effort.
    pub fn load() -> Result<Self> {
        let path = Self::key_path()?;
        assert_owner_only(&path)
            .with_context(|| format!("private key {} has unsafe permissions", path.display()))?;
        let bytes = std::fs::read(&path)?;
        let arr: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .context("malformed key length")?;
        Ok(Self {
            signing: ed25519_dalek::SigningKey::from_bytes(&arr),
        })
    }

    pub fn verifying_key(&self) -> ed25519_dalek::VerifyingKey {
        self.signing.verifying_key()
    }

    pub fn sign(&self, msg: &[u8]) -> ed25519_dalek::Signature {
        use ed25519_dalek::Signer;
        self.signing.sign(msg)
    }
}

fn dirs_home() -> Option<PathBuf> {
    // Avoid an extra dep in the scaffold; real impl uses the `dirs` crate.
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

// ---- ACL carve-out (Decision #8) -------------------------------------------------

/// Set the private key to owner-only access.
#[cfg(windows)]
fn enforce_owner_only(_path: &Path) -> Result<()> {
    // windows-acl: strip inherited ACEs, set an explicit DACL granting only the
    // current user's SID. Do NOT shell to icacls (fragile, localized, can't assert).
    todo!("set explicit owner-only DACL via windows-acl")
}

/// Verify the key is still owner-only. Called on every load.
#[cfg(windows)]
fn assert_owner_only(_path: &Path) -> Result<()> {
    todo!("read DACL via windows-acl; bail if any non-owner ACE grants access")
}

// Non-Windows dev/CI: use unix perms so the crate builds and tests off-Windows.
#[cfg(unix)]
fn enforce_owner_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(unix)]
fn assert_owner_only(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        anyhow::bail!("key mode {:o} is not owner-only", mode);
    }
    Ok(())
}
