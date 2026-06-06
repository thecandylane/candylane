//! DotfileHandler — copies a source file to an expanded target path, with sha256-verified
//! backup-and-restore for CRITICAL #2 (integrity on undo).
//!
//! ## Undo recipe shapes
//!
//! When the target file **did not exist** before apply:
//! ```json
//! { "action": "delete", "target": "<expanded target path>" }
//! ```
//!
//! When the target file **existed** before apply (a backup is taken):
//! ```json
//! {
//!   "action": "restore",
//!   "target": "<expanded target path>",
//!   "backup": "<absolute path to .bak file>",
//!   "backup_sha256": "<lowercase hex sha256 of the backup bytes>"
//! }
//! ```
//!
//! synthesize_undo crash-recovery shapes:
//!
//! before.exists == false (we created the file, no prior backup opportunity):
//! ```json
//! { "action": "delete", "target": "<expanded target path>" }
//! ```
//!
//! before.exists == true (original was overwritten before crash; backup may not exist):
//! ```json
//! {
//!   "action": "noop",
//!   "reason": "original not backed up before crash; cannot restore"
//! }
//! ```

use std::path::{Path, PathBuf};

use anyhow::Context;
use sha2::{Digest, Sha256};

use crate::handler::Handler;
use crate::types::{
    Applied, ApplyCtx, HandlerKind, Item, Json, PlannedAction, Probe, RecordedAction, Target,
    UndoKind,
};
use crate::Result;

// ─────────────────────────────────────────────────────────────────────────────
// Path expansion helper
// ─────────────────────────────────────────────────────────────────────────────

/// Expand a leading `~` or `$HOME` to the value of the `HOME` environment variable.
/// On Windows the `HOME` env var may not be set; the call gracefully returns the
/// unexpanded string in that case (real expansion uses USERPROFILE in the CLI layer).
fn expand(p: &str) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    if p.starts_with("~/") {
        PathBuf::from(format!("{}{}", home, &p[1..]))
    } else if p == "~" {
        PathBuf::from(&home)
    } else if p.starts_with("$HOME/") {
        PathBuf::from(format!("{}{}", home, &p[5..]))
    } else if p == "$HOME" {
        PathBuf::from(&home)
    } else {
        PathBuf::from(p)
    }
}

/// Reject a dotfile target that escapes its intended location.
///
/// Phase-1 profiles are owner-authored, but Candylane's whole future is *fetching
/// profiles from a registry* — at which point `target` is attacker-influenced. A `..`
/// component turns an innocent-looking `~/cfg/../../etc/cron.d/evil` into an arbitrary
/// system-file overwrite. We refuse rather than silently follow it. An unexpanded
/// leading variable (anything starting with `$` that is not `$HOME`) is also rejected
/// so a typo deploys nowhere surprising instead of to a literal `$FOO` path.
fn reject_unsafe_target(raw: &str, expanded: &Path) -> Result<()> {
    if expanded
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        anyhow::bail!(
            "dotfile target {raw:?} contains a '..' path component — refusing (path traversal)"
        );
    }
    if raw.starts_with('$') && !raw.starts_with("$HOME") {
        anyhow::bail!(
            "dotfile target {raw:?} uses an unsupported variable — only ~ and $HOME are expanded"
        );
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// SHA-256 helper (same byte-by-byte format used in profile.rs)
// ─────────────────────────────────────────────────────────────────────────────

/// Return the lowercase hex SHA-256 of `bytes`.
/// `Sha256::digest` returns a `GenericArray` without `LowerHex`, so we format per byte.
fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Compute the sha256 of a file on disk.
fn sha256_of_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("sha256_of_file: could not read {}", path.display()))?;
    Ok(hex_sha256(&bytes))
}

// ─────────────────────────────────────────────────────────────────────────────
// Backup-path helper
// ─────────────────────────────────────────────────────────────────────────────

/// Deterministic backup filename: `<sha256 of target path string>.bak`
fn backup_path(backups_dir: &Path, target_path_str: &str) -> PathBuf {
    let name = hex_sha256(target_path_str.as_bytes());
    backups_dir.join(format!("{name}.bak"))
}

/// Atomically write `bytes` to `path`: write a sibling temp file, then `rename` it over the
/// target. `std::fs::rename` replaces atomically on both Unix and Windows, so a crash mid-write
/// can never leave a torn file — the target is always either the old bytes or the new, never a
/// partial. The caller must ensure `path`'s parent directory already exists.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("atomic_write: target has no file name: {}", path.display()))?;
    let tmp = parent.join(format!(".{file_name}.candylane-tmp"));
    std::fs::write(&tmp, bytes)
        .with_context(|| format!("atomic_write: temp write failed at {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| {
        format!(
            "atomic_write: rename {} -> {} failed",
            tmp.display(),
            path.display()
        )
    })?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Public struct
// ─────────────────────────────────────────────────────────────────────────────

pub struct DotfileHandler;

impl DotfileHandler {
    pub fn new() -> Self {
        DotfileHandler
    }
}

impl Default for DotfileHandler {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Handler impl
// ─────────────────────────────────────────────────────────────────────────────

impl Handler for DotfileHandler {
    fn kind(&self) -> HandlerKind {
        HandlerKind::Dotfile
    }

    // ── probe ──────────────────────────────────────────────────────────────────

    /// Pure read. If the target file exists, return `{"exists":true,"sha256":HEX}`.
    /// If it is absent, return `{"exists":false}`.
    fn probe(&self, target: &Target) -> Result<Probe> {
        let path = expand(&target.0);
        if path.exists() {
            let bytes = std::fs::read(&path)
                .with_context(|| format!("probe: could not read {}", path.display()))?;
            let sha = hex_sha256(&bytes);
            Ok(Probe(serde_json::json!({ "exists": true, "sha256": sha })))
        } else {
            Ok(Probe(serde_json::json!({ "exists": false })))
        }
    }

    // ── plan ───────────────────────────────────────────────────────────────────

    /// Return `Ok(None)` if the target already matches the desired source file (idempotent).
    /// Otherwise return a `PlannedAction` with the src path and desired sha256 in `payload`.
    fn plan(&self, desired: &Item, probe: &Probe) -> Result<Option<PlannedAction>> {
        let (src, target_raw) = match desired {
            Item::Dotfile { src, target } => (src, target),
            _ => anyhow::bail!("DotfileHandler::plan called with non-Dotfile item"),
        };

        let src_bytes = std::fs::read(src)
            .with_context(|| format!("plan: could not read source file {src}"))?;
        let desired_sha = hex_sha256(&src_bytes);

        // Idempotent re-pull: if exists and sha matches, nothing to do.
        if let Some(existing_sha) = probe.0.get("sha256").and_then(|v| v.as_str()) {
            if probe.0.get("exists").and_then(|v| v.as_bool()) == Some(true)
                && existing_sha == desired_sha
            {
                return Ok(None);
            }
        }

        let expanded_target = expand(target_raw);
        reject_unsafe_target(target_raw, &expanded_target)?;
        let expanded_target_str = expanded_target
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("plan: target path is not valid UTF-8: {target_raw}"))?
            .to_owned();

        Ok(Some(PlannedAction {
            handler: HandlerKind::Dotfile,
            target: Target(expanded_target_str),
            before: probe.0.clone(),
            undo_kind: UndoKind::Inverse,
            payload: serde_json::json!({
                "src": src,
                "desired_sha256": desired_sha,
            }),
        }))
    }

    // ── apply ──────────────────────────────────────────────────────────────────

    /// Copy `payload.src` to `action.target`, taking a sha256-tagged backup of any
    /// existing file first.  Re-probes after the write to confirm the effect.
    fn apply(&self, action: &PlannedAction, ctx: &ApplyCtx) -> Result<Applied> {
        let src_str = action.payload["src"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("apply: payload missing 'src'"))?;
        let desired_sha = action.payload["desired_sha256"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("apply: payload missing 'desired_sha256'"))?;

        let target_path = Path::new(&action.target.0);

        // 1. Ensure backups directory exists.
        std::fs::create_dir_all(ctx.backups_dir).with_context(|| {
            format!(
                "apply: could not create backups dir {}",
                ctx.backups_dir.display()
            )
        })?;

        // 2. Backup existing target if present.
        let undo_recipe: Json = if target_path.exists() {
            let existing_bytes = std::fs::read(target_path).with_context(|| {
                format!(
                    "apply: could not read existing target {}",
                    target_path.display()
                )
            })?;
            let existing_sha = hex_sha256(&existing_bytes);
            let bak = backup_path(ctx.backups_dir, &action.target.0);
            atomic_write(&bak, &existing_bytes)
                .with_context(|| format!("apply: could not write backup to {}", bak.display()))?;
            serde_json::json!({
                "action": "restore",
                "target": action.target.0,
                "backup": bak.to_str().ok_or_else(|| anyhow::anyhow!("apply: backup path not UTF-8"))?,
                "backup_sha256": existing_sha,
            })
        } else {
            serde_json::json!({
                "action": "delete",
                "target": action.target.0,
            })
        };

        // 3. Create parent directories and write the source bytes to the target.
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "apply: could not create parent dirs for {}",
                    target_path.display()
                )
            })?;
        }
        let src_bytes = std::fs::read(src_str)
            .with_context(|| format!("apply: could not read src {src_str}"))?;
        atomic_write(target_path, &src_bytes)
            .with_context(|| format!("apply: could not write to {}", target_path.display()))?;

        // 4. RE-PROBE — success is confirmed by reading back, not by trusting the write syscall.
        let actual_sha = sha256_of_file(target_path)
            .with_context(|| format!("apply: re-probe failed for {}", target_path.display()))?;
        if actual_sha != desired_sha {
            anyhow::bail!(
                "apply: post-write sha256 mismatch for {} (expected {}, got {})",
                target_path.display(),
                desired_sha,
                actual_sha,
            );
        }

        Ok(Applied {
            after: serde_json::json!({ "exists": true, "sha256": actual_sha }),
            undo: undo_recipe,
        })
    }

    // ── undo ───────────────────────────────────────────────────────────────────

    /// Reverse a recorded dotfile action.  CRITICAL #2: for the "restore" recipe, the
    /// backup bytes are sha256-verified BEFORE any write.  Mismatch refuses — never write
    /// unverified bytes over the user file.
    fn undo(&self, action: &RecordedAction, _ctx: &ApplyCtx) -> Result<()> {
        let recipe = &action.undo;
        let action_name = recipe["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("undo: recipe missing 'action' field"))?;

        match action_name {
            "delete" => {
                let target_str = recipe["target"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("undo delete: recipe missing 'target'"))?;
                let target_path = Path::new(target_str);
                if target_path.exists() {
                    std::fs::remove_file(target_path).with_context(|| {
                        format!("undo delete: could not remove {}", target_path.display())
                    })?;
                }
                // Already gone → idempotent Ok(()).
                Ok(())
            }

            "restore" => {
                let target_str = recipe["target"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("undo restore: recipe missing 'target'"))?;
                let backup_str = recipe["backup"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("undo restore: recipe missing 'backup'"))?;
                let expected_sha = recipe["backup_sha256"].as_str().ok_or_else(|| {
                    anyhow::anyhow!("undo restore: recipe missing 'backup_sha256'")
                })?;

                let backup_path = Path::new(backup_str);
                let backup_bytes = std::fs::read(backup_path).with_context(|| {
                    format!(
                        "undo restore: could not read backup file {}",
                        backup_path.display()
                    )
                })?;

                // ── CRITICAL #2 — integrity check before any write ──────────────
                let actual_sha = hex_sha256(&backup_bytes);
                if actual_sha != expected_sha {
                    anyhow::bail!(
                        "backup integrity check failed for {target_str}: \
                         refusing to restore corrupt bytes \
                         (expected sha256 {expected_sha}, got {actual_sha})"
                    );
                }
                // ── end integrity check ─────────────────────────────────────────

                let target_path = Path::new(target_str);
                // Create parent dirs in case undo runs in a fresh environment.
                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!(
                            "undo restore: could not create parent dirs for {}",
                            target_path.display()
                        )
                    })?;
                }
                atomic_write(target_path, &backup_bytes).with_context(|| {
                    format!("undo restore: could not write to {}", target_path.display())
                })?;
                Ok(())
            }

            "noop" => Ok(()),

            other => anyhow::bail!("undo: unknown action {other:?} in recipe"),
        }
    }

    // ── synthesize_undo ────────────────────────────────────────────────────────

    /// Crash-reconcile leaf (CRITICAL #4).
    ///
    /// `before` = the probe state captured at plan time (before apply ran).
    /// `probe`  = the post-crash observed state (apply already took effect).
    ///
    /// - `before.exists == false` → we created the file; undo must delete it.
    /// - `before.exists == true`  → we overwrote the file; the original is gone.
    ///   BE HONEST: do not fabricate a backup.  Return a `noop` recipe that
    ///   explains why restoration is impossible.
    fn synthesize_undo(&self, target: &Target, before: &Json, probe: &Probe) -> Result<Applied> {
        let before_existed = before
            .get("exists")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if before_existed {
            // Original was overwritten without a backup (crash skipped backup step).
            // Returning a noop recipe surfaces the loss honestly in diff/history.
            Ok(Applied {
                after: probe.0.clone(),
                undo: serde_json::json!({
                    "action": "noop",
                    "reason": "original not backed up before crash; cannot restore",
                }),
            })
        } else {
            // File was created by our apply; undo by deleting it.
            Ok(Applied {
                after: probe.0.clone(),
                undo: serde_json::json!({
                    "action": "delete",
                    "target": target.0,
                }),
            })
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    /// Build a minimal `ApplyCtx` pointing at `backups_dir`.
    fn make_ctx(backups_dir: &Path) -> ApplyCtx<'_> {
        ApplyCtx {
            backups_dir,
            timeout: Duration::from_secs(5),
            dry_run: false,
            max_undo_attempts: 3,
        }
    }

    /// Minimal `RecordedAction` for testing `undo`.
    fn make_recorded(target_str: &str, undo_recipe: Json) -> RecordedAction {
        RecordedAction {
            id: 1,
            op_id: 1,
            seq: 0,
            handler: HandlerKind::Dotfile,
            target: Target(target_str.to_owned()),
            status: crate::types::ActionStatus::Applied,
            before: serde_json::json!({}),
            after: None,
            undo_kind: UndoKind::Inverse,
            undo: undo_recipe,
            undo_attempts: 0,
            undo_error: None,
        }
    }

    // ── probe ────────────────────────────────────────────────────────────────

    #[test]
    fn probe_missing_file_returns_not_exists() {
        let handler = DotfileHandler::new();
        let tmp = TempDir::new().unwrap();
        let ghost = tmp.path().join("ghost.txt");
        // File does not exist.
        let probe = handler
            .probe(&Target(ghost.to_str().unwrap().to_owned()))
            .unwrap();
        assert_eq!(probe.0["exists"], false);
        assert!(probe.0.get("sha256").is_none());
    }

    #[test]
    fn probe_existing_file_returns_exists_and_sha() {
        let handler = DotfileHandler::new();
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("config.toml");
        std::fs::write(&file, b"hello world").unwrap();

        let probe = handler
            .probe(&Target(file.to_str().unwrap().to_owned()))
            .unwrap();
        assert_eq!(probe.0["exists"], true);

        let sha = probe.0["sha256"].as_str().unwrap();
        assert_eq!(sha.len(), 64);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
        // Value must be lowercase.
        assert!(!sha.chars().any(|c| c.is_ascii_uppercase()));
        // Verify the sha matches a fresh computation.
        assert_eq!(sha, hex_sha256(b"hello world"));
    }

    // ── plan ─────────────────────────────────────────────────────────────────

    #[test]
    fn plan_noop_when_sha_matches() {
        let handler = DotfileHandler::new();
        let tmp = TempDir::new().unwrap();

        // Write source file.
        let src = tmp.path().join("source.toml");
        std::fs::write(&src, b"content").unwrap();

        // Probe that already shows the same sha as the source.
        let sha = hex_sha256(b"content");
        let probe = Probe(serde_json::json!({ "exists": true, "sha256": sha }));

        let item = Item::Dotfile {
            src: src.to_str().unwrap().to_owned(),
            target: "/some/target".to_owned(),
        };

        let result = handler.plan(&item, &probe).unwrap();
        assert!(
            result.is_none(),
            "plan should return None when target already matches source"
        );
    }

    #[test]
    fn plan_some_when_sha_differs() {
        let handler = DotfileHandler::new();
        let tmp = TempDir::new().unwrap();

        let src = tmp.path().join("source.toml");
        std::fs::write(&src, b"new content").unwrap();

        // Probe shows a different sha.
        let probe = Probe(serde_json::json!({ "exists": true, "sha256": "000000" }));

        let item = Item::Dotfile {
            src: src.to_str().unwrap().to_owned(),
            target: "/some/target".to_owned(),
        };

        let result = handler.plan(&item, &probe).unwrap();
        assert!(result.is_some(), "plan should return Some when sha differs");
    }

    #[test]
    fn plan_some_when_target_absent() {
        let handler = DotfileHandler::new();
        let tmp = TempDir::new().unwrap();

        let src = tmp.path().join("source.toml");
        std::fs::write(&src, b"content").unwrap();

        let probe = Probe(serde_json::json!({ "exists": false }));
        let item = Item::Dotfile {
            src: src.to_str().unwrap().to_owned(),
            target: "/nonexistent/target".to_owned(),
        };

        let result = handler.plan(&item, &probe).unwrap();
        assert!(
            result.is_some(),
            "plan should return Some when target does not exist"
        );
    }

    // ── CRITICAL: path-traversal guard ───────────────────────────────────────

    #[test]
    fn plan_rejects_parent_dir_traversal() {
        let handler = DotfileHandler::new();
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("source.toml");
        std::fs::write(&src, b"content").unwrap();

        let probe = Probe(serde_json::json!({ "exists": false }));
        // A target that climbs out via `..` must be refused at plan time.
        let item = Item::Dotfile {
            src: src.to_str().unwrap().to_owned(),
            target: "/home/victim/cfg/../../../etc/cron.d/evil".to_owned(),
        };

        let result = handler.plan(&item, &probe);
        assert!(result.is_err(), "plan must reject a '..' traversal target");
        assert!(
            result.unwrap_err().to_string().contains("path traversal"),
            "error must name the traversal"
        );
    }

    #[test]
    fn plan_rejects_unsupported_variable() {
        let handler = DotfileHandler::new();
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("source.toml");
        std::fs::write(&src, b"content").unwrap();

        let probe = Probe(serde_json::json!({ "exists": false }));
        let item = Item::Dotfile {
            src: src.to_str().unwrap().to_owned(),
            target: "$APPDATA/whatever".to_owned(),
        };

        let result = handler.plan(&item, &probe);
        assert!(
            result.is_err(),
            "plan must reject an unsupported leading variable"
        );
    }

    // ── apply + undo (non-existing target) ───────────────────────────────────

    #[test]
    fn apply_over_nonexistent_then_undo_deletes() {
        let handler = DotfileHandler::new();
        let tmp = TempDir::new().unwrap();
        let backups = tmp.path().join("backups");

        // Source file.
        let src = tmp.path().join("src.txt");
        std::fs::write(&src, b"hello from src").unwrap();
        let desired_sha = hex_sha256(b"hello from src");

        // Target does NOT exist.
        let target_path = tmp.path().join("dest.txt");
        assert!(!target_path.exists());

        let before_probe = Probe(serde_json::json!({ "exists": false }));
        let planned = PlannedAction {
            handler: HandlerKind::Dotfile,
            target: Target(target_path.to_str().unwrap().to_owned()),
            before: before_probe.0.clone(),
            undo_kind: UndoKind::Inverse,
            payload: serde_json::json!({
                "src": src.to_str().unwrap(),
                "desired_sha256": desired_sha,
            }),
        };

        let ctx = make_ctx(&backups);
        let applied = handler.apply(&planned, &ctx).unwrap();

        // Target now exists with the correct sha.
        assert!(target_path.exists());
        assert_eq!(applied.after["exists"], true);
        assert_eq!(applied.after["sha256"], desired_sha);

        // Undo recipe is "delete".
        assert_eq!(applied.undo["action"], "delete");

        // Run undo: target must be deleted.
        let recorded = make_recorded(target_path.to_str().unwrap(), applied.undo);
        handler.undo(&recorded, &ctx).unwrap();
        assert!(
            !target_path.exists(),
            "undo must delete the file we created"
        );
    }

    #[test]
    fn undo_delete_is_idempotent() {
        let handler = DotfileHandler::new();
        let tmp = TempDir::new().unwrap();
        let backups = tmp.path().join("backups");
        let ctx = make_ctx(&backups);

        let ghost = tmp.path().join("nonexistent.txt");
        let recipe = serde_json::json!({
            "action": "delete",
            "target": ghost.to_str().unwrap(),
        });
        let recorded = make_recorded(ghost.to_str().unwrap(), recipe);

        // Calling undo twice on an already-absent file must not error.
        handler.undo(&recorded, &ctx).unwrap();
        handler.undo(&recorded, &ctx).unwrap();
    }

    // ── apply + undo (existing target — backup taken) ────────────────────────

    #[test]
    fn apply_over_existing_then_undo_restores_original_bytes() {
        let handler = DotfileHandler::new();
        let tmp = TempDir::new().unwrap();
        let backups = tmp.path().join("backups");

        let original_bytes = b"original content";
        let new_bytes = b"new content from src";

        // Target EXISTS with original content.
        let target_path = tmp.path().join("config.txt");
        std::fs::write(&target_path, original_bytes).unwrap();

        // Source file with new content.
        let src = tmp.path().join("new_src.txt");
        std::fs::write(&src, new_bytes).unwrap();
        let desired_sha = hex_sha256(new_bytes);

        let before_sha = hex_sha256(original_bytes);
        let before_probe = Probe(serde_json::json!({ "exists": true, "sha256": before_sha }));

        let planned = PlannedAction {
            handler: HandlerKind::Dotfile,
            target: Target(target_path.to_str().unwrap().to_owned()),
            before: before_probe.0.clone(),
            undo_kind: UndoKind::Inverse,
            payload: serde_json::json!({
                "src": src.to_str().unwrap(),
                "desired_sha256": desired_sha,
            }),
        };

        let ctx = make_ctx(&backups);
        let applied = handler.apply(&planned, &ctx).unwrap();

        // Target now has new content.
        let target_now = std::fs::read(&target_path).unwrap();
        assert_eq!(target_now, new_bytes);

        // After state reflects new sha.
        assert_eq!(applied.after["sha256"], desired_sha);

        // Undo recipe is "restore".
        assert_eq!(applied.undo["action"], "restore");
        let backup_str = applied.undo["backup"].as_str().unwrap();
        assert!(backup_str.ends_with(".bak"));

        // Run undo: target must be restored to original content EXACTLY.
        let recorded = make_recorded(target_path.to_str().unwrap(), applied.undo);
        handler.undo(&recorded, &ctx).unwrap();

        let restored = std::fs::read(&target_path).unwrap();
        assert_eq!(
            restored, original_bytes,
            "undo must restore the original bytes exactly"
        );
    }

    // ── CRITICAL #2: undo refuses on tampered backup ─────────────────────────

    #[test]
    fn undo_restore_refuses_tampered_backup() {
        let handler = DotfileHandler::new();
        let tmp = TempDir::new().unwrap();
        let backups = tmp.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        let ctx = make_ctx(&backups);

        let target_path = tmp.path().join("victim.txt");
        std::fs::write(&target_path, b"deployed content").unwrap();

        // Write a backup file and record its real sha256.
        let bak_path = backups.join("backup.bak");
        let good_bytes = b"original content";
        std::fs::write(&bak_path, good_bytes).unwrap();
        let good_sha = hex_sha256(good_bytes);

        // Now CORRUPT the backup.
        std::fs::write(&bak_path, b"tampered!").unwrap();

        let recipe = serde_json::json!({
            "action": "restore",
            "target": target_path.to_str().unwrap(),
            "backup": bak_path.to_str().unwrap(),
            "backup_sha256": good_sha, // recorded sha no longer matches file
        });
        let recorded = make_recorded(target_path.to_str().unwrap(), recipe);

        let result = handler.undo(&recorded, &ctx);
        assert!(
            result.is_err(),
            "undo must refuse to restore tampered backup"
        );

        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("backup integrity check failed"),
            "error message must mention integrity: {msg}"
        );

        // The target must NOT have been overwritten.
        let still_there = std::fs::read(&target_path).unwrap();
        assert_eq!(still_there, b"deployed content");
    }

    // ── synthesize_undo ───────────────────────────────────────────────────────

    #[test]
    fn synthesize_undo_before_absent_yields_delete_recipe() {
        let handler = DotfileHandler::new();
        let target = Target("/home/user/.config/tool/settings.toml".to_owned());

        // before: file did not exist.
        let before = serde_json::json!({ "exists": false });
        // Post-crash probe: file now exists.
        let post_crash_sha = hex_sha256(b"some content");
        let probe = Probe(serde_json::json!({ "exists": true, "sha256": post_crash_sha }));

        let applied = handler.synthesize_undo(&target, &before, &probe).unwrap();

        // after should reflect the observed probe state.
        assert_eq!(applied.after["exists"], true);

        // undo recipe must be "delete" for the target.
        assert_eq!(applied.undo["action"], "delete");
        assert_eq!(
            applied.undo["target"],
            "/home/user/.config/tool/settings.toml"
        );
    }

    #[test]
    fn synthesize_undo_before_present_yields_noop_recipe() {
        let handler = DotfileHandler::new();
        let target = Target("/home/user/.bashrc".to_owned());

        let original_sha = hex_sha256(b"original");
        let before = serde_json::json!({ "exists": true, "sha256": original_sha });
        let new_sha = hex_sha256(b"replaced");
        let probe = Probe(serde_json::json!({ "exists": true, "sha256": new_sha }));

        let applied = handler.synthesize_undo(&target, &before, &probe).unwrap();

        assert_eq!(applied.after["sha256"], new_sha);
        assert_eq!(applied.undo["action"], "noop");
        assert!(applied.undo["reason"]
            .as_str()
            .unwrap()
            .contains("cannot restore"));
    }

    // ── expand helper ─────────────────────────────────────────────────────────

    #[test]
    fn expand_tilde_prefix() {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return; // Skip if HOME is not set.
        }
        let result = expand("~/.config");
        assert_eq!(result, PathBuf::from(format!("{home}/.config")));
    }

    #[test]
    fn expand_dollar_home_prefix() {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return;
        }
        let result = expand("$HOME/.bashrc");
        assert_eq!(result, PathBuf::from(format!("{home}/.bashrc")));
    }

    #[test]
    fn expand_plain_path_unchanged() {
        let result = expand("/etc/hosts");
        assert_eq!(result, PathBuf::from("/etc/hosts"));
    }
}
