//! `l10n-diff`: what a change to the language files does to real lines.
//!
//! ```text
//! cargo tools l10n-diff --base 6150fc1c            # every language
//! cargo tools l10n-diff --base dev --lang es       # one language
//! cargo tools l10n-diff --base dev --lines said.txt
//! ```
//!
//! Each English line is translated twice, with the language file at `--base`
//! and with the one on disk, and every line whose translation changed is
//! printed. A line is tagged MIXED when its translation now keeps more
//! English words than before, or was all English and is now part English:
//! that is the damage a loose new entry does ("{cargo_label} for
//! {destination}" turning "Signal set for exit 42" into "Blinker gesetzt für
//! exit 42"). The lines are every English string literal the integration
//! tests and the translation tests already hold, plus `--lines`, one per line.

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use ff_core::l10n::{po, Catalog, Translator};
use regex::Regex;

use super::extract::{lang_dir, language_codes};
use crate::cli::{Args, Spec};
use crate::util::{self, Result};

static LITERAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""((?:[^"\\]|\\.)*)""#).expect("fixed pattern"));
static CONTINUATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\\n\s*").expect("fixed pattern"));
static WORDY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z]{3}").expect("fixed pattern"));

/// Every `.rs` file under `dir`, recursively.
fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// English lines the tests hold: string literals that read as a sentence or
/// a phrase (a capital or a digit first, a space, a word of three letters).
pub fn test_lines(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for crate_dir in ["crates/ff-core", "crates/freight-fate"] {
        rust_files(&root.join(crate_dir).join("tests"), &mut files);
    }
    rust_files(&root.join("crates/ff-core/src/l10n/tests"), &mut files);
    files.push(root.join("crates/ff-core/src/l10n/tests.rs"));
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let text = CONTINUATION.replace_all(&text, "");
        for caps in LITERAL.captures_iter(&text) {
            let raw = caps[1].replace("\\\"", "\"").replace("\\'", "'");
            let first = raw.chars().next().unwrap_or(' ');
            if raw.contains('\\')
                || raw.contains('{')
                || !raw.trim().contains(' ')
                || !WORDY.is_match(&raw)
                || !(first.is_uppercase() || first.is_ascii_digit())
                || raw.chars().any(|c| c as u32 > 0x2000)
            {
                continue;
            }
            if seen.insert(raw.clone()) {
                lines.push(raw);
            }
        }
    }
    lines
}

/// Lowercase ASCII words of two letters or more.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphabetic() && c != '\'')
        .filter(|w| w.len() >= 2 && w.chars().all(|c| c.is_ascii_lowercase() || c == '\''))
}

/// Words of the English lines that the language's own translations never use.
fn english_words(entries: &[po::PoEntry]) -> HashSet<String> {
    let native: HashSet<&str> = entries.iter().flat_map(|e| words(&e.msgstr)).collect();
    entries
        .iter()
        .flat_map(|e| words(&e.msgid))
        .filter(|w| !native.contains(w))
        .map(str::to_string)
        .collect()
}

/// The changed lines of one language, printed; returns (changed, mixed).
fn diff_language(
    code: &str,
    base: &str,
    current: &str,
    lines: &[String],
) -> Result<(usize, usize)> {
    let before =
        Translator::new(Catalog::from_po(code, base).map_err(|e| format!("{code} at base: {e}"))?);
    let after = Translator::new(
        Catalog::from_po(code, current).map_err(|e| format!("lang/{code}.po: {e}"))?,
    );
    let english = english_words(&po::parse(current).map_err(|e| e.to_string())?);
    // Ideographic and Cyrillic text has no English words of its own.
    let latin = !matches!(code, "zh_CN" | "ja" | "ru");
    let count = |text: &str| {
        words(text)
            .filter(|w| !latin || english.contains(*w))
            .count()
    };
    let (mut changed, mut mixed) = (0, 0);
    for line in lines {
        let was = before.translate(line);
        let now = after.translate(line);
        if was == now {
            continue;
        }
        changed += 1;
        let (english_was, english_now) = (count(&was), count(&now));
        let tag = if english_now > english_was || (was == line.as_str() && english_now > 0) {
            mixed += 1;
            "MIXED"
        } else {
            "changed"
        };
        println!("[{code}] {tag}: {line}\n    was: {was}\n    now: {now}");
    }
    println!(
        "{code}: {changed} of {} lines changed, {mixed} MIXED",
        lines.len()
    );
    Ok((changed, mixed))
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["base", "lang", "lines"],
            flags: &[],
        },
    )
    .map_err(|e| format!("{e}\nusage: l10n-diff --base REV [--lang CODE] [--lines FILE]"))?;
    let base = args
        .value("base")
        .ok_or("usage: l10n-diff --base REV [--lang CODE] [--lines FILE]")?;
    let root = util::repo_root();
    let mut lines = test_lines(&root);
    if let Some(path) = args.value("lines") {
        let extra = util::read_text(Path::new(path))?;
        let mut seen: HashSet<String> = lines.iter().cloned().collect();
        lines.extend(
            extra
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(str::to_string)
                .filter(|l| seen.insert(l.clone())),
        );
    }
    let codes: Vec<String> = match args.value("lang") {
        Some(code) => vec![code.to_string()],
        None => language_codes(),
    };
    let mut any_mixed = false;
    for code in codes {
        let current = util::read_text(&lang_dir().join(format!("{code}.po")))?;
        let old = util::git(&root, &["show", &format!("{base}:lang/{code}.po")])?;
        let (_, mixed) = diff_language(&code, &old, &current, &lines)?;
        any_mixed |= mixed > 0;
    }
    Ok(i32::from(any_mixed))
}
