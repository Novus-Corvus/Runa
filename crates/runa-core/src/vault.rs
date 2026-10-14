//! A vault: a folder of notes.
//!
//! Paths given to a vault are always **vault-relative with `/` separators**
//! (for example `"Gods/Odin.md"`). [`Vault::resolve`] turns one into a real
//! path and guarantees it cannot escape the vault, whether through `..`,
//! absolute paths, Windows reserved names, or symbolic links.
//!
//! Scanning never aborts on a bad entry: unreadable folders, odd file names
//! and skipped symbolic links are collected in the [`ScanReport`] so the UI can
//! show them while still displaying everything that worked.
//!
//! Known limits: "does the target exist?" checks before rename are not atomic
//! with the rename itself. Renaming does not yet rewrite links in other notes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::{ErrorKind, Result, RunaError};
use crate::fsio;

/// Name of Runa's private folder inside a vault (trash, rescue copies, settings).
pub const RUNA_DIR: &str = ".runa";

/// Deepest folder nesting the scanner will descend into.
pub const MAX_SCAN_DEPTH: usize = 64;

/// What kind of file a vault entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A Markdown note (`.md`).
    Note,
    /// Anything else (images, PDFs, ...).
    Attachment,
}

/// A file found in the vault.
#[derive(Debug, Clone)]
pub struct VaultEntry {
    /// Vault-relative path with `/` separators.
    pub rel_path: String,
    /// Note or attachment.
    pub kind: EntryKind,
    /// Size in bytes.
    pub size: u64,
    /// Last-modified time, if known.
    pub modified: Option<SystemTime>,
}

/// The result of scanning a vault.
#[derive(Debug, Default)]
pub struct ScanReport {
    /// Files, sorted case-insensitively by path.
    pub entries: Vec<VaultEntry>,
    /// Folders (vault-relative), sorted case-insensitively.
    pub folders: Vec<String>,
    /// Symbolic links that were not followed (vault-relative).
    pub skipped_links: Vec<String>,
    /// Things that could not be read. Show these to the person.
    pub problems: Vec<RunaError>,
}

/// An opened vault.
#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
}

impl Vault {
    /// Opens the folder at `root` as a vault.
    pub fn open(root: &Path) -> Result<Self> {
        let meta = fs::metadata(root)
            .map_err(|e| RunaError::from_io("open the vault folder", root, &e))?;
        if !meta.is_dir() {
            return Err(RunaError::new(
                ErrorKind::NotAVault,
                "A vault must be a folder, not a file",
            )
            .with_path(root)
            .with_suggestion("Choose the folder that contains your notes."));
        }
        let root = fs::canonicalize(root)
            .map_err(|e| RunaError::from_io("open the vault folder", root, &e))?;
        Ok(Self { root })
    }

    /// The vault's absolute root folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Converts a vault-relative path into a real path inside the vault.
    ///
    /// Rejects empty paths, `..`, characters that are illegal on common
    /// operating systems, Windows reserved names, the private `.runa` folder,
    /// and anything that would leave the vault through a symbolic link.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let mut out = self.root.clone();
        let mut components = 0usize;
        for part in rel.split(|c| c == '/' || c == '\\') {
            if part.is_empty() || part == "." {
                continue;
            }
            if components == 0 && part.eq_ignore_ascii_case(RUNA_DIR) {
                return Err(invalid(rel, "that folder is reserved for Runa's own use"));
            }
            if let Some(why) = component_problem(part) {
                return Err(invalid(rel, &why));
            }
            out.push(part);
            components += 1;
        }
        if components == 0 {
            return Err(invalid(rel, "the name is empty"));
        }
        self.ensure_inside(&out)?;
        Ok(out)
    }

    /// Converts an absolute path inside the vault to a vault-relative one.
    /// Returns `None` if the path is outside the vault or not valid UTF-8.
    pub fn relative(&self, absolute: &Path) -> Option<String> {
        let rest = absolute.strip_prefix(&self.root).ok()?;
        let mut parts = Vec::new();
        for component in rest.components() {
            parts.push(component.as_os_str().to_str()?);
        }
        Some(parts.join("/"))
    }

    /// Lists every note, attachment and folder. Never fails as a whole.
    pub fn scan(&self) -> ScanReport {
        let mut report = ScanReport::default();
        let mut stack: Vec<(PathBuf, String, usize)> = vec![(self.root.clone(), String::new(), 0)];

        while let Some((dir, rel_dir, depth)) = stack.pop() {
            let reader = match fs::read_dir(&dir) {
                Ok(reader) => reader,
                Err(e) => {
                    report
                        .problems
                        .push(RunaError::from_io("list the folder", &dir, &e));
                    continue;
                }
            };
            for item in reader {
                let entry = match item {
                    Ok(entry) => entry,
                    Err(e) => {
                        report
                            .problems
                            .push(RunaError::from_io("read a folder entry", &dir, &e));
                        continue;
                    }
                };
                let file_name = entry.file_name();
                let Some(name) = file_name.to_str() else {
                    report.problems.push(
                        RunaError::new(
                            ErrorKind::InvalidPath,
                            "A file name is not valid text and was skipped",
                        )
                        .with_path(entry.path())
                        .with_suggestion("Rename the file using a regular file manager."),
                    );
                    continue;
                };
                if name.starts_with('.') {
                    continue; // hidden: .git, .runa, .obsidian, our temp files...
                }
                let rel = if rel_dir.is_empty() {
                    name.to_string()
                } else {
                    format!("{rel_dir}/{name}")
                };
                let file_type = match entry.file_type() {
                    Ok(t) => t,
                    Err(e) => {
                        report.problems.push(RunaError::from_io(
                            "inspect a file",
                            &entry.path(),
                            &e,
                        ));
                        continue;
                    }
                };
                if file_type.is_symlink() {
                    report.skipped_links.push(rel);
                } else if file_type.is_dir() {
                    if depth >= MAX_SCAN_DEPTH {
                        report.problems.push(
                            RunaError::new(
                                ErrorKind::InvalidPath,
                                format!("Folders nested more than {MAX_SCAN_DEPTH} levels deep were skipped"),
                            )
                            .with_path(entry.path()),
                        );
                        continue;
                    }
                    report.folders.push(rel.clone());
                    stack.push((entry.path(), rel, depth + 1));
                } else if file_type.is_file() {
                    match entry.metadata() {
                        Ok(meta) => report.entries.push(VaultEntry {
                            kind: kind_of(name),
                            rel_path: rel,
                            size: meta.len(),
                            modified: meta.modified().ok(),
                        }),
                        Err(e) => report.problems.push(RunaError::from_io(
                            "inspect a file",
                            &entry.path(),
                            &e,
                        )),
                    }
                }
            }
        }

        report
            .entries
            .sort_by_cached_key(|e| (e.rel_path.to_lowercase(), e.rel_path.clone()));
        report
            .folders
            .sort_by_cached_key(|f| (f.to_lowercase(), f.clone()));
        report
    }

    /// Creates a new note (adding `.md` if missing) and any missing folders.
    /// Fails, without touching anything, if the note already exists.
    pub fn create_note(&self, rel: &str, initial_text: &str) -> Result<PathBuf> {
        let path = self.resolve(&with_md_extension(rel))?;
        ensure_parent(&path)?;
        fsio::write_new(&path, initial_text.as_bytes()).map_err(|e| {
            if e.kind() == ErrorKind::AlreadyExists {
                RunaError::new(
                    ErrorKind::AlreadyExists,
                    "A note with that name already exists",
                )
                .with_path(&path)
                .with_suggestion("Choose a different name, or open the existing note.")
            } else {
                e
            }
        })?;
        Ok(path)
    }

    /// Finds a free "Untitled", "Untitled 1", ... name inside `folder`
    /// (`""` for the vault root). Returns the vault-relative path.
    pub fn next_untitled(&self, folder: &str) -> Result<String> {
        let prefix = folder.trim_matches(|c| c == '/' || c == '\\');
        for n in 0..10_000u32 {
            let stem = if n == 0 {
                "Untitled".to_string()
            } else {
                format!("Untitled {n}")
            };
            let rel = if prefix.is_empty() {
                format!("{stem}.md")
            } else {
                format!("{prefix}/{stem}.md")
            };
            let path = self.resolve(&rel)?;
            match fs::symlink_metadata(&path) {
                Ok(_) => continue,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(rel),
                Err(e) => return Err(RunaError::from_io("check for a free name", &path, &e)),
            }
        }
        Err(RunaError::new(
            ErrorKind::AlreadyExists,
            "Could not find a free \"Untitled\" name",
        )
        .with_suggestion("Rename or delete some of the Untitled notes."))
    }

    /// Renames or moves a file. Refuses to overwrite an existing file.
    /// Returns the new absolute path.
    pub fn rename(&self, from_rel: &str, to_rel: &str) -> Result<PathBuf> {
        let from = self.resolve(from_rel)?;
        let to = self.resolve(to_rel)?;
        if let Err(e) = fs::symlink_metadata(&from) {
            return Err(RunaError::from_io("find the file to rename", &from, &e));
        }
        if fs::symlink_metadata(&to).is_ok() && !is_same_file(&from, &to) {
            return Err(RunaError::new(
                ErrorKind::AlreadyExists,
                "Something with the new name already exists",
            )
            .with_path(&to)
            .with_suggestion("Choose a different name."));
        }
        ensure_parent(&to)?;
        fs::rename(&from, &to).map_err(|e| RunaError::from_io("rename the file", &from, &e))?;
        Ok(to)
    }

    /// Moves a file into the vault's trash (`.runa/trash`) instead of deleting
    /// it, so it can always be recovered. Returns where it went.
    pub fn trash(&self, rel: &str) -> Result<PathBuf> {
        let source = self.resolve(rel)?;
        if let Err(e) = fs::symlink_metadata(&source) {
            return Err(RunaError::from_io("find the file to delete", &source, &e));
        }
        let trash_dir = self.root.join(RUNA_DIR).join("trash");
        fs::create_dir_all(&trash_dir)
            .map_err(|e| RunaError::from_io("prepare the trash folder", &trash_dir, &e))?;
        let flat = rel
            .trim_matches(|c| c == '/' || c == '\\')
            .replace(['/', '\\'], "__");
        let stamp = fsio::unix_seconds();
        for n in 0..1000u32 {
            let name = if n == 0 {
                format!("{stamp}-{flat}")
            } else {
                format!("{stamp}-{n}-{flat}")
            };
            let destination = trash_dir.join(name);
            if fs::symlink_metadata(&destination).is_ok() {
                continue;
            }
            fs::rename(&source, &destination)
                .map_err(|e| RunaError::from_io("move the file to the trash", &source, &e))?;
            return Ok(destination);
        }
        Err(RunaError::new(
            ErrorKind::AlreadyExists,
            "Could not find a free name in the trash",
        )
        .with_path(trash_dir))
    }

    fn ensure_inside(&self, path: &Path) -> Result<()> {
        for ancestor in path.ancestors() {
            match fs::canonicalize(ancestor) {
                Ok(real) => {
                    return if real.starts_with(&self.root) {
                        Ok(())
                    } else {
                        Err(outside(path))
                    };
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(RunaError::from_io("check the location", ancestor, &e)),
            }
        }
        Err(outside(path))
    }
}

fn outside(path: &Path) -> RunaError {
    RunaError::new(
        ErrorKind::InvalidPath,
        "That location is outside the vault (a shortcut or link leads out of it)",
    )
    .with_path(path)
}

fn invalid(rel: &str, why: &str) -> RunaError {
    RunaError::new(
        ErrorKind::InvalidPath,
        format!("\"{rel}\" is not a valid note path: {why}"),
    )
    .with_suggestion(
        "Use letters, numbers, spaces and normal punctuation, and \"/\" between folders.",
    )
}

fn component_problem(part: &str) -> Option<String> {
    if part == ".." {
        return Some("\"..\" is not allowed".to_string());
    }
    if part.len() > 255 {
        return Some("a name is longer than 255 bytes".to_string());
    }
    if let Some(bad) = part
        .chars()
        .find(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return Some(format!(
            "the character {bad:?} is not allowed (it does not work on every operating system)"
        ));
    }
    if part.ends_with('.') || part.ends_with(' ') {
        return Some("names cannot end with a dot or a space".to_string());
    }
    let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        return Some(format!("\"{stem}\" is a reserved name on Windows"));
    }
    None
}

fn kind_of(file_name: &str) -> EntryKind {
    match file_name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.eq_ignore_ascii_case("md") => EntryKind::Note,
        _ => EntryKind::Attachment,
    }
}

fn with_md_extension(rel: &str) -> String {
    if rel.to_ascii_lowercase().ends_with(".md") {
        rel.to_string()
    } else {
        format!("{rel}.md")
    }
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| RunaError::from_io("create the folder", parent, &e))?;
    }
    Ok(())
}

/// True if both paths point at the same file (e.g. a case-only rename on a
/// case-insensitive file system).
fn is_same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn vault(tag: &str) -> (TempDir, Vault) {
        let dir = TempDir::new(tag);
        let vault = Vault::open(dir.path()).unwrap();
        (dir, vault)
    }

    #[test]
    fn open_rejects_missing_and_files() {
        let dir = TempDir::new("vault-open");
        assert_eq!(
            Vault::open(&dir.path().join("nope")).unwrap_err().kind(),
            ErrorKind::NotFound
        );
        let file = dir.path().join("f.md");
        fs::write(&file, "x").unwrap();
        assert_eq!(Vault::open(&file).unwrap_err().kind(), ErrorKind::NotAVault);
    }

    #[test]
    fn resolve_rejects_escapes_and_bad_names() {
        let (_dir, vault) = vault("vault-resolve");
        for bad in [
            "",
            "   /  ",
            "../x.md",
            "a/../../x.md",
            "a/..",
            "CON.md",
            "nul",
            "a/lpt1.txt",
            "bad:name.md",
            "q?.md",
            "trail.",
            ".runa/trash/x",
            ".RUNA",
            "tab\there.md",
        ] {
            let err = vault.resolve(bad).unwrap_err();
            assert_eq!(
                err.kind(),
                ErrorKind::InvalidPath,
                "{bad:?} should be rejected"
            );
        }
        assert!(vault.resolve("Gods/Odin.md").is_ok());
        assert!(vault.resolve("Gods\\Odin.md").is_ok());
        assert!(vault.resolve("Ásgarðr/Þór.md").is_ok());
    }

    #[test]
    fn resolve_rejects_absolute_looking_paths() {
        let (_dir, vault) = vault("vault-abs");
        // Leading slashes are ignored, so this stays inside the vault.
        let path = vault.resolve("/etc/passwd").unwrap();
        assert!(path.starts_with(vault.root()));
        // A drive prefix contains ':' and is rejected.
        assert!(vault.resolve("C:/Windows/x.md").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_symlink_escape() {
        let (_dir, vault) = vault("vault-link");
        let outside_dir = TempDir::new("vault-link-outside");
        std::os::unix::fs::symlink(outside_dir.path(), vault.root().join("portal")).unwrap();
        let err = vault.resolve("portal/secret.md").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidPath);
    }

    #[test]
    fn scan_lists_sorted_notes_attachments_and_skips_hidden() {
        let (_dir, vault) = vault("vault-scan");
        fs::create_dir_all(vault.root().join("Gods")).unwrap();
        fs::create_dir_all(vault.root().join(".git")).unwrap();
        fs::write(vault.root().join("b.md"), "b").unwrap();
        fs::write(vault.root().join("A.md"), "a").unwrap();
        fs::write(vault.root().join("Gods/Odin.md"), "o").unwrap();
        fs::write(vault.root().join("raven.png"), [1, 2, 3]).unwrap();
        fs::write(vault.root().join(".git/config"), "x").unwrap();
        fs::write(vault.root().join(".hidden.md"), "x").unwrap();

        let report = vault.scan();
        let paths: Vec<_> = report.entries.iter().map(|e| e.rel_path.as_str()).collect();
        assert_eq!(paths, vec!["A.md", "b.md", "Gods/Odin.md", "raven.png"]);
        assert_eq!(report.folders, vec!["Gods".to_string()]);
        assert_eq!(report.entries[3].kind, EntryKind::Attachment);
        assert_eq!(report.entries[0].kind, EntryKind::Note);
        assert!(report.problems.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn scan_reports_symlinks_instead_of_following_them() {
        let (_dir, vault) = vault("vault-scan-link");
        std::os::unix::fs::symlink(vault.root(), vault.root().join("loop")).unwrap();
        let report = vault.scan();
        assert_eq!(report.skipped_links, vec!["loop".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn scan_reports_unreadable_folders_and_keeps_going() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, vault) = vault("vault-scan-perm");
        let locked = vault.root().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::write(vault.root().join("ok.md"), "x").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let report = vault.scan();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        // Running as root bypasses permissions; only assert when it applied.
        if !report.problems.is_empty() {
            assert_eq!(report.problems[0].kind(), ErrorKind::PermissionDenied);
        }
        assert!(report.entries.iter().any(|e| e.rel_path == "ok.md"));
    }

    #[test]
    fn create_note_adds_extension_makes_folders_and_never_overwrites() {
        let (_dir, vault) = vault("vault-create");
        let path = vault.create_note("Gods/Thor", "# Thor").unwrap();
        assert!(path.ends_with("Thor.md"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "# Thor");
        let err = vault.create_note("Gods/Thor.md", "other").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).unwrap(), "# Thor");
    }

    #[test]
    fn next_untitled_skips_taken_names() {
        let (_dir, vault) = vault("vault-untitled");
        assert_eq!(vault.next_untitled("").unwrap(), "Untitled.md");
        vault.create_note("Untitled", "").unwrap();
        assert_eq!(vault.next_untitled("").unwrap(), "Untitled 1.md");
        assert_eq!(vault.next_untitled("Sub").unwrap(), "Sub/Untitled.md");
    }

    #[test]
    fn rename_refuses_to_overwrite_and_moves_otherwise() {
        let (_dir, vault) = vault("vault-rename");
        vault.create_note("a", "A").unwrap();
        vault.create_note("b", "B").unwrap();
        let err = vault.rename("a.md", "b.md").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::AlreadyExists);
        let moved = vault.rename("a.md", "Folder/c.md").unwrap();
        assert_eq!(fs::read_to_string(moved).unwrap(), "A");
        assert_eq!(
            vault.rename("ghost.md", "x.md").unwrap_err().kind(),
            ErrorKind::NotFound
        );
    }

    #[test]
    fn trash_moves_into_runa_trash_and_is_recoverable() {
        let (_dir, vault) = vault("vault-trash");
        let path = vault.create_note("Gods/Loki", "trickster").unwrap();
        let trashed = vault.trash("Gods/Loki.md").unwrap();
        assert!(!path.exists());
        assert!(trashed.starts_with(vault.root().join(RUNA_DIR).join("trash")));
        assert_eq!(fs::read_to_string(trashed).unwrap(), "trickster");
        // The private folder is hidden from scans.
        assert!(vault.scan().entries.is_empty());
    }

    #[test]
    fn relative_roundtrips() {
        let (_dir, vault) = vault("vault-rel");
        let path = vault.create_note("Gods/Odin", "").unwrap();
        assert_eq!(vault.relative(&path).as_deref(), Some("Gods/Odin.md"));
        assert_eq!(vault.relative(Path::new("/definitely/elsewhere")), None);
    }

    #[test]
    fn kind_detection() {
        assert_eq!(kind_of("a.md"), EntryKind::Note);
        assert_eq!(kind_of("A.MD"), EntryKind::Note);
        assert_eq!(kind_of(".md"), EntryKind::Attachment);
        assert_eq!(kind_of("a.png"), EntryKind::Attachment);
        assert_eq!(kind_of("noext"), EntryKind::Attachment);
    }
}
