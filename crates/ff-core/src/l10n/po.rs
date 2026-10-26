//! A reader for gettext PO catalogs: the subset a translation file in
//! `lang/` uses.
//!
//! PO is the format translators already know (Poedit, Weblate, Lokalize,
//! any text editor), which is why the game reads it directly rather than a
//! format of its own. Supported: `msgid`/`msgstr` pairs with multi-line
//! strings and C escapes, comments, the `fuzzy` flag (a fuzzy entry is a
//! guess a translator has not confirmed, so it is skipped, as gettext does),
//! and `msgctxt` (kept on the entry; the game matches finished lines, not
//! call sites, so a catalog skips entries that carry one). Plural entries (`msgid_plural`) are skipped; plurals live
//! inside a translation as `{count:one|many}` instead (see `plural`).

/// One translated entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoEntry {
    /// The `msgctxt`, when the entry has one.
    pub msgctxt: Option<String>,
    pub msgid: String,
    pub msgstr: String,
    /// The `#,` flags, e.g. `fuzzy`.
    pub flags: Vec<String>,
    /// The line the entry's `msgid` starts on, for error messages.
    pub line: usize,
}

impl PoEntry {
    pub fn is_fuzzy(&self) -> bool {
        self.flags.iter().any(|flag| flag == "fuzzy")
    }
}

/// A PO file that could not be read, with the line it broke on.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("line {line}: {message}")]
pub struct PoError {
    pub line: usize,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    None,
    Ctxt,
    Id,
    IdPlural,
    Str,
    Ignored,
}

#[derive(Default)]
struct Pending {
    msgctxt: Option<String>,
    msgid: Option<String>,
    msgstr: Option<String>,
    flags: Vec<String>,
    plural: bool,
    line: usize,
}

fn flush(pending: &mut Pending, entries: &mut Vec<PoEntry>) {
    let done = std::mem::take(pending);
    if let (Some(msgid), Some(msgstr)) = (done.msgid, done.msgstr) {
        if !done.plural {
            entries.push(PoEntry {
                msgctxt: done.msgctxt,
                msgid,
                msgstr,
                flags: done.flags,
                line: done.line,
            });
        }
    }
}

/// Parse a whole PO file. The header entry (empty `msgid`) is returned like
/// any other; callers skip it.
pub fn parse(text: &str) -> Result<Vec<PoEntry>, PoError> {
    let mut entries = Vec::new();
    let mut pending = Pending::default();
    let mut field = Field::None;
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#,") {
            if pending.msgstr.is_some() {
                flush(&mut pending, &mut entries);
            }
            pending
                .flags
                .extend(rest.split(',').map(|f| f.trim().to_string()));
            field = Field::None;
            continue;
        }
        if line.starts_with('#') {
            // `#~` marks an obsolete entry; every other comment is notes.
            if line.starts_with("#~") {
                field = Field::Ignored;
            } else if pending.msgstr.is_some() {
                flush(&mut pending, &mut entries);
                field = Field::None;
            }
            continue;
        }
        let (keyword, rest) = match line.find(char::is_whitespace) {
            Some(at) if !line.starts_with('"') => (&line[..at], line[at..].trim_start()),
            _ => ("", line),
        };
        match keyword {
            "msgctxt" | "msgid" => {
                if pending.msgstr.is_some() || (keyword == "msgctxt" && pending.msgid.is_some()) {
                    flush(&mut pending, &mut entries);
                }
                let value = unquote(rest, line_no)?;
                if keyword == "msgid" {
                    pending.msgid = Some(value);
                    pending.line = line_no;
                    field = Field::Id;
                } else {
                    pending.msgctxt = Some(value);
                    field = Field::Ctxt;
                }
            }
            "msgid_plural" => {
                pending.plural = true;
                unquote(rest, line_no)?;
                field = Field::IdPlural;
            }
            _ if keyword == "msgstr" || keyword.starts_with("msgstr[") => {
                if pending.msgid.is_none() {
                    return Err(PoError {
                        line: line_no,
                        message: "msgstr without a msgid".into(),
                    });
                }
                let value = unquote(rest, line_no)?;
                if keyword == "msgstr" || keyword == "msgstr[0]" {
                    pending.msgstr = Some(value);
                    field = Field::Str;
                } else {
                    field = Field::Ignored;
                }
            }
            "" => {
                let value = unquote(rest, line_no)?;
                match field {
                    Field::Id => pending
                        .msgid
                        .get_or_insert_with(String::new)
                        .push_str(&value),
                    Field::Str => pending
                        .msgstr
                        .get_or_insert_with(String::new)
                        .push_str(&value),
                    Field::Ctxt => pending
                        .msgctxt
                        .get_or_insert_with(String::new)
                        .push_str(&value),
                    Field::IdPlural | Field::Ignored => {}
                    Field::None => {
                        return Err(PoError {
                            line: line_no,
                            message: "a string continues nothing".into(),
                        })
                    }
                }
            }
            other => {
                return Err(PoError {
                    line: line_no,
                    message: format!("unknown keyword {other:?}"),
                })
            }
        }
    }
    flush(&mut pending, &mut entries);
    Ok(entries)
}

/// The value of one `"..."` token, escapes decoded.
fn unquote(token: &str, line: usize) -> Result<String, PoError> {
    let err = |message: &str| PoError {
        line,
        message: message.to_string(),
    };
    let inner = token
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .filter(|_| token.len() >= 2)
        .ok_or_else(|| err("expected a quoted string"))?;
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            if c == '"' {
                return Err(err("unescaped quote inside a string"));
            }
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                return Err(err(&format!("unknown escape \\{other}")));
            }
            None => return Err(err("string ends in a backslash")),
        }
    }
    Ok(out)
}

/// A string as one PO `"..."` token (the inverse of `unquote`).
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// A field of a PO file's header (the empty `msgid`'s translation), such as
/// `Language` or `X-Language-Autonym`; `None` when the file has no header,
/// does not parse, or leaves the field out or blank.
pub fn header_field(text: &str, field: &str) -> Option<String> {
    let entries = parse(text).ok()?;
    let header = entries.iter().find(|entry| entry.msgid.is_empty())?;
    header.msgstr.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        let value = value.trim();
        (name.trim().eq_ignore_ascii_case(field) && !value.is_empty()).then(|| value.to_string())
    })
}
