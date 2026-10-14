//! # runa-core
//!
//! Everything Runa does with notes that is not drawing pixels:
//!
//! * [`vault`]: open a folder of notes, list it, create / rename / trash notes.
//! * [`fsio`]: crash-safe reading and writing of files.
//! * [`note`]: parse Markdown notes for front matter, headings, links and tags.
//! * [`index`]: resolve `[[wikilinks]]`, compute backlinks and unresolved links.
//! * [`search`]: find text across notes.
//! * [`rescue`]: last-resort copies of text that could not be saved.
//! * [`error`]: the one error type, written for humans.
//!
//! This crate contains no UI code and never panics: the workspace lints
//! deny `unwrap`, `expect`, `panic!` and unchecked indexing outside tests.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod error;
pub mod fsio;
pub mod index;
pub mod note;
pub mod rescue;
pub mod search;
pub mod vault;

#[cfg(test)]
mod testutil;

pub use error::{ErrorKind, Result, RunaError};
