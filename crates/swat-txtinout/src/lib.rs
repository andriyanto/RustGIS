//! L2: SWAT+'s TxtInOut files.
//!
//! [`read`] makes a [`Project`] of a scenario's files (bytes, from a folder or an archive: the
//! caller reads them, so this works in the browser too) and [`write`] gives the same bytes back
//! for a file nobody edited. [`cio`] reads `file.cio`, the index of the scenario's files, and
//! [`view`] what a file's lines are: its title, headers and records, nested where a record owns
//! the lines after it (a soil's layers, a schedule's operations, a decision table's conditions).
//!
//! The formats are written from their description (the SWAT+ input documentation and the files
//! the SWAT+ Editor writes), not from SWAT+'s source.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod cio;
pub mod view;

use rustgis_swat_model::{Encoding, Project, TextFile};

/// A file's bytes as text: UTF-8, or else one character a byte (Latin-1).
pub fn decode(bytes: &[u8]) -> TextFile {
    match std::str::from_utf8(bytes) {
        Ok(s) => TextFile::from_text(s),
        Err(_) => {
            let s: String = bytes.iter().map(|&b| char::from(b)).collect();
            let mut f = TextFile::from_text(&s);
            f.encoding = Encoding::Latin1;
            f
        }
    }
}

/// A file's text as the bytes it was read from (characters outside Latin-1 as `?` in a Latin-1
/// file).
pub fn encode(f: &TextFile) -> Vec<u8> {
    let s = f.to_text();
    match f.encoding {
        Encoding::Utf8 => s.into_bytes(),
        Encoding::Latin1 => s.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect(),
    }
}

/// A project of files given as (name, bytes): every file of the scenario folder, names relative
/// to it with `/` between folders.
pub fn read<N: Into<String>, B: AsRef<[u8]>>(files: impl IntoIterator<Item = (N, B)>) -> Project {
    let mut p = Project::new();
    for (name, bytes) in files {
        p.files.insert(name.into().replace('\\', "/"), decode(bytes.as_ref()));
    }
    p
}

/// The project's files as (name, bytes).
pub fn write(p: &Project) -> Vec<(String, Vec<u8>)> {
    p.files.iter().map(|(n, f)| (n.clone(), encode(f))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_round_trip() {
        let files = [("a.txt", b"x\r\ny\r\n".to_vec()), ("b.txt", vec![b'n', 0xe9, b'\n']), ("c.txt", "ä\n".as_bytes().to_vec())];
        let p = read(files.iter().map(|(n, b)| (*n, b.clone())));
        let out = write(&p);
        for ((n, b), (n2, b2)) in files.iter().zip(&out) {
            assert_eq!((*n, b), (n2.as_str(), b2));
        }
    }
}
