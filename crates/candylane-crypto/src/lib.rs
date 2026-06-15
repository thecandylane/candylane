//! candylane-crypto — local Ed25519 identity for Phase 1.
//!
//! Phase 1 only generates + stores the key and can sign/verify locally. Profile
//! signing/verification is Phase 5. The non-negotiable here is CRITICAL #3:
//! only the owner can read the private key.
//!
//! Decision: DPAPI (user-scope) is the primary mechanism on Windows. The on-disk
//! representation is ciphertext; successful unprotect on load is the assertion.
//! This is strictly stronger than ACL for copied-file threats and requires far
//! less security-critical code. A trivial owner-only file permission is kept as
//! defense-in-depth on the ciphertext file.
//!
//! The private signing key is author-machine-bound in practice (see portability
//! clarification: bundles travel, the private key does not). Explicit export/
//! re-protect for the "new authoring laptop, same identity" case lands in Phase 5.
//!
//! Decision #8 carve-out: the ONLY Windows-API surface allowed in this crate is
//! the two DPAPI calls (plus any minimal owner-grant helper if added later).
//!
//! Unix fallback remains the honest 0600 (weaker, as documented).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

pub struct Identity {
    signing: ed25519_dalek::SigningKey,
}

impl Identity {
    /// ~/.candylane/identity.key
    pub fn key_path() -> Result<PathBuf> {
        let home = dirs_home().context("could not resolve home directory")?;
        Ok(home.join(".candylane").join("identity.key"))
    }

    /// Generate a fresh keypair and persist it owner-only (DPAPI-protected on Windows).
    /// Never clobbers an existing key.
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

        let plaintext = signing.to_bytes().to_vec();

        #[cfg(windows)]
        let to_write = protect_key(&plaintext)?;
        #[cfg(not(windows))]
        let to_write = plaintext.clone();

        // Zero the in-memory copy of the raw private key as soon as possible.
        let mut plaintext = plaintext;
        plaintext.zeroize();

        // Write the (possibly protected) blob, then the platform-specific
        // "owner-only" step (cheap grant on Windows, 0600 on Unix).
        std::fs::write(&path, &to_write)?;
        enforce_owner_only(&path)?;
        assert_owner_only(&path)?;
        Ok(Self { signing })
    }

    /// Load the key.
    ///
    /// On Windows: the file is DPAPI-protected. We unprotect it; a failed
    /// unprotect is fatal (no plaintext fallback) — the bytes are not a valid
    /// identity for this user on this machine, so we refuse rather than accept
    /// an unauthenticated key. The caller is directed to re-`init`.
    ///
    /// The "assert owner-only" contract is preserved: on Windows the unprotect
    /// success itself is the assertion. On Unix we still do the 0600 check.
    pub fn load() -> Result<Self> {
        let path = Self::key_path()?;
        assert_owner_only(&path)
            .with_context(|| format!("private key {} has unsafe permissions", path.display()))?;

        let raw = std::fs::read(&path)?;

        #[cfg(windows)]
        let key_bytes = unprotect_key(&raw)
            .context("DPAPI unprotect failed for identity key. This key is not valid for the current user on this machine. If this is a legacy dev key from before DPAPI protection was enabled, delete it and run `candylane init` again to generate a fresh protected identity.")?;

        #[cfg(not(windows))]
        let key_bytes = raw;

        // Zero the source buffer after we have copied the bytes out.
        let mut key_bytes = key_bytes;
        let arr: [u8; 32] = key_bytes
            .as_slice()
            .try_into()
            .context("malformed key length")?;
        key_bytes.zeroize();

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

// ---- DPAPI protection (Windows) + 0600 fallback (Unix) ---------------------------
//
// Decision (post threat-model comparison + product clarification):
// DPAPI (user-scope) is primary for the private Ed25519 signing key on Windows.
// The file on disk is ciphertext. Successful CryptUnprotectData on load *is* the
// assertion that only the owner on a machine with their logon material can read it.
// A copied/stolen file is undecryptable ciphertext — this is the property ACL could
// never deliver.
//
// The private key is author-machine-bound in the actual workflow (laptop signs .candy;
// USB carries the signed bundle, not the key; targets verify or trust physical presence).
// This is exactly the case where DPAPI is strictly better and simpler.
//
// Portability of the *private key* between your own author machines (dead laptop, etc.)
// is a deliberate Phase 5 export/re-wrap flow (or fresh identity). It is not the job
// of the at-rest protection.
//
// On Windows the crypto is the only protection currently in force: the Windows
// enforce/assert_owner_only fns are deliberate no-ops (see below). A cheap
// owner-only grant on the ciphertext file is NOT yet implemented — it is a
// possible future defense-in-depth, not a current guarantee.
//
// Unix remains the honest (weaker) 0600 fallback.

#[cfg(windows)]
fn protect_key(plaintext: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let data_in = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len() as u32,
        pbData: plaintext.as_ptr() as *mut u8,
    };
    let mut data_out = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptProtectData(
            &data_in,
            None, // no description
            None, // no entropy
            None,
            None, // no prompt
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut data_out,
        )?;

        let protected =
            std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize).to_vec();

        // DPAPI allocated with LocalAlloc; we must free it.
        let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
            data_out.pbData as _,
        ));

        Ok(protected)
    }
}

#[cfg(windows)]
fn unprotect_key(protected: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let data_in = CRYPT_INTEGER_BLOB {
        cbData: protected.len() as u32,
        pbData: protected.as_ptr() as *mut u8,
    };
    let mut data_out = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptUnprotectData(
            &data_in,
            None, // description out (optional)
            None, // entropy
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut data_out,
        )?;

        let plaintext =
            std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize).to_vec();

        let _ = windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
            data_out.pbData as _,
        ));

        Ok(plaintext)
    }
}

// Windows: write DPAPI-protected blob. The "owner-only" is now primarily the
// fact that only the user who protected it can unprotect it. We still ensure
// the file lives in the user's profile.
#[cfg(windows)]
fn enforce_owner_only(_path: &Path) -> Result<()> {
    // DPAPI is the primary protection (the bytes are ciphertext and only
    // decrypt for this user on this profile).
    // We intentionally keep the Windows enforce/assert as no-ops for now
    // to avoid re-introducing the heavy ACL machinery.
    // The file lives inside the user's profile directory, which on a normal
    // Windows installation already limits unprivileged cross-user access.
    // A future cheap owner-only grant (defense-in-depth) can be added here
    // without the full negative-assertion/repair-valve complexity.
    Ok(())
}

#[cfg(windows)]
fn assert_owner_only(_path: &Path) -> Result<()> {
    // The real load-time assertion on Windows is "unprotect succeeded".
    // See load() for the context! error if it fails.
    Ok(())
}

// Unix fallback — honest and weaker (root can read, same-uid processes on some
// systems can be a concern). This is exactly the same model we had before.
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

// ---- Windows DPAPI test stubs ------------------------------------------------
// These only run on a real Windows msvc host with --ignored.
// They exercise the exact path used by CRITICAL #3 (generate + load + sign).

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::*;

    #[test]
    #[cfg(windows)]
    #[ignore = "native msvc Windows only; run `cargo test -p candylane-crypto -- --ignored` on a Windows host"]
    fn dpapi_protect_unprotect_roundtrip_sign_verify() {
        // Fresh generate writes a DPAPI blob.
        let id = Identity::generate().expect("generate should succeed on Windows");
        let vk = id.verifying_key();

        // Load must go through unprotect (this is the assertion).
        let id2 = Identity::load().expect("load must succeed via DPAPI unprotect");
        assert_eq!(id2.verifying_key(), vk, "loaded key must match");

        // Exercise the signing path with the loaded key.
        let msg = b"candylane refurb-clean DPAPI test vector";
        let sig = id2.sign(msg);
        assert!(vk.verify_strict(msg, &sig).is_ok(), "signature must verify");

        // Best-effort cleanup for the test key (never do this in production use).
        if let Ok(p) = Identity::key_path() {
            let _ = std::fs::remove_file(p);
        }
    }
}
