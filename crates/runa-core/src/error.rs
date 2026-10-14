//! The one error type used across Runa.
//!
//! Every failure is a [`RunaError`]: something a person can read, understand,
//! and act on. Each error carries
//!
//! * a **summary** (one sentence, what went wrong),
//! * the **path** involved (if any),
//! * the technical **detail** from the operating system (if any),
//! * a list of **suggestions** (what the person can do about it).
//!
//! The UI is expected to show all four. Nothing in this crate panics or
//! swallows an error; if something fails, it comes back as one of these.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Result alias used throughout Runa.
pub type Result<T> = std::result::Result<T, RunaError>;

/// Broad category of a failure. Lets the UI pick an icon, decide whether to
/// offer "Retry", "Save a copy…", and so on, without parsing text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The file or folder does not exist (any more).
    NotFound,
    /// The operating system refused access.
    PermissionDenied,
    /// Something with that name already exists.
    AlreadyExists,
    /// Another program has the file open or locked.
    FileInUse,
    /// No space left on the device (or quota exceeded).
    DiskFull,
    /// The file is not valid UTF-8 text.
    InvalidEncoding,
    /// The file is larger than Runa is willing to open as a note.
    TooLarge,
    /// The file changed on disk since Runa loaded it.
    Conflict,
    /// A path or file name is not allowed (unsafe, reserved, malformed).
    InvalidPath,
    /// The chosen folder cannot be used as a vault.
    NotAVault,
    /// Any other input/output failure.
    Io,
}

/// A failure, described so that a person can act on it.
#[derive(Debug, Clone)]
pub struct RunaError {
    kind: ErrorKind,
    summary: String,
    detail: Option<String>,
    path: Option<PathBuf>,
    suggestions: Vec<String>,
}

impl RunaError {
    /// Creates an error with a one-sentence summary.
    pub fn new(kind: ErrorKind, summary: impl Into<String>) -> Self {
        Self {
            kind,
            summary: summary.into(),
            detail: None,
            path: None,
            suggestions: Vec::new(),
        }
    }

    /// Attaches the file or folder involved.
    #[must_use]
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Attaches technical detail (usually the operating system's message).
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Adds one thing the person can try.
    #[must_use]
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    /// The failure category.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// One sentence describing what went wrong.
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Technical detail, if any.
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// The file or folder involved, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Things the person can try, most useful first.
    pub fn suggestions(&self) -> &[String] {
        &self.suggestions
    }

    /// Builds an error from an [`io::Error`].
    ///
    /// `action` completes the sentence "Could not …", for example
    /// `"read the note"`.
    pub fn from_io(action: &str, path: &Path, err: &io::Error) -> Self {
        let kind = classify_io(err);
        let summary = match kind {
            ErrorKind::NotFound => {
                format!("Could not {action}: the file or folder does not exist")
            }
            ErrorKind::PermissionDenied => {
                format!("Could not {action}: permission was denied")
            }
            ErrorKind::AlreadyExists => {
                format!("Could not {action}: something with that name already exists")
            }
            ErrorKind::FileInUse => {
                format!("Could not {action}: another program is using the file")
            }
            ErrorKind::DiskFull => format!("Could not {action}: the disk is full"),
            _ => format!("Could not {action}"),
        };
        let mut error = Self::new(kind, summary)
            .with_path(path)
            .with_detail(err.to_string());
        for suggestion in io_suggestions(kind) {
            error = error.with_suggestion(*suggestion);
        }
        error
    }

    /// A multi-line, human-readable rendering for dialogs and logs.
    pub fn report(&self) -> String {
        let mut out = self.summary.clone();
        if let Some(path) = &self.path {
            out.push_str(&format!("\nLocation: {}", path.display()));
        }
        if let Some(detail) = &self.detail {
            out.push_str(&format!("\nDetails: {detail}"));
        }
        if !self.suggestions.is_empty() {
            out.push_str("\n\nWhat you can do:");
            for suggestion in &self.suggestions {
                out.push_str(&format!("\n  - {suggestion}"));
            }
        }
        out
    }
}

impl fmt::Display for RunaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(path) => write!(f, "{} ({})", self.summary, path.display()),
            None => write!(f, "{}", self.summary),
        }
    }
}

impl std::error::Error for RunaError {}

fn classify_io(err: &io::Error) -> ErrorKind {
    match err.kind() {
        io::ErrorKind::NotFound => return ErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => return ErrorKind::PermissionDenied,
        io::ErrorKind::AlreadyExists => return ErrorKind::AlreadyExists,
        _ => {}
    }
    match err.raw_os_error() {
        // Windows: 32 sharing violation, 33 lock violation, 39/112 disk full.
        Some(32 | 33) if cfg!(windows) => ErrorKind::FileInUse,
        Some(39 | 112) if cfg!(windows) => ErrorKind::DiskFull,
        // Unix: 28 ENOSPC, 122 EDQUOT.
        Some(28 | 122) if !cfg!(windows) => ErrorKind::DiskFull,
        _ => ErrorKind::Io,
    }
}

fn io_suggestions(kind: ErrorKind) -> &'static [&'static str] {
    match kind {
        ErrorKind::NotFound => &[
            "Check that the file or folder was not moved, renamed or deleted.",
            "If it lives on an external drive or network share, make sure it is connected.",
        ],
        ErrorKind::PermissionDenied => &[
            "Check that you have permission to change this file and its folder.",
            "Check that the file is not marked read-only.",
        ],
        ErrorKind::AlreadyExists => &["Choose a different name."],
        ErrorKind::FileInUse => &[
            "Close any other program that has this file open (sync tools and virus scanners can briefly lock files).",
            "Wait a moment and try again.",
        ],
        ErrorKind::DiskFull => &[
            "Free some space on the disk, then try again.",
            "Save a copy to a different drive in the meantime.",
        ],
        _ => &["Try again. If it keeps happening, save a copy of your work elsewhere."],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_not_found_is_classified_with_suggestions() {
        let io_err = io::Error::from(io::ErrorKind::NotFound);
        let err = RunaError::from_io("read the note", Path::new("a.md"), &io_err);
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert!(!err.suggestions().is_empty());
        assert!(err.summary().contains("read the note"));
    }

    #[test]
    fn report_contains_every_part() {
        let err = RunaError::new(ErrorKind::Io, "Could not do it")
            .with_path("x/y.md")
            .with_detail("os said no")
            .with_suggestion("try again");
        let report = err.report();
        assert!(report.contains("Could not do it"));
        assert!(report.contains("y.md"));
        assert!(report.contains("os said no"));
        assert!(report.contains("try again"));
    }

    #[test]
    fn display_mentions_path() {
        let err = RunaError::new(ErrorKind::Io, "Boom").with_path("n.md");
        assert_eq!(err.to_string(), "Boom (n.md)");
    }
}
