//! Read and write the gettext PO files in `lang/`.
//!
//! The same subset the game's reader (`ff_core::l10n::po`) understands:
//! single `msgid`/`msgstr` pairs, multi-line strings, C escapes, `#.` notes
//! from the tools, `#` notes from translators, `#:` references, `#,` flags
//! and `#~` obsolete entries.

use crate::util::{py_rstrip, py_splitlines, py_strip};

const WRAP: usize = 76;

#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub msgid: String,
    pub msgstr: String,
    /// gettext's `msgctxt`: tells apart two entries with the same English.
    pub msgctxt: Option<String>,
    pub refs: Vec<String>,
    pub comments: Vec<String>,
    pub translator_comments: Vec<String>,
    pub flags: Vec<String>,
    pub obsolete: bool,
    pub line: usize,
}

impl Entry {
    #[cfg(test)]
    pub fn new(msgid: &str) -> Self {
        Entry {
            msgid: msgid.to_string(),
            ..Entry::default()
        }
    }

    pub fn key(&self) -> (Option<String>, String) {
        (self.msgctxt.clone(), self.msgid.clone())
    }

    pub fn fuzzy(&self) -> bool {
        self.flags.iter().any(|f| f == "fuzzy")
    }
}

fn unquote(token: &str, line: usize) -> Result<String, String> {
    let token = py_strip(token);
    let chars: Vec<char> = token.chars().collect();
    if chars.len() < 2 || chars[0] != '"' || chars[chars.len() - 1] != '"' {
        return Err(format!(
            "line {line}: expected a quoted string, found {}",
            crate::l10n::check::py_repr(token)
        ));
    }
    let body = &chars[1..chars.len() - 1];
    let mut out = String::new();
    let mut i = 0;
    while i < body.len() {
        let c = body[i];
        if c == '\\' {
            let Some(&next) = body.get(i + 1) else {
                return Err(format!("line {line}: string ends in a backslash"));
            };
            out.push(match next {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '"' => '"',
                '\\' => '\\',
                other => return Err(format!("line {line}: unknown escape \\{other}")),
            });
            i += 2;
            continue;
        }
        if c == '"' {
            return Err(format!("line {line}: unescaped quote inside a string"));
        }
        out.push(c);
        i += 1;
    }
    Ok(out)
}

fn quote(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    )
}

#[derive(PartialEq, Clone, Copy)]
enum Field {
    None,
    Msgctxt,
    Msgid,
    Msgstr,
}

pub fn parse_po(text: &str) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    let mut cur: Option<Entry> = None;
    let mut pending = Entry::default();
    let mut field = Field::None;

    fn finish(entries: &mut Vec<Entry>, cur: &mut Option<Entry>, pending: &mut Entry) {
        if let Some(entry) = cur.take() {
            entries.push(entry);
        }
        *pending = Entry::default();
    }

    for (index, raw) in py_splitlines(text).into_iter().enumerate() {
        let number = index + 1;
        let mut line = py_strip(raw);
        let mut obsolete = false;
        if let Some(rest) = line.strip_prefix("#~") {
            obsolete = true;
            line = py_strip(rest);
            if line.is_empty() {
                continue;
            }
        }
        if line.is_empty() {
            if cur.is_some() && field == Field::Msgstr {
                finish(&mut entries, &mut cur, &mut pending);
            }
            continue;
        }
        if line.starts_with('#') && !obsolete {
            if cur.is_some() && field == Field::Msgstr {
                finish(&mut entries, &mut cur, &mut pending);
            }
            if let Some(rest) = line.strip_prefix("#.") {
                pending.comments.push(py_strip(rest).to_string());
            } else if let Some(rest) = line.strip_prefix("#:") {
                pending.refs.extend(
                    rest.split(crate::util::py_isspace)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                );
            } else if let Some(rest) = line.strip_prefix("#,") {
                pending.flags.extend(
                    rest.split(',')
                        .map(py_strip)
                        .filter(|f| !f.is_empty())
                        .map(str::to_string),
                );
            } else if line.starts_with("#|") {
            } else {
                pending
                    .translator_comments
                    .push(py_strip(&line[1..]).to_string());
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgctxt ") {
            if cur.is_some() {
                finish(&mut entries, &mut cur, &mut pending);
            }
            pending.msgctxt = Some(unquote(rest, number)?);
            field = Field::Msgctxt;
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgid ") {
            if cur.is_some() {
                finish(&mut entries, &mut cur, &mut pending);
            }
            let mut entry = std::mem::take(&mut pending);
            entry.msgid = unquote(rest, number)?;
            entry.line = number;
            entry.obsolete = obsolete;
            cur = Some(entry);
            field = Field::Msgid;
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgstr ") {
            let Some(entry) = cur.as_mut() else {
                return Err(format!("line {number}: msgstr without a msgid"));
            };
            entry.msgstr = unquote(rest, number)?;
            field = Field::Msgstr;
            continue;
        }
        if line.starts_with('"') {
            if field == Field::Msgctxt {
                let value = unquote(line, number)?;
                pending
                    .msgctxt
                    .get_or_insert_with(String::new)
                    .push_str(&value);
                continue;
            }
            let (Some(entry), true) = (cur.as_mut(), field != Field::None) else {
                return Err(format!("line {number}: a string continues nothing"));
            };
            let value = unquote(line, number)?;
            if field == Field::Msgid {
                entry.msgid.push_str(&value);
            } else {
                entry.msgstr.push_str(&value);
            }
            continue;
        }
        return Err(format!(
            "line {number}: cannot read {}",
            crate::l10n::check::py_repr(line)
        ));
    }
    finish(&mut entries, &mut cur, &mut pending);
    Ok(entries)
}

/// Python's `str.splitlines(keepends=True)`.
fn splitlines_keepends(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut iter = s.char_indices().peekable();
    while let Some((pos, c)) = iter.next() {
        let is_break = matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if is_break {
            let mut end = pos + c.len_utf8();
            if c == '\r' && iter.peek().is_some_and(|&(_, n)| n == '\n') {
                iter.next();
                end += 1;
            }
            out.push(&s[start..end]);
            start = end;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// `msgid "..."`, wrapped the way gettext tools wrap long strings.
fn string_lines(keyword: &str, text: &str, prefix: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let inner_newline = chars.len() > 1 && chars[..chars.len() - 1].contains(&'\n');
    if chars.len() + keyword.len() + 3 <= WRAP && !inner_newline {
        return vec![format!("{prefix}{keyword} {}", quote(text))];
    }
    let mut chunks: Vec<String> = Vec::new();
    for part in splitlines_keepends(text) {
        let mut part: Vec<char> = part.chars().collect();
        while part.len() > WRAP {
            let cut = match part[..WRAP].iter().rposition(|&c| c == ' ') {
                Some(p) if p > 0 => p + 1,
                _ => WRAP,
            };
            chunks.push(part[..cut].iter().collect());
            part = part[cut..].to_vec();
        }
        if !part.is_empty() {
            chunks.push(part.iter().collect());
        }
    }
    let mut lines = vec![format!("{prefix}{keyword} \"\"")];
    lines.extend(chunks.iter().map(|c| format!("{prefix}{}", quote(c))));
    lines
}

pub fn write_po(entries: &[Entry], refs: bool) -> String {
    let mut blocks = Vec::new();
    for entry in entries {
        let mut lines: Vec<String> = Vec::new();
        for note in &entry.translator_comments {
            lines.push(py_rstrip(&format!("# {note}")).to_string());
        }
        for note in &entry.comments {
            lines.push(format!("#. {note}"));
        }
        if refs && !entry.refs.is_empty() {
            lines.push(format!("#: {}", entry.refs.join(" ")));
        }
        if !entry.flags.is_empty() {
            lines.push(format!("#, {}", entry.flags.join(", ")));
        }
        let prefix = if entry.obsolete { "#~ " } else { "" };
        if let Some(ctxt) = &entry.msgctxt {
            lines.extend(string_lines("msgctxt", ctxt, prefix));
        }
        lines.extend(string_lines("msgid", &entry.msgid, prefix));
        lines.extend(string_lines("msgstr", &entry.msgstr, prefix));
        blocks.push(lines.join("\n"));
    }
    blocks.join("\n\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_wrapped_and_obsolete_entries() {
        let long = "word ".repeat(30).trim_end().to_string();
        let mut entry = Entry::new(&long);
        entry.msgstr = "two\nlines".into();
        entry.comments.push("placeholders: {x}".into());
        entry.refs.push("crates/a.rs".into());
        let mut gone = Entry::new("Old");
        gone.msgstr = "Viejo".into();
        gone.obsolete = true;
        let text = write_po(&[entry, gone], true);
        assert!(text.contains("msgid \"\"\n\"word word"));
        let back = parse_po(&text).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].msgid, long);
        assert_eq!(back[0].msgstr, "two\nlines");
        assert_eq!(back[0].refs, ["crates/a.rs"]);
        assert!(back[1].obsolete);
        assert_eq!(back[1].msgstr, "Viejo");
    }

    #[test]
    fn bad_escapes_are_refused() {
        assert!(parse_po("msgid \"a\\q\"\nmsgstr \"\"\n").is_err());
        assert!(parse_po("msgstr \"x\"\n").is_err());
    }
}
