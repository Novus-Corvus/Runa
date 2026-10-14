//! Safe file reading and writing.
//!
//! The rules this module enforces:
//!
//! 1. **A save never leaves a half-written note.** Content goes to a temporary
//!    file in the same folder, is flushed to disk, and only then replaces the
//!    original with a single rename. If anything fails, the original is
//!    untouched and the temporary file is removed.
//! 2. **A save never silently overwrites someone else's change.**
//!    [`save_checked`] refuses if the file changed on disk since it was loaded.
//! 3. **Read-only means read-only.** Saving over a read-only file is refused.
//! 4. **Odd files are reported, not mangled.** Non-UTF-8 files load with a
//!    `lossy` flag so the UI can warn before the person saves over the original.
//!
//! Known limits, deliberately documented rather than hidden: the conflict check
//! and the rename are two steps, so another program could still change the file
//! in the few microseconds between them. Directory flushing (for power-loss
//! durability) is best-effort and only done on Unix.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{ErrorKind, Result, RunaError};

/// Largest file Runa will open as a note (64 MiB).
pub const MAX_NOTE_BYTES: u64 = 64 * 1024 * 1024;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A cheap fingerprint of a file on disk, used to detect outside changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskStamp {
    /// Last-modified time, if the platform reports one.
    pub modified: Option<SystemTime>,
    /// Size in bytes.
    pub len: u64,
}

impl DiskStamp {
    /// Reads the current stamp of `path`.
    pub fn of(path: &Path) -> Result<Self> {
        let meta =
            fs::metadata(path).map_err(|e| RunaError::from_io("inspect the file", path, &e))?;
        Ok(Self {
            modified: meta.modified().ok(),
            len: meta.len(),
        })
    }
}

/// A note's text as loaded from disk.
#[derive(Debug, Clone)]
pub struct LoadedText {
    /// The text. If `lossy` is true, undecodable bytes were replaced with U+FFFD.
    pub text: String,
    /// What the file looked like when it was read.
    pub stamp: DiskStamp,
    /// True if the file was not valid UTF-8. Saving would permanently alter it,
    /// so the UI should warn first.
    pub lossy: bool,
}

/// Seconds since the Unix epoch (0 if the clock is before 1970).
pub fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Reads a text file, refusing folders and oversized files.
pub fn read_text(path: &Path) -> Result<LoadedText> {
    let meta = fs::metadata(path).map_err(|e| RunaError::from_io("read the note", path, &e))?;
    if !meta.is_file() {
        return Err(RunaError::new(ErrorKind::InvalidPath, "This is not a file")
            .with_path(path)
            .with_suggestion("Choose a note file instead of a folder."));
    }
    if meta.len() > MAX_NOTE_BYTES {
        return Err(RunaError::new(
            ErrorKind::TooLarge,
            format!(
                "This file is {} MiB, which is larger than the {} MiB Runa opens as a note",
                meta.len() / (1024 * 1024),
                MAX_NOTE_BYTES / (1024 * 1024)
            ),
        )
        .with_path(path)
        .with_suggestion("Open it in a dedicated editor, or split it into smaller notes."));
    }
    let stamp = DiskStamp {
        modified: meta.modified().ok(),
        len: meta.len(),
    };
    let bytes = fs::read(path).map_err(|e| RunaError::from_io("read the note", path, &e))?;
    match String::from_utf8(bytes) {
        Ok(text) => Ok(LoadedText {
            text,
            stamp,
            lossy: false,
        }),
        Err(err) => Ok(LoadedText {
            text: String::from_utf8_lossy(err.as_bytes()).into_owned(),
            stamp,
            lossy: true,
        }),
    }
}

/// Atomically replaces (or creates) `path` with `bytes`.
///
/// Returns the new [`DiskStamp`]. On failure the original file is untouched.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<DiskStamp> {
    let target = follow_symlink(path)?;
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let name = target.file_name().ok_or_else(|| {
        RunaError::new(ErrorKind::InvalidPath, "The path does not name a file").with_path(&target)
    })?;

    let existing_permissions = fs::metadata(&target).ok().map(|m| m.permissions());
    if let Some(perms) = &existing_permissions {
        if perms.readonly() {
            return Err(write_hint(
                RunaError::new(ErrorKind::PermissionDenied, "The file is read-only")
                    .with_path(&target)
                    .with_suggestion(
                        "Clear the read-only attribute, or save a copy under another name.",
                    ),
            ));
        }
    }

    let temp = parent.join(format!(
        ".{}.runa-tmp-{}-{}",
        name.to_string_lossy(),
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    let outcome = write_temp_then_rename(&temp, &target, bytes, existing_permissions);
    if outcome.is_err() {
        // Best effort: a leftover temp file is hidden and harmless.
        let _ = fs::remove_file(&temp);
    }
    outcome?;
    sync_directory(&parent);
    DiskStamp::of(&target)
}

/// Saves `bytes` to `path` only if the file is still as it was when loaded.
///
/// Returns a [`ErrorKind::Conflict`] error if the file was modified or deleted
/// outside Runa. The caller can then let the person choose between keeping
/// their version ([`atomic_write`]), reloading, or saving a copy.
pub fn save_checked(path: &Path, bytes: &[u8], expected: &DiskStamp) -> Result<DiskStamp> {
    match DiskStamp::of(path) {
        Ok(now) if now == *expected => atomic_write(path, bytes),
        Ok(_) => Err(write_hint(
            RunaError::new(
                ErrorKind::Conflict,
                "This note was changed outside Runa since you opened it",
            )
            .with_path(path)
            .with_suggestion("Save a copy of your version, then compare it with the file on disk.")
            .with_suggestion("Or choose to overwrite the file with your version."),
        )),
        Err(e) if e.kind() == ErrorKind::NotFound => Err(write_hint(
            RunaError::new(
                ErrorKind::Conflict,
                "This note was deleted or moved outside Runa since you opened it",
            )
            .with_path(path)
            .with_suggestion("Save it again to recreate the file, or save a copy elsewhere."),
        )),
        Err(e) => Err(e),
    }
}

/// Creates a new file with `bytes`, failing if it already exists.
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| RunaError::from_io("create the file", path, &e))?;
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| RunaError::from_io("write the file", path, &e));
    drop(file);
    if written.is_err() {
        let _ = fs::remove_file(path);
    }
    written
}

fn write_temp_then_rename(
    temp: &Path,
    target: &Path,
    bytes: &[u8],
    permissions: Option<fs::Permissions>,
) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)
        .map_err(|e| write_hint(RunaError::from_io("create a temporary file", temp, &e)))?;
    file.write_all(bytes)
        .map_err(|e| write_hint(RunaError::from_io("write the note", temp, &e)))?;
    file.sync_all()
        .map_err(|e| write_hint(RunaError::from_io("flush the note to disk", temp, &e)))?;
    drop(file);
    if let Some(perms) = permissions {
        // Best effort: keeps e.g. Unix mode bits. Failing only means the file
        // gets default permissions; the content is what matters.
        let _ = fs::set_permissions(temp, perms);
    }
    fs::rename(temp, target)
        .map_err(|e| write_hint(RunaError::from_io("replace the note", target, &e)))
}

/// Reassures the person that their text still exists in the editor.
fn write_hint(error: RunaError) -> RunaError {
    error.with_suggestion(
        "Your text has not been lost: it is still open in Runa. Use \"Save a copy\" to keep it safe while you fix the problem.",
    )
}

fn follow_symlink(path: &Path) -> Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => fs::canonicalize(path)
            .map_err(|e| RunaError::from_io("follow the link to the note", path, &e)),
        _ => Ok(path.to_path_buf()),
    }
}

#[cfg(unix)]
fn sync_directory(dir: &Path) {
    if let Ok(handle) = fs::File::open(dir) {
        let _ = handle.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_directory(_dir: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn atomic_write_creates_and_replaces() {
        let dir = TempDir::new("fsio-write");
        let path = dir.path().join("a.md");
        atomic_write(&path, b"one").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "one");
        atomic_write(&path, b"two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
    }

    #[test]
    fn atomic_write_leaves_no_temp_files() {
        let dir = TempDir::new("fsio-temp");
        let path = dir.path().join("a.md");
        atomic_write(&path, b"x").unwrap();
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.md".to_string()]);
    }

    #[test]
    fn failed_write_keeps_original_and_cleans_up() {
        let dir = TempDir::new("fsio-fail");
        let path = dir.path().join("a.md");
        atomic_write(&path, b"original").unwrap();
        // Make the target a directory so the final rename must fail.
        let blocked = dir.path().join("blocked");
        fs::create_dir(&blocked).unwrap();
        let err = atomic_write(&blocked, b"nope").unwrap_err();
        assert!(!err.suggestions().is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        let leftovers = fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("runa-tmp")
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    // set_readonly(false) makes a Unix file world-writable; harmless for a
    // throwaway file in a temp directory, and needed to clean up on Windows.
    #[allow(clippy::permissions_set_readonly_false)]
    #[test]
    fn read_only_file_is_refused() {
        let dir = TempDir::new("fsio-ro");
        let path = dir.path().join("a.md");
        fs::write(&path, "keep").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&path, perms).unwrap();
        let err = atomic_write(&path, b"change").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::PermissionDenied);
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep");
        // Restore so the temp dir can be cleaned up on Windows.
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(false);
        fs::set_permissions(&path, perms).unwrap();
    }

    #[test]
    fn save_checked_detects_outside_change() {
        let dir = TempDir::new("fsio-conflict");
        let path = dir.path().join("a.md");
        fs::write(&path, "v1").unwrap();
        let loaded = read_text(&path).unwrap();
        // Someone else changes the file (different length guarantees a new stamp).
        fs::write(&path, "v2 changed elsewhere").unwrap();
        let err = save_checked(&path, b"mine", &loaded.stamp).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Conflict);
        assert_eq!(fs::read_to_string(&path).unwrap(), "v2 changed elsewhere");
    }

    #[test]
    fn save_checked_detects_deletion() {
        let dir = TempDir::new("fsio-deleted");
        let path = dir.path().join("a.md");
        fs::write(&path, "v1").unwrap();
        let loaded = read_text(&path).unwrap();
        fs::remove_file(&path).unwrap();
        let err = save_checked(&path, b"mine", &loaded.stamp).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Conflict);
    }

    #[test]
    fn save_checked_succeeds_when_unchanged() {
        let dir = TempDir::new("fsio-ok");
        let path = dir.path().join("a.md");
        fs::write(&path, "v1").unwrap();
        let loaded = read_text(&path).unwrap();
        let stamp = save_checked(&path, b"v2", &loaded.stamp).unwrap();
        assert_eq!(stamp.len, 2);
    }

    #[test]
    fn invalid_utf8_loads_lossy_and_flags_it() {
        let dir = TempDir::new("fsio-utf8");
        let path = dir.path().join("a.md");
        fs::write(&path, [b'h', b'i', 0xFF, 0xFE]).unwrap();
        let loaded = read_text(&path).unwrap();
        assert!(loaded.lossy);
        assert!(loaded.text.starts_with("hi"));
    }

    #[test]
    fn reading_a_folder_or_missing_file_is_an_error() {
        let dir = TempDir::new("fsio-bad");
        assert_eq!(
            read_text(dir.path()).unwrap_err().kind(),
            ErrorKind::InvalidPath
        );
        assert_eq!(
            read_text(&dir.path().join("nope.md")).unwrap_err().kind(),
            ErrorKind::NotFound
        );
    }

    #[test]
    fn write_new_refuses_to_overwrite() {
        let dir = TempDir::new("fsio-new");
        let path = dir.path().join("a.md");
        write_new(&path, b"first").unwrap();
        let err = write_new(&path, b"second").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).unwrap(), "first");
    }
}
