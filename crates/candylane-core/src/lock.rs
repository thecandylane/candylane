//! Single-writer lockfile. Two `candylane` processes mutating the same state DB at once
//! would corrupt it; this guarantees only one runs at a time.
//!
//! Fail-fast, not blocking: if another process holds the lock we return an error rather
//! than wait. The lock is an OS advisory file lock (`fs2`/`flock`), which the kernel
//! releases automatically when the holding process exits — even on a crash — so a stale
//! lock after a crash is impossible and no PID liveness check is needed.

use std::fs::{File, OpenOptions};
use std::path::Path;

use fs2::FileExt;

use crate::Result;

/// An acquired single-writer lock. Holding the value holds the lock; dropping it (end of
/// scope, or process exit) releases it.
pub struct Lock {
    // The lock lives for as long as this file handle is open. Never read directly —
    // its existence IS the lock.
    _file: File,
}

impl Lock {
    /// Acquire the exclusive lock at `<dir>/lock`, creating `dir` if needed. Returns an
    /// error immediately if another `candylane` process already holds it.
    pub fn acquire(dir: &Path) -> Result<Lock> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("lock");
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        file.try_lock_exclusive().map_err(|_| {
            anyhow::anyhow!(
                "another candylane process is already running (lock held at {})",
                path.display()
            )
        })?;
        Ok(Lock { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn second_acquire_fails_while_first_is_held() {
        let tmp = TempDir::new().unwrap();
        let first = Lock::acquire(tmp.path()).unwrap();
        // A second acquire while the first is held must fail fast.
        let second = Lock::acquire(tmp.path());
        assert!(second.is_err(), "second lock acquire must fail while held");
        drop(first);
        // Once released, acquiring again succeeds.
        let third = Lock::acquire(tmp.path());
        assert!(third.is_ok(), "lock must be re-acquirable after release");
    }
}
