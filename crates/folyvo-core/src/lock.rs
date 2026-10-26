//! One vault, several processes: the desktop app and the command line may open the same vault.
//! Every open holds a lock on `<vault>/lock`. Only a process that finds the vault otherwise closed
//! may sweep leftovers, because an object another process is writing has no database row yet.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

use crate::error::{CoreError, Result};

/// Held for as long as the vault is open.
#[derive(Debug)]
pub(crate) struct VaultLock {
    file: File,
}

impl VaultLock {
    /// Lock `vault`. `alone` is true when no other process has it open: the caller may sweep,
    /// then must call [`VaultLock::share`].
    pub(crate) fn acquire(vault: &Path) -> Result<(VaultLock, bool)> {
        let path = vault.join("lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| CoreError::io(&path, e))?;
        let alone = match file.try_lock() {
            Ok(()) => true,
            Err(TryLockError::WouldBlock) => {
                file.lock_shared().map_err(|e| CoreError::io(&path, e))?;
                false
            }
            Err(TryLockError::Error(e)) => return Err(CoreError::io(&path, e)),
        };
        Ok((VaultLock { file }, alone))
    }

    /// Let other processes open the vault too. Call once the exclusive work is done.
    pub(crate) fn share(&self) -> Result<()> {
        self.file.unlock().map_err(|e| CoreError::io("lock", e))?;
        self.file.lock_shared().map_err(|e| CoreError::io("lock", e))
    }
}
