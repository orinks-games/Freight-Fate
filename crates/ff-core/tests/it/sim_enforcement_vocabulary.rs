//! The enforcement vocabulary, swept across the source.
//!
//! "Bear" is trade slang, and it may appear only inside a clause the line
//! attributes to the CB. In a warning, a menu, or a status readout the word
//! is "trooper" (`docs/ontology.md`). The Python game enforced this with a
//! sweep of every string in the package; this is that sweep for the Rust
//! sources of both crates.

use std::path::{Path, PathBuf};

/// Every string literal in `source`, comments skipped. A small scanner
/// rather than a line match: a Rust string can run across lines, and a line
/// match would miss the word on a continuation line.
fn string_literals(source: &str) -> Vec<String> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
            }
            'r' if matches!(chars.get(i + 1), Some('"' | '#'))
                && !(i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_')) =>
            {
                let mut hashes = 0;
                let mut j = i + 1;
                while chars.get(j) == Some(&'#') {
                    hashes += 1;
                    j += 1;
                }
                if chars.get(j) != Some(&'"') {
                    i += 1;
                    continue;
                }
                let start = j + 1;
                let mut end = start;
                while end < chars.len()
                    && !(chars[end] == '"'
                        && (1..=hashes).all(|k| chars.get(end + k) == Some(&'#')))
                {
                    end += 1;
                }
                out.push(chars[start..end.min(chars.len())].iter().collect());
                i = end + 1 + hashes;
            }
            '"' => {
                let mut body = String::new();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' {
                        i += 1;
                    }
                    if let Some(c) = chars.get(i) {
                        body.push(*c);
                    }
                    i += 1;
                }
                out.push(body);
                i += 1;
            }
            // A char literal ('"' among them) is skipped whole; a lifetime
            // has no closing quote two characters on.
            '\'' if chars.get(i + 1) == Some(&'\\') => {
                i += 3;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                i += 1;
            }
            '\'' if chars.get(i + 2) == Some(&'\'') => i += 3,
            _ => i += 1,
        }
    }
    out
}

/// Whether `text` uses "bear" or "bears", any case, as a word of its own.
fn says_bear(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.match_indices("bear").any(|(at, _)| {
        let before = lower[..at].chars().next_back();
        let rest = &lower[at + 4..];
        let rest = rest.strip_prefix('s').unwrap_or(rest);
        !before.is_some_and(char::is_alphabetic) && !rest.starts_with(char::is_alphabetic)
    })
}

fn rust_sources(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir)
        .expect("a source directory")
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn test_bear_is_never_spoken_in_any_player_facing_string() {
    // CB slang most players never learned; the officer is a trooper, on the
    // CB too (owner, 2026-10-11). The one use allowed is a song title.
    const TITLES: [&str; 1] = ["Black Bear Road"];

    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/");
    let mut files = Vec::new();
    for krate in ["ff-core", "freight-fate"] {
        rust_sources(&crates.join(krate).join("src"), &mut files);
    }
    assert!(files.len() > 100, "the sweep found {} files", files.len());

    let mut titles_seen = 0;
    let mut offenders = Vec::new();
    for path in &files {
        let source = std::fs::read_to_string(path).expect("readable source");
        for literal in string_literals(&source) {
            if TITLES.iter().any(|title| literal.contains(title)) {
                titles_seen += 1;
            }
            let text = TITLES
                .iter()
                .fold(literal.clone(), |text, title| text.replace(title, ""));
            if says_bear(&text) {
                offenders.push(format!("{}: {literal}", path.display()));
            }
        }
    }
    assert!(offenders.is_empty(), "{offenders:#?}");
    // The song title is still found, so the sweep is looking.
    assert!(titles_seen >= 1, "the sweep found no song title");
}

#[test]
fn the_bear_sweep_reads_strings_the_way_rust_writes_them() {
    let source = r##"
        // a bear in a comment is not a string
        let a = "a bear on the shoulder";
        let b = "CB chatter: a bear \
                 on the shoulder";
        let c = r#"raw "bears" here"#;
        let d = '"';
        let e = "bearing north, overbearing";
        fn f<'a>(x: &'a str) {}
    "##;
    let literals = string_literals(source);
    let found: Vec<&String> = literals.iter().filter(|s| says_bear(s)).collect();
    assert_eq!(found.len(), 3, "{literals:#?}");
    assert!(found.iter().any(|s| s.contains("CB")));
    assert!(literals.iter().any(|s| s == "bearing north, overbearing"));
    assert!(says_bear("Bear reported ahead."));
}
