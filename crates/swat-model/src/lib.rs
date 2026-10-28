//! L1: a SWAT+ project for editing.
//!
//! A project is its TxtInOut files, each kept as the text it was read from ([`TextFile`]), so a
//! project that is read and written again is the same bytes, and an edit changes only the value
//! edited. SWAT+ reads its files as whitespace-separated values (Fortran list-directed input),
//! so a file is lines of tokens; [`TextFile::set_token`] replaces one while keeping the columns
//! aligned. What each file's lines mean (titles, headers, records, nested records) is read by
//! `rustgis-swat-txtinout`; this crate has no I/O.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::BTreeMap;

mod text;

pub use text::{Encoding, TextFile, Token};

/// Why an edit was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// No such file in the project.
    NoFile(String),
    /// No such line in the file.
    NoLine(usize),
    /// No such token on the line.
    NoToken { line: usize, token: usize },
    /// A value must be one token: not empty, without whitespace.
    NotAToken(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::NoFile(n) => write!(f, "no file {n} in the project"),
            EditError::NoLine(l) => write!(f, "no line {}", l + 1),
            EditError::NoToken { line, token } => write!(f, "no value {} on line {}", token + 1, line + 1),
            EditError::NotAToken(t) => write!(f, "{t:?} is not a single value"),
        }
    }
}

impl std::error::Error for EditError {}

/// A SWAT+ project: its files by name (as `file.cio` and the other files name them, relative
/// to the project folder, `/` separated).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    pub files: BTreeMap<String, TextFile>,
}

impl Project {
    pub fn new() -> Self {
        Self::default()
    }

    /// The file named `name`. SWAT+ names are case-sensitive, as on Linux.
    pub fn file(&self, name: &str) -> Option<&TextFile> {
        self.files.get(name)
    }

    pub fn file_mut(&mut self, name: &str) -> Option<&mut TextFile> {
        self.files.get_mut(name)
    }

    /// Replaces value `token` (0-based) of line `line` (0-based) of file `name`.
    pub fn set_token(&mut self, name: &str, line: usize, token: usize, value: &str) -> Result<(), EditError> {
        self.files.get_mut(name).ok_or_else(|| EditError::NoFile(name.to_string()))?.set_token(line, token, value)
    }
}
