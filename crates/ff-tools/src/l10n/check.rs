//! `l10n-check`: check every language file in `lang/` for mistakes the
//! game cannot fix.
//!
//! ```text
//! cargo tools l10n-check           # fail on any problem
//! cargo tools l10n-check --stats   # also print coverage
//! ```
//!
//! A translated entry must keep every placeholder of its English line and
//! name no other, keep every number, and keep the key names the help reads
//! out (Enter, Escape, F1 ...). A plural choice must give as many forms as
//! the language has. The game skips an entry it cannot use and speaks the
//! English instead, so none of these break the game; they break the
//! translation.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::extract::{lang_dir, language_codes, LANGUAGES};
use super::po::parse_po;
use crate::cli::{Args, Spec};
use crate::util::{self, Result};

static HOLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\{([A-Za-z_][A-Za-z0-9_]*)(?::([^{}]*))?\}").expect("fixed pattern")
});
static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").expect("fixed pattern"));

/// Key names as the help and the key settings speak them, in the order the
/// alternatives are tried. "Shift" is left out: it is also the gearshift
/// verb ("Shift up").
const KEY_NAMES: &[&str] = &[
    "Enter",
    "Escape",
    "Backspace",
    "Page Up",
    "Page Down",
    "Tab",
    "Control",
    "Alt",
    "F10",
    "F11",
    "F12",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
];

/// Python's `repr()` of a string, for naming an entry in a report.
pub fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c if c.is_control() => {
                let code = c as u32;
                if code <= 0xff {
                    out.push_str(&format!("\\x{code:02x}"));
                } else {
                    out.push_str(&format!("\\u{code:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `re.findall(r"(?<![A-Za-z])(Enter|...|F(?:1[0-2]|[1-9]))(?![A-Za-z])")`.
fn key_names(text: &str) -> Vec<&'static str> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if i > 0 && chars[i - 1].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let hit = KEY_NAMES.iter().find(|name| {
            let len = name.chars().count();
            chars.len() >= i + len
                && chars[i..i + len].iter().copied().eq(name.chars())
                && !chars.get(i + len).is_some_and(char::is_ascii_alphabetic)
        });
        match hit {
            Some(name) => {
                found.push(*name);
                i += name.chars().count();
            }
            None => i += 1,
        }
    }
    found
}

fn plural_forms(code: &str) -> usize {
    match code {
        "zh_CN" | "ja" | "vi" => 1,
        "ru" | "pl" => 3,
        _ => 2,
    }
}

fn check_entry(code: &str, msgid: &str, msgstr: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let source_holes: BTreeSet<&str> = HOLE
        .captures_iter(msgid)
        .filter(|c| c.get(2).is_none())
        .map(|c| c.get(1).map_or("", |m| m.as_str()))
        .collect();
    let translated: Vec<regex::Captures> = HOLE.captures_iter(msgstr).collect();
    let used: BTreeSet<&str> = translated
        .iter()
        .map(|c| c.get(1).map_or("", |m| m.as_str()))
        .collect();
    for name in used.difference(&source_holes) {
        problems.push(format!(
            "names {{{name}}}, which the English line does not have"
        ));
    }
    // {s_ending} is the English "s"; a translation leaves it out.
    for name in source_holes.difference(&used) {
        if *name != "s_ending" {
            problems.push(format!("drops {{{name}}}"));
        }
    }
    let mut depth = 0i32;
    let mut broken = false;
    for ch in msgstr.chars() {
        depth += match ch {
            '{' => 1,
            '}' => -1,
            _ => 0,
        };
        if !(0..=1).contains(&depth) {
            problems.push("has a brace inside a placeholder, or an unmatched brace".into());
            broken = true;
            break;
        }
    }
    if !broken && depth != 0 {
        problems.push("has an unmatched brace".into());
    }
    let want = plural_forms(code);
    for caps in &translated {
        if let Some(forms) = caps.get(2) {
            let count = forms.as_str().split('|').count();
            if count != want && !(want == 1 && count >= 1) {
                problems.push(format!(
                    "{{{}:...}} gives {count} forms; {code} has {want}",
                    &caps[1]
                ));
            }
        }
    }
    let numbers = |text: &str| -> Vec<String> {
        NUMBER
            .find_iter(&HOLE.replace_all(text, " "))
            .map(|m| m.as_str().to_string())
            .collect()
    };
    let mut source_numbers = numbers(msgid);
    source_numbers.sort();
    let mut target_numbers = numbers(msgstr);
    for number in source_numbers {
        match target_numbers.iter().position(|n| *n == number) {
            Some(k) => {
                target_numbers.remove(k);
            }
            None => problems.push(format!("loses the number {number}")),
        }
    }
    let mut target_keys = key_names(msgstr);
    for key in key_names(msgid) {
        match target_keys.iter().position(|k| *k == key) {
            Some(k) => {
                target_keys.remove(k);
            }
            None => problems.push(format!("translates or drops the key name {key}")),
        }
    }
    problems
}

fn check_file(code: &str, path: &Path) -> Result<(Vec<String>, usize, usize)> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let entries = match parse_po(&util::read_text(path)?) {
        Ok(entries) => entries,
        Err(err) => return Ok((vec![format!("{name}: {err}")], 0, 0)),
    };
    let mut problems = Vec::new();
    let (mut total, mut translated) = (0, 0);
    for entry in entries {
        if entry.msgid.is_empty() || entry.obsolete {
            continue;
        }
        total += 1;
        if entry.msgstr.is_empty() || entry.fuzzy() {
            continue;
        }
        translated += 1;
        let short: String = entry.msgid.chars().take(70).collect();
        for problem in check_entry(code, &entry.msgid, &entry.msgstr) {
            problems.push(format!(
                "{name}:{}: {problem}: {}",
                entry.line,
                py_repr(&short)
            ));
        }
    }
    Ok((problems, total, translated))
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &[],
            flags: &["stats"],
        },
    )
    .map_err(|e| format!("{e}\nusage: l10n-check [--stats]"))?;
    let mut failed = false;
    for code in language_codes() {
        let name = LANGUAGES
            .iter()
            .find(|(c, _, _)| *c == code)
            .map_or(code.as_str(), |(_, n, _)| n);
        let path = lang_dir().join(format!("{code}.po"));
        if !path.exists() {
            println!("missing: lang/{code}.po");
            failed = true;
            continue;
        }
        let (problems, total, translated) = check_file(&code, &path)?;
        for problem in &problems {
            println!("{problem}");
        }
        failed |= !problems.is_empty();
        if args.flag("stats") {
            let share = if total > 0 {
                100.0 * translated as f64 / total as f64
            } else {
                0.0
            };
            println!(
                "{name} ({code}): {translated} of {total} lines, {} percent",
                util::py_round0(share)
            );
        }
    }
    Ok(i32::from(failed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_numbers_and_keys_must_survive() {
        let problems = check_entry(
            "es",
            "Press Enter for {miles} miles, 10 left",
            "Pulsa {millas} 9",
        );
        assert!(problems.contains(&"names {millas}, which the English line does not have".into()));
        assert!(problems.contains(&"drops {miles}".into()));
        assert!(problems.contains(&"loses the number 10".into()));
        assert!(problems.contains(&"translates or drops the key name Enter".into()));
        assert!(check_entry("es", "{n} load{s_ending}", "{n:carga|cargas}").is_empty());
        assert_eq!(
            check_entry("ru", "{n} load{s_ending}", "{n:a|b}"),
            ["{n:...} gives 2 forms; ru has 3"]
        );
    }

    #[test]
    fn key_names_follow_the_regex() {
        assert_eq!(key_names("F10a F1 xTab Tab."), ["F1", "F1", "Tab"]);
        assert_eq!(py_repr("it's"), "\"it's\"");
        assert_eq!(py_repr("a\nb"), "'a\\nb'");
    }

    #[test]
    fn committed_language_files_pass() {
        for code in language_codes() {
            let path = lang_dir().join(format!("{code}.po"));
            let (problems, _, _) = check_file(&code, &path).unwrap();
            assert!(problems.is_empty(), "{problems:?}");
        }
    }
}
