use anyhow::Context;
use nix::fcntl::{Flock, FlockArg};
use std::fs::File;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

const LOCK_NAME: &str = "litecrazy-tray";

/// Holds an exclusive file lock, released on drop.
pub struct LockGuard {
    _lock: Flock<File>,
}

/// Take the single-instance lock, failing immediately if another tray holds it.
pub fn acquire_instance_lock() -> anyhow::Result<LockGuard> {
    acquire_named(LOCK_NAME)
        .map_err(|_| anyhow::anyhow!("Another litecrazy instance is already running"))
}

fn acquire_named(name: &str) -> anyhow::Result<LockGuard> {
    let file = open_lock_file(name)?;
    let lock = Flock::lock(file, FlockArg::LockExclusiveNonblock)
        .map_err(|_| anyhow::anyhow!("Lock '{name}' is already held"))?;
    Ok(LockGuard { _lock: lock })
}

/// True if a tray is already running in another process.
pub fn instance_is_running() -> bool {
    acquire_instance_lock().is_err()
}

fn open_lock_file(name: &str) -> anyhow::Result<File> {
    let path = lock_path(name);
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("Failed to open lock file: {}", path.display()))
}

/// Prefer `$XDG_RUNTIME_DIR` (tmpfs, per-user, cleaned on logout) and fall
/// back to a uid-suffixed path in `/tmp` so two users on one machine dont
/// collide.
fn lock_path(name: &str) -> PathBuf {
    let uid = nix::unistd::Uid::effective().as_raw();
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{uid}")));

    if dir.is_dir() {
        dir.join(format!("{name}.lock"))
    } else {
        PathBuf::from(format!("/tmp/{name}-{uid}.lock"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_path_is_absolute_and_suffixed() {
        let path = lock_path(LOCK_NAME);
        assert!(path.is_absolute());
        assert!(path.to_string_lossy().ends_with(".lock"));
    }

    #[test]
    fn lock_is_exclusive() {
        let name = format!("litecrazy-test-{}", std::process::id());
        let _first = acquire_named(&name).expect("first lock should succeed");
        // flock is per-open-file-description, so a second open in the same
        // process contends just like a second process would.
        assert!(acquire_named(&name).is_err());
        let _ = std::fs::remove_file(lock_path(&name));
    }
}
