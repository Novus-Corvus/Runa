//! Last-resort copies of text that could not be saved where it belongs.
//!
//! When a save fails and the person chooses "Save a copy", or when the app is
//! about to close with unsaved changes it could not write, [`rescue_copy`]
//! puts the text somewhere safe. It tries the vault's own `.runa/rescue`
//! folder first, then the operating system's temporary folder. It reports
//! exactly where the text went, or, if both fail, why, so the person can copy
//! the text out of the editor by hand.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{ErrorKind, Result, RunaError};
use crate::fsio;
use crate::vault::RUNA_DIR;

/// Writes `content` to a new, uniquely named file and returns its path.
///
/// `name_hint` (usually the note's name) is sanitised and used in the file
/// name. Existing files are never overwritten.
pub fn rescue_copy(vault_root: Option<&Path>, name_hint: &str, content: &str) -> Result<PathBuf> {
    let mut folders = Vec::new();
    if let Some(root) = vault_root {
        folders.push(root.join(RUNA_DIR).join("rescue"));
    }
    folders.push(std::env::temp_dir().join("runa-rescue"));

    let stem = sanitize(name_hint);
    let stamp = fsio::unix_seconds();
    let mut failures: Vec<String> = Vec::new();

    for folder in &folders {
        if let Err(e) = fs::create_dir_all(folder) {
            failures.push(format!("{}: {e}", folder.display()));
            continue;
        }
        for n in 0..1000u32 {
            let name = if n == 0 {
                format!("{stamp}-{stem}.md")
            } else {
                format!("{stamp}-{stem}-{n}.md")
            };
            let path = folder.join(name);
            match fsio::write_new(&path, content.as_bytes()) {
                Ok(()) => return Ok(path),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
                Err(e) => {
                    failures.push(e.to_string());
                    break;
                }
            }
        }
    }

    let mut error = RunaError::new(
        ErrorKind::Io,
        "Could not save a rescue copy anywhere",
    )
    .with_suggestion("Select all the text in the editor, copy it, and paste it into another program to keep it safe.");
    if !failures.is_empty() {
        error = error.with_detail(failures.join("; "));
    }
    Err(error)
}

fn sanitize(hint: &str) -> String {
    let cleaned: String = hint
        .trim_end_matches(".md")
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "note".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn writes_into_the_vault_rescue_folder_without_overwriting() {
        let dir = TempDir::new("rescue-vault");
        let first = rescue_copy(Some(dir.path()), "Odin.md", "one").unwrap();
        let second = rescue_copy(Some(dir.path()), "Odin.md", "two").unwrap();
        assert_ne!(first, second);
        assert!(first.starts_with(dir.path().join(".runa").join("rescue")));
        assert_eq!(fs::read_to_string(first).unwrap(), "one");
        assert_eq!(fs::read_to_string(second).unwrap(), "two");
    }

    #[test]
    fn falls_back_to_temp_when_the_vault_is_unusable() {
        let dir = TempDir::new("rescue-fallback");
        // A file where the folder should be makes the vault location unusable.
        let fake_root = dir.path().join("not-a-folder");
        fs::write(&fake_root, "x").unwrap();
        let path = rescue_copy(Some(&fake_root), "A/B:C", "saved anyway").unwrap();
        assert!(path.starts_with(std::env::temp_dir().join("runa-rescue")));
        assert_eq!(fs::read_to_string(&path).unwrap(), "saved anyway");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn hostile_names_are_sanitised() {
        assert_eq!(sanitize("../../etc/passwd"), "______etc_passwd");
        assert_eq!(sanitize(""), "note");
        assert_eq!(sanitize(".md"), "note");
        assert!(sanitize(&"x".repeat(500)).len() <= 64);
    }
}
