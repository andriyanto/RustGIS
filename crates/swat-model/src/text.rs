//! A text file kept as it was read: its lines, line endings and encoding.

use crate::EditError;

/// How a file's bytes became text, to write the same bytes back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    #[default]
    Utf8,
    /// Not valid UTF-8: each byte a character (Latin-1), as old Windows tools wrote.
    Latin1,
}

/// A value on a line: its byte range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub start: usize,
    pub end: usize,
}

/// A text file: its lines without their endings, which of them end in `\r\n` (files mixing
/// endings keep them), and whether the last line has an ending.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextFile {
    lines: Vec<String>,
    /// The ending of new lines: the first line's.
    crlf: bool,
    /// Each line's ending is `\r\n`.
    cr: Vec<bool>,
    final_newline: bool,
    pub encoding: Encoding,
}

fn is_space(c: char) -> bool {
    c == ' ' || c == '\t'
}

impl TextFile {
    /// The file of text `s` (its line endings found from the first line).
    pub fn from_text(s: &str) -> Self {
        let crlf = s.find('\n').is_some_and(|i| i > 0 && s.as_bytes().get(i - 1) == Some(&b'\r'));
        let final_newline = s.ends_with('\n');
        let body = if final_newline { &s[..s.len() - 1] } else { s };
        let (mut lines, mut cr) = (Vec::new(), Vec::new());
        if !s.is_empty() {
            let parts: Vec<&str> = body.split('\n').collect();
            let n = parts.len();
            for (k, l) in parts.into_iter().enumerate() {
                // A last line without an ending keeps its `\r`.
                let ended = k + 1 < n || final_newline;
                match l.strip_suffix('\r').filter(|_| ended) {
                    Some(t) => {
                        lines.push(t.to_string());
                        cr.push(true);
                    }
                    None => {
                        lines.push(l.to_string());
                        cr.push(false);
                    }
                }
            }
        }
        TextFile { lines, crlf, cr, final_newline, encoding: Encoding::Utf8 }
    }

    /// The text again, with the endings it was read with.
    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(self.lines.iter().map(|l| l.len() + 2).sum());
        let n = self.lines.len();
        for (k, l) in self.lines.iter().enumerate() {
            out.push_str(l);
            if k + 1 < n || self.final_newline {
                out.push_str(if self.cr.get(k).copied().unwrap_or(self.crlf) { "\r\n" } else { "\n" });
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn line(&self, i: usize) -> Option<&str> {
        self.lines.get(i).map(String::as_str)
    }

    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().map(String::as_str)
    }

    /// The values on line `i`, separated by spaces and tabs.
    pub fn tokens(&self, i: usize) -> Vec<Token> {
        let Some(l) = self.lines.get(i) else { return Vec::new() };
        let mut out = Vec::new();
        let mut start = None;
        for (k, c) in l.char_indices() {
            match (is_space(c), start) {
                (false, None) => start = Some(k),
                (true, Some(s)) => {
                    out.push(Token { start: s, end: k });
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            out.push(Token { start: s, end: l.len() });
        }
        out
    }

    /// The text of value `k` of line `i`.
    pub fn token(&self, i: usize, k: usize) -> Option<&str> {
        let t = *self.tokens(i).get(k)?;
        self.lines.get(i).and_then(|l| l.get(t.start..t.end))
    }

    /// The values of line `i` as text.
    pub fn values(&self, i: usize) -> Vec<&str> {
        let Some(l) = self.lines.get(i) else { return Vec::new() };
        self.tokens(i).into_iter().filter_map(|t| l.get(t.start..t.end)).collect()
    }

    /// Replaces value `k` of line `i` with `value`, keeping the columns: a number keeps its right
    /// edge (numbers are right-aligned), any other value its left edge; a longer value takes the
    /// spaces beside it, keeping one, before it moves the rest of the line.
    pub fn set_token(&mut self, i: usize, k: usize, value: &str) -> Result<(), EditError> {
        if value.is_empty() || value.chars().any(|c| is_space(c) || c == '\n' || c == '\r') {
            return Err(EditError::NotAToken(value.to_string()));
        }
        let toks = self.tokens(i);
        let line = self.lines.get(i).ok_or(EditError::NoLine(i))?;
        let t = *toks.get(k).ok_or(EditError::NoToken { line: i, token: k })?;
        let old_len = t.end - t.start;
        let numeric = line[t.start..t.end].parse::<f64>().is_ok();
        let n = value.len();
        let spaces = |s: &str| s.len() - s.trim_end_matches(is_space).len();
        let mut out = line[..t.start].to_string();
        if numeric {
            if n <= old_len {
                out.push_str(&" ".repeat(old_len - n));
            } else {
                // Take the spaces before it, keeping one after a previous value.
                let keep = usize::from(k > 0);
                let take = (n - old_len).min(spaces(&out).saturating_sub(keep));
                out.truncate(out.len() - take);
            }
            out.push_str(value);
        } else {
            out.push_str(value);
            if n < old_len {
                out.push_str(&" ".repeat(old_len - n));
            }
        }
        // What follows: if the value grew past its old end, take the spaces after it, keeping
        // one before a next value.
        let rest = &line[t.end..];
        let lead = rest.len() - rest.trim_start_matches(is_space).len();
        let over = out.len().saturating_sub(t.end);
        let keep = usize::from(k + 1 < toks.len());
        let take = over.min(lead.saturating_sub(keep));
        out.push_str(&rest[take..]);
        self.lines[i] = out;
        Ok(())
    }

    /// Replaces line `i`.
    pub fn set_line(&mut self, i: usize, text: &str) -> Result<(), EditError> {
        let l = self.lines.get_mut(i).ok_or(EditError::NoLine(i))?;
        *l = text.replace(['\n', '\r'], " ");
        Ok(())
    }

    /// Inserts a line before line `i` (at the end when `i` is the number of lines).
    pub fn insert_line(&mut self, i: usize, text: &str) -> Result<(), EditError> {
        if i > self.lines.len() {
            return Err(EditError::NoLine(i));
        }
        self.lines.insert(i, text.replace(['\n', '\r'], " "));
        self.cr.insert(i.min(self.cr.len()), self.crlf);
        Ok(())
    }

    /// Removes line `i`.
    pub fn remove_line(&mut self, i: usize) -> Result<String, EditError> {
        if i >= self.lines.len() {
            return Err(EditError::NoLine(i));
        }
        if i < self.cr.len() {
            self.cr.remove(i);
        }
        Ok(self.lines.remove(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for s in ["", "a\n", "a\r\nb\r\n", "a\nb", "t\n\n  1  2\n", "a\r\nb\nc\r\n", "a\r", "\r\n"] {
            assert_eq!(TextFile::from_text(s).to_text(), s);
        }
    }

    #[test]
    fn edit_keeps_columns() {
        let mut f = TextFile::from_text("name          a       b\nhru001     0.050   1.000\n");
        f.set_token(1, 1, "0.12345").ok();
        assert_eq!(f.line(1), Some("hru001   0.12345   1.000"));
        f.set_token(1, 0, "x").ok();
        assert_eq!(f.line(1), Some("x        0.12345   1.000"));
        f.set_token(1, 2, "12345678901").ok();
        assert_eq!(f.line(1), Some("x        0.12345 12345678901"));
        f.set_token(1, 0, "a_very_long_name").ok();
        assert_eq!(f.token(1, 0), Some("a_very_long_name"));
        assert_eq!(f.values(1), vec!["a_very_long_name", "0.12345", "12345678901"]);
        assert!(f.set_token(1, 3, "1").is_err());
        assert!(f.set_token(1, 0, "a b").is_err());
    }
}
