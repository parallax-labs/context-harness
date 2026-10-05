//! Durable, immutable artifact files and process-lifetime ownership of local runs.
//! Files are synced before their metadata is recorded in SQLite. A crash between
//! those steps can leave an unreferenced file; files are never overwritten or
//! implicitly removed. The persistent lock inode must never be unlinked.
use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MAX_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub(crate) struct RunOwnershipUnavailable;

impl std::fmt::Display for RunOwnershipUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("run already has an active owner")
    }
}

impl std::error::Error for RunOwnershipUnavailable {}

#[derive(Debug, Clone)]
pub struct ArtifactFile {
    pub relative_path: String,
    pub sha256: String,
    pub size: u64,
}

/// Owns an exclusive OS lock until dropped, including during unwinding.
#[derive(Debug)]
pub struct RunFiles {
    root: PathBuf,
    id: String,
    directory: std::fs::File,
    artifacts: std::fs::File,
    _lock: std::fs::File,
}

impl Drop for RunFiles {
    fn drop(&mut self) {
        // Release ownership before any of the held directory descriptors are
        // dropped. Relying on descriptor close alone proved racy when the same
        // run was reacquired immediately under Rust 1.99 on Linux.
        let _ = std::fs::File::unlock(&self._lock);
    }
}

pub fn acquire(root: &Path, id: &str) -> Result<RunFiles> {
    // Require the canonical UUID spelling: no filesystem syntax or aliases.
    ensure!(
        uuid::Uuid::parse_str(id)
            .map(|value| value.to_string() == id)
            .unwrap_or(false),
        "run ID must be a canonical UUID"
    );
    acquire_impl(root, id)
}

impl RunFiles {
    /// Create a unique immutable artifact; callers may record this metadata only
    /// after success. An error or crash can leave an unreferenced partial file.
    pub fn write_artifact(&self, name: &str, content: &[u8]) -> Result<ArtifactFile> {
        ensure!(
            !name.is_empty()
                && name.len() <= 80
                && name.as_bytes()[0].is_ascii_alphanumeric()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')),
            "artifact label must be 1 to 80 safe ASCII characters"
        );
        ensure!(
            content.len() <= MAX_ARTIFACT_BYTES,
            "artifact exceeds 16 MiB"
        );
        self.write_impl(name, content)
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::{
        ffi::CString,
        fs::{File, OpenOptions},
        io::Write,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
    };

    /// Every traversal is descriptor-relative and rejects symlinks atomically.
    /// Holding descriptors also prevents a renamed ancestor redirecting writes.
    fn child_directory(parent: &File, name: &str, create: bool) -> Result<File> {
        let name = CString::new(name)?;
        if create {
            // SAFETY: parent is live, name is NUL terminated, mode is valid.
            let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
            if result < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error).context("create run directory");
                }
            } else {
                parent.sync_all().context("sync run directory parent")?;
            }
        }
        // SAFETY: parent is live, name is NUL terminated, flags require a real dir.
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        ensure!(
            fd >= 0,
            "run path must contain only real directories: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: openat returned a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn directory(root: &Path, id: &str, create: bool) -> Result<File> {
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root)
            .context("open canonical workspace root")?;
        let ctx = child_directory(&root, ".ctx", create)?;
        let runs = child_directory(&ctx, "runs", create)?;
        child_directory(&runs, id, create)
    }

    fn child_file(directory: &File, name: &str, exclusive: bool) -> Result<File> {
        let name = CString::new(name)?;
        let flags = libc::O_RDWR
            | libc::O_CREAT
            | libc::O_NONBLOCK
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | if exclusive { libc::O_EXCL } else { 0 };
        // SAFETY: directory is live and name is NUL terminated; mode is valid.
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, 0o600) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error()).context("open runtime file");
        }
        // SAFETY: openat returned a new owned descriptor.
        let file = unsafe { File::from_raw_fd(fd) };
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file() && metadata.nlink() == 1,
            "runtime file must be a regular file without hard links"
        );
        Ok(file)
    }

    pub(super) fn acquire(root: &Path, id: &str) -> Result<RunFiles> {
        let root = root.canonicalize().context("resolve workspace root")?;
        let directory = directory(&root, id, true)?;
        let lock = child_file(&directory, ".lock", false)?;
        if let Err(error) = lock.try_lock() {
            match error {
                std::fs::TryLockError::WouldBlock => {
                    return Err(RunOwnershipUnavailable.into());
                }
                std::fs::TryLockError::Error(error) => {
                    return Err(error).context("acquire run ownership lock");
                }
            }
        }
        directory.sync_all().context("sync run lock directory")?;
        let artifacts = child_directory(&directory, "artifacts", true)?;
        let files = RunFiles {
            root,
            id: id.to_owned(),
            directory,
            artifacts,
            _lock: lock,
        };
        files.verify_directory()?;
        Ok(files)
    }

    impl RunFiles {
        fn verify_directory(&self) -> Result<()> {
            let current = directory(&self.root, &self.id, false)?.metadata()?;
            let held = self.directory.metadata()?;
            ensure!(
                current.dev() == held.dev() && current.ino() == held.ino(),
                "run directory changed while owned"
            );
            let current = child_directory(&self.directory, "artifacts", false)?.metadata()?;
            let held = self.artifacts.metadata()?;
            ensure!(
                current.dev() == held.dev() && current.ino() == held.ino(),
                "artifact directory changed while owned"
            );
            Ok(())
        }

        pub(super) fn write_impl(&self, name: &str, content: &[u8]) -> Result<ArtifactFile> {
            self.verify_directory()?;
            let filename = format!("{}-{name}", uuid::Uuid::new_v4());
            let mut file = child_file(&self.artifacts, &filename, true)?;
            file.write_all(content).context("write run artifact")?;
            file.sync_all().context("sync run artifact")?;
            self.artifacts
                .sync_all()
                .context("sync artifact directory")?;
            self.verify_directory()?;
            Ok(ArtifactFile {
                relative_path: format!(".ctx/runs/{}/artifacts/{filename}", self.id),
                sha256: format!("{:x}", Sha256::digest(content)),
                size: content.len() as u64,
            })
        }
    }
}

#[cfg(unix)]
fn acquire_impl(root: &Path, id: &str) -> Result<RunFiles> {
    unix::acquire(root, id)
}

#[cfg(not(unix))]
fn acquire_impl(_root: &Path, _id: &str) -> Result<RunFiles> {
    anyhow::bail!("secure run files are not supported on this platform")
}

#[cfg(not(unix))]
impl RunFiles {
    fn write_impl(&self, _name: &str, _content: &[u8]) -> Result<ArtifactFile> {
        anyhow::bail!("secure run files are not supported on this platform")
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
    };

    #[test]
    fn locks_conflict_and_release_without_removing_inode() {
        let root = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let first = acquire(root.path(), &id).unwrap();
        let error = acquire(root.path(), &id).unwrap_err();
        assert!(error.downcast_ref::<RunOwnershipUnavailable>().is_some());
        let lock = root.path().join(format!(".ctx/runs/{id}/.lock"));
        assert!(lock.is_file());
        drop(first);
        assert!(lock.is_file());
        let _second = acquire(root.path(), &id).unwrap();
    }

    #[test]
    fn immutable_artifact_bytes_digest_and_private_permissions() {
        let root = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let files = acquire(root.path(), &id).unwrap();
        let first = files.write_artifact("answer.txt", b"hello").unwrap();
        let second = files.write_artifact("answer.txt", b"world").unwrap();
        assert_ne!(first.relative_path, second.relative_path);
        assert_eq!(
            fs::read(root.path().join(&first.relative_path)).unwrap(),
            b"hello"
        );
        assert_eq!(first.size, 5);
        assert_eq!(
            first.sha256,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(
            fs::metadata(root.path().join(&first.relative_path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(root.path().join(format!(".ctx/runs/{id}")))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        for label in [
            "",
            "../escape",
            "/tmp/escape",
            "nested/name",
            ".lock",
            "a\\b",
        ] {
            assert!(files.write_artifact(label, b"no").is_err());
        }
        assert!(files
            .write_artifact("large", &vec![0; MAX_ARTIFACT_BYTES + 1])
            .is_err());
        assert!(acquire(root.path(), "../../escape").is_err());
    }

    #[test]
    fn rejects_symlinks_at_each_runtime_component() {
        for component in [".ctx", ".ctx/runs", ".ctx/runs/ID", ".ctx/runs/ID/.lock"] {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            let path = root.path().join(component.replace("ID", &id));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            symlink(outside.path(), &path).unwrap();
            assert!(acquire(root.path(), &id).is_err(), "{component}");
            assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
        }
    }

    #[test]
    fn rejects_artifact_directory_replacement_after_acquiring() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let files = acquire(root.path(), &id).unwrap();
        let run = root.path().join(format!(".ctx/runs/{id}"));
        fs::rename(run.join("artifacts"), run.join("old-artifacts")).unwrap();
        symlink(outside.path(), run.join("artifacts")).unwrap();
        assert!(files.write_artifact("answer", b"no").is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn rejects_ancestor_replacement_after_acquiring() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let files = acquire(root.path(), &id).unwrap();
        fs::rename(root.path().join(".ctx"), root.path().join("old-ctx")).unwrap();
        symlink(outside.path(), root.path().join(".ctx")).unwrap();
        assert!(files.write_artifact("answer", b"no").is_err());
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
