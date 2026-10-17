//! Platform paths for Sigrun's config, data, and SSH locations.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Resolved filesystem locations. `~/.ssh` is discovery-only; Sigrun never
/// creates, modifies, or deletes anything in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub ssh_dir: PathBuf,
}

impl Paths {
    /// Resolves paths from the environment overrides (`SIGRUN_CONFIG_DIR`,
    /// `SIGRUN_DATA_DIR`, `SIGRUN_SSH_DIR`) or platform defaults.
    pub fn resolve() -> Result<Self> {
        let config_dir = match std::env::var_os("SIGRUN_CONFIG_DIR") {
            Some(v) => PathBuf::from(v),
            None => dirs::config_dir()
                .ok_or_else(|| Error::Config("cannot determine config directory".into()))?
                .join(app_folder()),
        };
        let data_dir = match std::env::var_os("SIGRUN_DATA_DIR") {
            Some(v) => PathBuf::from(v),
            None => dirs::data_dir()
                .ok_or_else(|| Error::Config("cannot determine data directory".into()))?
                .join(app_folder()),
        };
        let ssh_dir = match std::env::var_os("SIGRUN_SSH_DIR") {
            Some(v) => PathBuf::from(v),
            None => dirs::home_dir()
                .ok_or_else(|| Error::Config("cannot determine home directory".into()))?
                .join(".ssh"),
        };
        Ok(Self {
            config_dir,
            data_dir,
            ssh_dir,
        })
    }

    pub fn with_roots(config_dir: PathBuf, data_dir: PathBuf, ssh_dir: PathBuf) -> Self {
        Self {
            config_dir,
            data_dir,
            ssh_dir,
        }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.toml")
    }

    pub fn data_file(&self) -> PathBuf {
        self.data_dir.join("data.toml")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.data_dir.join("data.lock")
    }

    /// The host's single-instance guard. Deliberately distinct from
    /// [`Paths::lock_file`] (the store's per-write lock) so the long-lived host
    /// never contends with its own store writes.
    pub fn host_lock_file(&self) -> PathBuf {
        self.data_dir.join("host.lock")
    }

    pub fn vault_file(&self) -> PathBuf {
        self.data_dir.join("vault.json")
    }

    /// Host IPC socket: `data_dir/host.sock` on Unix, a per-user named pipe on
    /// Windows.
    pub fn socket_path(&self) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(format!(r"\\.\pipe\sigrun-{}", pipe_user()))
        } else {
            self.data_dir.join("host.sock")
        }
    }

    /// Agent IPC socket: `data_dir/agent.sock` on Unix, a per-user named pipe
    /// on Windows.
    pub fn agent_socket(&self) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(format!(r"\\.\pipe\sigrun-agent-{}", pipe_user()))
        } else {
            self.data_dir.join("agent.sock")
        }
    }
}

fn app_folder() -> &'static str {
    if cfg!(target_os = "linux") {
        "sigrun"
    } else {
        "Sigrun"
    }
}

/// Local username for Windows named pipes, restricted to `[A-Za-z0-9_-]`.
fn pipe_user() -> String {
    let raw =
        std::env::var("USERNAME").unwrap_or_else(|_| std::env::var("USER").unwrap_or_default());
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Creates `path` and all parents; mode `0700` on Unix.
pub fn ensure_private_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Writes `data` to `path` via a temp file in the same directory, fsyncs, sets
/// `mode` on Unix, then atomically replaces `path`.
pub fn write_atomic(path: &Path, data: &[u8], mode: u32) -> Result<()> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let mut file = tempfile::NamedTempFile::new_in(&dir)?;
    file.write_all(data)?;
    file.as_file().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(mode))?;
    }
    #[cfg(windows)]
    let _ = mode;
    // PersistError's io::Error wraps paths only, never file contents.
    file.persist(path).map_err(|e| Error::Io(e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn with_roots_builds_expected_files() {
        let p = Paths::with_roots("/c".into(), "/d".into(), "/h/.ssh".into());
        // Join expected file names onto the roots so the separator is
        // platform-neutral (Windows joins with `\`).
        let (c, d) = (PathBuf::from("/c"), PathBuf::from("/d"));
        assert_eq!(p.settings_file(), c.join("settings.toml"));
        assert_eq!(p.data_file(), d.join("data.toml"));
        assert_eq!(p.vault_file(), d.join("vault.json"));
        assert_eq!(p.lock_file(), d.join("data.lock"));
    }

    #[test]
    fn env_overrides_are_used() {
        std::env::set_var("SIGRUN_CONFIG_DIR", "/tmp/c");
        std::env::set_var("SIGRUN_DATA_DIR", "/tmp/d");
        std::env::set_var("SIGRUN_SSH_DIR", "/tmp/ssh");
        let p = Paths::resolve().unwrap();
        assert_eq!(p.config_dir, PathBuf::from("/tmp/c"));
        assert_eq!(p.ssh_dir, PathBuf::from("/tmp/ssh"));
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_sets_mode_and_replaces() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.json");
        write_atomic(&path, b"one", 0o600).unwrap();
        write_atomic(&path, b"two", 0o600).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
