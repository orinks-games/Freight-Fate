//! `l10n-extract`: rebuild the translation template and bring every
//! language file up to date.
//!
//! ```text
//! cargo tools l10n-extract           # rewrite lang/*.pot and *.po
//! cargo tools l10n-extract --check   # fail if they are stale
//! ```
//!
//! The template, `lang/freight-fate.pot`, lists every line the game can
//! speak: the player-facing literals in `crates/` (see `rust_strings`), the
//! spoken fields of `data/buffs.json`, and `lang/extra.pot`, which holds
//! lines the source never spells out whole (assembled from pieces at run
//! time) that a maintainer added by hand.
//!
//! Each `lang/<code>.po` keeps every translation it has. A line new to the
//! template arrives untranslated; a line the game no longer says is
//! dropped, unless it was translated, in which case it is kept as an
//! obsolete `#~` entry so the work is not lost if the line comes back
//! reworded.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::po::{parse_po, write_po, Entry};
use super::rust_strings;
use crate::cli::{Args, Spec};
use crate::pyjson;
use crate::util::{self, py_isspace, py_strip, Result};

/// Every language with a catalog, matching `ff_core::l10n::LANGUAGES`.
pub const LANGUAGES: &[(&str, &str, &str)] = &[
    ("es", "Spanish", "nplurals=2; plural=(n != 1);"),
    ("fr", "French", "nplurals=2; plural=(n > 1);"),
    ("de", "German", "nplurals=2; plural=(n != 1);"),
    (
        "pt_BR",
        "Brazilian Portuguese",
        "nplurals=2; plural=(n != 1);",
    ),
    ("it", "Italian", "nplurals=2; plural=(n != 1);"),
    (
        "ru",
        "Russian",
        "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && \
         (n%100<10 || n%100>=20) ? 1 : 2);",
    ),
    (
        "pl",
        "Polish",
        "nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);",
    ),
    ("zh_CN", "Simplified Chinese", "nplurals=1; plural=0;"),
    ("ja", "Japanese", "nplurals=1; plural=0;"),
    ("vi", "Vietnamese", "nplurals=1; plural=0;"),
    ("tr", "Turkish", "nplurals=2; plural=(n != 1);"),
];

const CRATES: &[&str] = &["ff-core", "freight-fate"];
const BUFF_FIELDS: &[&str] = &["label", "help", "purchased", "worn_off"];

const TEMPLATE_HEADER: &str = "Project-Id-Version: Freight Fate\n\
Report-Msgid-Bugs-To: https://github.com/orinks-games/freight-fate/issues\n\
MIME-Version: 1.0\n\
Content-Type: text/plain; charset=UTF-8\n\
Content-Transfer-Encoding: 8bit\n";

static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{[A-Za-z_][A-Za-z0-9_]*\}").expect("fixed pattern"));

pub fn lang_dir() -> PathBuf {
    util::repo_root().join("lang")
}

/// Entries keyed by English, in the order they were first added.
#[derive(Default)]
struct Catalog {
    entries: Vec<Entry>,
    index: HashMap<String, usize>,
}

impl Catalog {
    fn get_mut(&mut self, msgid: &str) -> Option<&mut Entry> {
        self.index.get(msgid).map(|&k| &mut self.entries[k])
    }

    fn contains(&self, msgid: &str) -> bool {
        self.index.contains_key(msgid)
    }

    fn insert(&mut self, entry: Entry) {
        match self.index.get(&entry.msgid) {
            Some(&k) => self.entries[k] = entry,
            None => {
                self.index.insert(entry.msgid.clone(), self.entries.len());
                self.entries.push(entry);
            }
        }
    }
}

/// `re.split(r"(?<=[.!?])\s+(?=[A-Z0-9{\"])", text)`: break after a
/// sentence's closing mark, at a run of space before a capital, a digit, a
/// placeholder or a quote.
fn split_sentences(text: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 1;
    while i < chars.len() {
        if matches!(chars[i - 1].1, '.' | '!' | '?') && py_isspace(chars[i].1) {
            let mut j = i;
            while j < chars.len() && py_isspace(chars[j].1) {
                j += 1;
            }
            if j < chars.len()
                && (chars[j].1.is_ascii_uppercase()
                    || chars[j].1.is_ascii_digit()
                    || chars[j].1 == '{'
                    || chars[j].1 == '"')
            {
                out.push(&text[start..chars[i].0]);
                start = chars[j].0;
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out.push(&text[start..]);
    out
}

/// `re.sub(r"(?<=[A-Za-z.])\{name\}$", "", text)`: drop a piece glued on
/// straight after a word at the end.
fn drop_glued_tail(text: &str) -> String {
    let (body, newline) = match text.strip_suffix('\n') {
        Some(b) => (b, "\n"),
        None => (text, ""),
    };
    for m in PLACEHOLDER.find_iter(body) {
        if m.end() == body.len() {
            let glued = body[..m.start()]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '.');
            if glued {
                return format!("{}{newline}", &body[..m.start()]);
            }
        }
    }
    text.to_string()
}

fn letters(text: &str) -> usize {
    PLACEHOLDER
        .replace_all(text, "")
        .chars()
        .filter(|c| c.is_alphabetic())
        .count()
}

fn placeholder_note(text: &str) -> Vec<String> {
    let names: Vec<&str> = PLACEHOLDER.find_iter(text).map(|m| m.as_str()).collect();
    if names.is_empty() {
        Vec::new()
    } else {
        vec![format!("placeholders: {}", names.join(", "))]
    }
}

/// Add each piece of a longer line as a template of its own, unless it is
/// already one or has fewer than three letters.
fn add_pieces(catalog: &mut Catalog, parent: &Entry, pieces: Vec<String>, note: &str) {
    for piece in pieces {
        let piece = py_strip(&piece).to_string();
        if catalog.contains(&piece) || letters(&piece) < 3 {
            continue;
        }
        let mut comments = placeholder_note(&piece);
        comments.push(note.to_string());
        catalog.insert(Entry {
            msgid: piece,
            refs: parent.refs.iter().take(1).cloned().collect(),
            line: parent.line,
            comments,
            ..Entry::default()
        });
    }
}

fn template_entries(root: &Path) -> Result<Vec<Entry>> {
    let found = rust_strings::extract(root, CRATES)?;
    let mut catalog = Catalog::default();
    let mut contextual: Vec<Entry> = Vec::new();
    for template in found {
        let comments = if template.placeholders.is_empty() {
            Vec::new()
        } else {
            let holes: Vec<String> = template
                .placeholders
                .iter()
                .map(|p| format!("{{{p}}}"))
                .collect();
            vec![format!("placeholders: {}", holes.join(", "))]
        };
        // No line has a context of its own today; a (file, English) pair
        // that needs one would move from `refs` into `contextual` here.
        if template.refs.is_empty() {
            continue;
        }
        catalog.insert(Entry {
            msgid: template.msgid,
            refs: template.refs.into_iter().take(4).collect(),
            comments,
            line: template.line,
            ..Entry::default()
        });
    }
    // A long line built in one format! call often reaches the player with
    // one of its optional pieces changed (or empty), so each sentence of a
    // template with placeholders is a template too: the game translates
    // sentence by sentence when the whole line does not match.
    for template in catalog.entries.clone() {
        let sentences: Vec<String> = split_sentences(&template.msgid)
            .into_iter()
            .map(str::to_string)
            .collect();
        if sentences.len() < 2 || !PLACEHOLDER.is_match(&template.msgid) {
            continue;
        }
        add_pieces(
            &mut catalog,
            &template,
            sentences,
            "one sentence of a longer line",
        );
    }
    // A piece the game appends to a longer line (", automatic speed control
    // paused{keeper_target}") reaches the player inside that line, where the
    // game translates it clause by clause. Each of its clauses is a
    // template, without the joining comma or a piece glued on after it.
    for template in catalog.entries.clone() {
        let Some(rest) = template.msgid.strip_prefix(", ") else {
            continue;
        };
        let body = drop_glued_tail(py_strip(rest));
        let clauses = body.split(", ").map(str::to_string).collect();
        add_pieces(
            &mut catalog,
            &template,
            clauses,
            "one clause of a longer line",
        );
    }
    let buffs = pyjson::load_path(&root.join("data").join("buffs.json"))?;
    for buff in buffs
        .as_object()
        .map(|o| o.values().collect())
        .unwrap_or_else(Vec::new)
    {
        for field in BUFF_FIELDS {
            let Some(text) = buff.get(field).and_then(pyjson::Value::as_str) else {
                continue;
            };
            if py_strip(text).is_empty() {
                continue;
            }
            let reference = "data/buffs.json".to_string();
            let line = catalog.entries.len();
            match catalog.get_mut(text) {
                Some(entry) => {
                    if !entry.refs.contains(&reference) {
                        entry.refs.push(reference);
                    }
                }
                None => catalog.insert(Entry {
                    msgid: text.to_string(),
                    refs: vec![reference],
                    line,
                    ..Entry::default()
                }),
            }
        }
    }
    let extra = root.join("lang").join("extra.pot");
    if extra.exists() {
        for item in parse_po(&util::read_text(&extra)?).map_err(|e| format!("extra.pot: {e}"))? {
            if item.msgid.is_empty() {
                continue;
            }
            let refs = if item.refs.is_empty() {
                vec!["lang/extra.pot".to_string()]
            } else {
                item.refs.clone()
            };
            if item.msgctxt.is_some() {
                if contextual.iter().all(|c| c.key() != item.key()) {
                    contextual.push(Entry {
                        msgid: item.msgid.clone(),
                        msgctxt: item.msgctxt.clone(),
                        refs,
                        comments: item.comments.clone(),
                        line: item.line,
                        ..Entry::default()
                    });
                }
                continue;
            }
            match catalog.get_mut(&item.msgid) {
                Some(entry) => entry.comments.extend(item.comments.iter().cloned()),
                None => catalog.insert(Entry {
                    msgid: item.msgid.clone(),
                    refs,
                    comments: item.comments.clone(),
                    line: item.line,
                    ..Entry::default()
                }),
            }
        }
    }
    let mut all = catalog.entries;
    all.extend(contextual);
    // Group lines by the file that says them, in the order it says them.
    all.sort_by(|a, b| {
        let key = |e: &Entry| (e.refs.first().cloned().unwrap_or_default(), e.line);
        key(a).cmp(&key(b)).then_with(|| a.msgid.cmp(&b.msgid))
    });
    Ok(all)
}

/// The built-in languages, then any other `lang/*.po` a translator added.
pub fn language_codes() -> Vec<String> {
    let mut codes: Vec<String> = LANGUAGES.iter().map(|(c, _, _)| c.to_string()).collect();
    let mut added: Vec<String> = util::glob_suffix(&lang_dir(), ".po")
        .iter()
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .filter(|stem| !LANGUAGES.iter().any(|(c, _, _)| c == stem))
        .collect();
    added.sort();
    codes.extend(added);
    codes
}

/// A built-in language's header, or the one an added file brought.
fn language_header(code: &str, existing: &[Entry]) -> String {
    match LANGUAGES.iter().find(|(c, _, _)| *c == code) {
        Some((_, name, plural)) => format!(
            "{TEMPLATE_HEADER}Language: {code}\nPlural-Forms: {plural}\nX-Language-Name: {name}\n"
        ),
        None => {
            let brought = existing
                .iter()
                .find(|e| e.msgid.is_empty())
                .map(|e| e.msgstr.clone())
                .unwrap_or_default();
            if brought.is_empty() {
                format!("{TEMPLATE_HEADER}Language: {code}\n")
            } else {
                brought
            }
        }
    }
}

/// The language file for `template`, keeping `existing` translations.
fn merge(code: &str, template: &[Entry], existing: Vec<Entry>) -> Vec<Entry> {
    let header = language_header(code, &existing);
    // Keyed maps that keep first-seen order for the leftovers, as Python's
    // dicts do; a later duplicate replaces an earlier one in place.
    let mut known: Vec<Option<Entry>> = Vec::new();
    let mut known_at: HashMap<(Option<String>, String), usize> = HashMap::new();
    let mut obsolete: Vec<Option<Entry>> = Vec::new();
    let mut obsolete_at: HashMap<(Option<String>, String), usize> = HashMap::new();
    for entry in existing.into_iter().filter(|e| !e.msgid.is_empty()) {
        let (list, at) = if entry.obsolete {
            (&mut obsolete, &mut obsolete_at)
        } else {
            (&mut known, &mut known_at)
        };
        match at.get(&entry.key()) {
            Some(&k) => list[k] = Some(entry),
            None => {
                at.insert(entry.key(), list.len());
                list.push(Some(entry));
            }
        }
    }
    let mut out = vec![Entry {
        msgstr: header,
        ..Entry::default()
    }];
    for entry in template {
        let key = entry.key();
        let old = known_at
            .remove(&key)
            .and_then(|k| known[k].take())
            .or_else(|| obsolete_at.remove(&key).and_then(|k| obsolete[k].take()));
        out.push(Entry {
            msgid: entry.msgid.clone(),
            msgctxt: entry.msgctxt.clone(),
            msgstr: old.as_ref().map(|o| o.msgstr.clone()).unwrap_or_default(),
            // The template's notes come first; a translator's own `#`
            // notes on the entry survive the merge.
            comments: entry.comments.clone(),
            translator_comments: old
                .as_ref()
                .map(|o| o.translator_comments.clone())
                .unwrap_or_default(),
            flags: old.map(|o| o.flags).unwrap_or_default(),
            ..Entry::default()
        });
    }
    for mut gone in known.into_iter().chain(obsolete).flatten() {
        if !gone.msgstr.is_empty() {
            gone.obsolete = true;
            gone.refs.clear();
            gone.comments.clear();
            out.push(gone);
        }
    }
    out
}

/// Every file the extractor writes, with its text.
pub fn render_all() -> Result<Vec<(PathBuf, String)>> {
    let root = util::repo_root();
    let lang = lang_dir();
    let template = template_entries(&root)?;
    let mut with_header = vec![Entry {
        msgstr: TEMPLATE_HEADER.to_string(),
        ..Entry::default()
    }];
    with_header.extend(template.iter().cloned());
    let mut files = vec![(lang.join("freight-fate.pot"), write_po(&with_header, true))];
    for code in language_codes() {
        let path = lang.join(format!("{code}.po"));
        let existing = if path.exists() {
            parse_po(&util::read_text(&path)?).map_err(|e| format!("{code}.po: {e}"))?
        } else {
            Vec::new()
        };
        files.push((path, write_po(&merge(&code, &template, existing), false)));
    }
    Ok(files)
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &[],
            flags: &["check"],
        },
    )
    .map_err(|e| format!("{e}\nusage: l10n-extract [--check]"))?;
    let root = util::repo_root();
    let files = render_all()?;
    let stale: Vec<&(PathBuf, String)> = files
        .iter()
        .filter(|(path, text)| util::read_text_lf(path).as_ref() != Some(text))
        .collect();
    if args.flag("check") {
        for (path, _) in &stale {
            println!("stale: {}", util::rel(path, &root));
        }
        if !stale.is_empty() {
            println!("Run: cargo tools l10n-extract");
            return Ok(1);
        }
        return Ok(0);
    }
    for (path, text) in &stale {
        util::write_text(path, text)?;
        println!("wrote {}", util::rel(path, &root));
    }
    let count = parse_po(&files[0].1).map(|e| e.len()).unwrap_or(1) - 1;
    println!("{count} lines in the template");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_split_before_capitals_digits_and_holes() {
        assert_eq!(
            split_sentences("Fuel low. {miles} miles left!  Stop soon. ok then"),
            ["Fuel low.", "{miles} miles left!", "Stop soon. ok then"]
        );
    }

    #[test]
    fn glued_tails_drop_only_after_a_word() {
        assert_eq!(drop_glued_tail("cruise paused{target}"), "cruise paused");
        assert_eq!(drop_glued_tail("cruise {target}"), "cruise {target}");
        assert_eq!(drop_glued_tail("paused.{x}\n"), "paused.\n");
    }

    #[test]
    fn merge_keeps_translations_and_retires_lost_ones() {
        let mut kept = Entry::new("Hello");
        kept.msgstr = "Hola".into();
        let mut lost = Entry::new("Gone");
        lost.msgstr = "Ido".into();
        let untranslated = Entry::new("Never");
        let template = vec![Entry::new("Hello"), Entry::new("New")];
        let merged = merge("es", &template, vec![kept, lost, untranslated]);
        let ids: Vec<&str> = merged.iter().map(|e| e.msgid.as_str()).collect();
        assert_eq!(ids, ["", "Hello", "New", "Gone"]);
        assert_eq!(merged[1].msgstr, "Hola");
        assert!(merged[3].obsolete);
        assert!(merged[0].msgstr.contains("X-Language-Name: Spanish"));
    }

    #[test]
    fn committed_language_files_are_current() {
        for (path, text) in render_all().unwrap() {
            let committed = util::read_text_lf(&path).unwrap_or_default();
            assert!(
                committed == text,
                "{} is stale; run `cargo tools l10n-extract`",
                path.display()
            );
        }
    }
}
