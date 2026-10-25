//! Find the player-facing string literals in the game's Rust source.
//!
//! The game builds its spoken lines with `format!` and string literals
//! scattered across `crates/`. Rather than wrapping every call site in a
//! translation macro, the game translates each finished line at the speech
//! boundary (`ff_core::l10n`), matching it against the templates this
//! module extracts. So extraction only has to answer one question per
//! literal: could this text reach the player's ears?
//!
//! It errs toward including a literal. A template nobody ever hears costs a
//! translator one line; a spoken line that was filtered out stays English.
//!
//! Token comparisons below look at a token's text alone where the original
//! extractor did, so a string literal whose text is `(` counts as an opening
//! bracket. The committed template depends on exactly these rules.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::util::{self, py_isspace, py_lstrip, py_strip};

/// Macros whose literals are never spoken: logging, assertions, panics.
#[rustfmt::skip]
const SKIP_MACROS: &[&str] = &[
    "log", "info", "warn", "error", "debug", "trace", "transcript", "panic", "assert",
    "assert_eq", "assert_ne", "debug_assert", "debug_assert_eq", "debug_assert_ne",
    "unreachable", "todo", "unimplemented", "eprintln", "eprint", "println", "print",
    "include_str", "include_bytes", "env", "option_env", "concat", "matches", "cfg",
    "compile_error",
];

/// Functions and methods whose literal arguments are never spoken.
#[rustfmt::skip]
const SKIP_CALLS: &[&str] = &[
    "expect", "expect_err", "context", "with_context", "var", "var_os", "set_var",
    "remove_var", "join", "with_extension", "get", "get_mut", "contains_key", "remove",
    "starts_with", "ends_with", "strip_prefix", "strip_suffix", "split", "splitn", "rsplit",
    "split_once", "rsplit_once", "find", "rfind", "trim_matches", "trim_start_matches",
    "trim_end_matches", "contains", "Path", "PathBuf", "open", "create", "Command", "arg",
    "args", "header", "query", "pointer", "target",
    // SDL hints (sdl2::hint::set) are configuration values, never spoken.
    "set",
];

const FORMAT_MACROS: &[&str] = &["format", "write", "writeln", "format_args"];

/// Calls that translate what they are handed: the visual display's own
/// labels ("SPEED", "CRUISE {value}"). A literal inside one is player text
/// whatever its shape, so all-capital sign words are not dropped as keys.
const TRANSLATE_CALLS: &[&str] = &["tr_drawn"];

/// Path fragments whose files hold no player-facing text.
#[rustfmt::skip]
const SKIP_PATH_PARTS: &[&str] = &[
    "/tests/", "/benches/", "/bin/", "/agent_server", "/playtest", "/tests.rs", "_tests.rs",
    "jaws_script", "/build.rs", "/l10n",
    // Place names, song titles and radio clip names stay as they are.
    "/legacy_aliases.rs", "/real_traffic", "/music/", "/radio_content.rs", "/world_parsing",
    // Plumbing: integrity checks, the network clients, the settings file.
    "profile_integrity_invariants", "profile_invariants", "cloud_save_integrity",
    "/cloud_saves/api", "/settings/migrate", "/settings/paths", "/net.rs", "/net/",
    "/updater", "/online_presence", "discord_presence", "single_instance", "/app/logging",
    "/app/boot_timing", "/app/loop_watch",
];

/// The hole an English plural ending ("s", "es") fills; a translation may
/// leave it out.
const PLURAL_ENDING: &str = "s_ending";
const ENGLISH_ENDINGS: &[&str] = &["", "s", "es", "S", "ES"];
const HOLE_MARK: char = '\u{0}';

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a fixed pattern compiles")
}

static PLACEHOLDER_RE: LazyLock<Regex> = LazyLock::new(|| re(r"\{\{|\}\}|\{([^{}]*)\}"));
static PATHISH: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(\.(json|wav|ogg|opus|txt|toml|dll|so|dylib|exe|png|log|md|ffdata|pak|mp3)\b)|://|\\\\|^/|::",
    )
});
static HELP_TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| re(r"\{\{(?:(pad|key):)?([a-z_][a-z0-9_]*)\}\}"));

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Ident,
    Num,
    Punct,
    Str,
    Bytes,
    Char,
    Lifetime,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    text: String,
    line: usize,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Just enough of a Rust lexer to find literals and bracket nesting.
struct Lexer {
    src: Vec<char>,
    i: usize,
    line: usize,
    tokens: Vec<Token>,
}

impl Lexer {
    fn starts_with(&self, at: usize, s: &str) -> bool {
        (at..)
            .zip(s.chars())
            .all(|(j, c)| self.src.get(j) == Some(&c))
    }

    fn find(&self, from: usize, s: &str) -> Option<usize> {
        (from..self.src.len()).find(|&j| self.starts_with(j, s))
    }

    fn ident_end(&self, from: usize) -> Option<usize> {
        if !self.src.get(from).is_some_and(|&c| is_ident_start(c)) {
            return None;
        }
        let mut j = from + 1;
        while self.src.get(j).is_some_and(|&c| is_ident_char(c)) {
            j += 1;
        }
        Some(j)
    }

    fn push(&mut self, kind: Kind, text: String, line: usize) {
        self.tokens.push(Token { kind, text, line });
    }

    fn run(mut self) -> Vec<Token> {
        let n = self.src.len();
        while self.i < n {
            let c = self.src[self.i];
            if c == '\n' {
                self.line += 1;
                self.i += 1;
            } else if py_isspace(c) {
                self.i += 1;
            } else if self.starts_with(self.i, "//") {
                self.i = self.find(self.i, "\n").unwrap_or(n);
            } else if self.starts_with(self.i, "/*") {
                self.block_comment();
            } else if c == '"' {
                self.string(self.i + 1, None, Kind::Str);
            } else if (c == 'r' || c == 'b') && self.raw_or_byte() {
            } else if c == '\'' {
                self.char_or_lifetime();
            } else if c.is_alphabetic() || c == '_' {
                let end = self.ident_end(self.i).unwrap_or(self.i + 1);
                let text: String = self.src[self.i..end].iter().collect();
                self.push(Kind::Ident, text, self.line);
                self.i = end;
            } else if c.is_ascii_digit() {
                let mut end = self.i + 1;
                while self
                    .src
                    .get(end)
                    .is_some_and(|&d| d.is_ascii_alphanumeric() || d == '_' || d == '.')
                {
                    end += 1;
                }
                let mut text: String = self.src[self.i..end].iter().collect();
                // "1..3" is a range, not a float.
                if let Some(cut) = text.find("..") {
                    text.truncate(cut);
                }
                self.i += text.chars().count();
                self.push(Kind::Num, text, self.line);
            } else {
                self.push(Kind::Punct, c.to_string(), self.line);
                self.i += 1;
            }
        }
        self.tokens
    }

    fn block_comment(&mut self) {
        let mut depth = 0;
        while self.i < self.src.len() {
            if self.starts_with(self.i, "/*") {
                depth += 1;
                self.i += 2;
            } else if self.starts_with(self.i, "*/") {
                depth -= 1;
                self.i += 2;
                if depth == 0 {
                    return;
                }
            } else {
                if self.src[self.i] == '\n' {
                    self.line += 1;
                }
                self.i += 1;
            }
        }
    }

    fn raw_or_byte(&mut self) -> bool {
        // (b?)r(#*)"
        let mut j = self.i;
        let byte = self.src.get(j) == Some(&'b') && self.src.get(j + 1) == Some(&'r');
        if byte {
            j += 1;
        }
        if self.src.get(j) == Some(&'r') {
            let mut k = j + 1;
            while self.src.get(k) == Some(&'#') {
                k += 1;
            }
            if self.src.get(k) == Some(&'"') {
                let hashes: String = self.src[j + 1..k].iter().collect();
                let kind = if byte { Kind::Bytes } else { Kind::Str };
                self.string(k + 1, Some(hashes), kind);
                return true;
            }
        }
        if self.starts_with(self.i, "b\"") {
            self.string(self.i + 2, None, Kind::Bytes);
            return true;
        }
        if self.starts_with(self.i, "b'") {
            self.i += 1;
            self.char_or_lifetime();
            return true;
        }
        false
    }

    fn string(&mut self, start: usize, raw_hashes: Option<String>, kind: Kind) {
        let line = self.line;
        let body: String;
        if let Some(hashes) = raw_hashes {
            let close = format!("\"{hashes}");
            let end = self.find(start, &close).unwrap_or(self.src.len());
            body = self.src[start..end].iter().collect();
            self.i = (end + close.chars().count()).min(self.src.len());
        } else {
            let src = &self.src;
            let mut j = start;
            let mut out = String::new();
            while j < src.len() && src[j] != '"' {
                if src[j] == '\\' && j + 1 < src.len() {
                    let next = src[j + 1];
                    if next == '\n' {
                        // Line continuation: skip the newline and leading space.
                        j += 2;
                        while j < src.len() && matches!(src[j], ' ' | '\t' | '\n' | '\r') {
                            j += 1;
                        }
                        continue;
                    }
                    if next == 'u' {
                        let close = (j..src.len()).find(|&k| src[k] == '}').unwrap_or(j + 3);
                        let hex: String = src[(j + 3).min(close)..close].iter().collect();
                        if let Some(ch) =
                            u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                        {
                            out.push(ch);
                        }
                        j = close + 1;
                        continue;
                    }
                    if next == 'x' {
                        let hex: String = src[j + 2..(j + 4).min(src.len())].iter().collect();
                        if let Some(ch) =
                            u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                        {
                            out.push(ch);
                        }
                        j += 4;
                        continue;
                    }
                    out.push(match next {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        other => other,
                    });
                    j += 2;
                    continue;
                }
                out.push(src[j]);
                j += 1;
            }
            body = out;
            self.i = (j + 1).min(self.src.len());
        }
        self.line += self.src[start.min(self.i)..self.i]
            .iter()
            .filter(|&&c| c == '\n')
            .count();
        self.push(kind, body, line);
    }

    fn char_or_lifetime(&mut self) {
        // '(\\u\{[0-9a-fA-F]+\}|\\.|[^\\'])'
        let at = self.i;
        let src = &self.src;
        let body_end = if src.get(at + 1) == Some(&'\\')
            && src.get(at + 2) == Some(&'u')
            && src.get(at + 3) == Some(&'{')
        {
            let mut k = at + 4;
            while src.get(k).is_some_and(char::is_ascii_hexdigit) {
                k += 1;
            }
            if k > at + 4 && src.get(k) == Some(&'}') {
                Some(k + 1)
            } else if src.get(at + 2).is_some_and(|&c| c != '\n') {
                Some(at + 3)
            } else {
                None
            }
        } else if src.get(at + 1) == Some(&'\\') {
            src.get(at + 2).filter(|&&c| c != '\n').map(|_| at + 3)
        } else if src.get(at + 1).is_some_and(|&c| c != '\'') {
            Some(at + 2)
        } else {
            None
        };
        // A `\u{` escape that fails falls back to `\u` followed by a quote,
        // which never closes; try the other alternatives as the regex would.
        if let Some(end) = body_end.filter(|&e| src.get(e) == Some(&'\'')) {
            let text: String = src[at + 1..end].iter().collect();
            self.push(Kind::Char, text, self.line);
            self.i = end + 1;
            return;
        }
        match self.ident_end(at + 1) {
            Some(end) => {
                let text: String = self.src[at + 1..end].iter().collect();
                self.push(Kind::Lifetime, text, self.line);
                self.i = end;
            }
            None => {
                self.push(Kind::Lifetime, String::new(), self.line);
                self.i = at + 1;
            }
        }
    }
}

fn lex(src: &str) -> Vec<Token> {
    Lexer {
        src: src.chars().collect(),
        i: 0,
        line: 1,
        tokens: Vec::new(),
    }
    .run()
}

/// Drop `#[cfg(test)]` items (modules and functions) and `#[test]` fns.
fn strip_test_modules(tokens: Vec<Token>) -> Vec<Token> {
    let mut out = Vec::new();
    let n = tokens.len();
    let mut i = 0;
    while i < n {
        let tok = &tokens[i];
        if tok.kind == Kind::Punct && tok.text == "#" && i + 1 < n && tokens[i + 1].text == "[" {
            let mut j = i + 2;
            let mut depth = 1;
            let mut attr = String::new();
            while j < n && depth > 0 {
                if tokens[j].text == "[" {
                    depth += 1;
                } else if tokens[j].text == "]" {
                    depth -= 1;
                }
                if depth > 0 {
                    attr.push_str(&tokens[j].text);
                }
                j += 1;
            }
            if attr == "cfg(test)" || attr == "test" || attr.starts_with("cfg(all(test") {
                // Skip to the end of the following item: a `;` at depth 0 or
                // a balanced `{...}` block.
                let mut k = j;
                while k < n && tokens[k].text != "{" && tokens[k].text != ";" {
                    k += 1;
                }
                if k < n && tokens[k].text == "{" {
                    let mut depth = 0;
                    while k < n {
                        if tokens[k].text == "{" {
                            depth += 1;
                        } else if tokens[k].text == "}" {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        k += 1;
                    }
                }
                i = k + 1;
                continue;
            }
        }
        out.push(tok.clone());
        i += 1;
    }
    out
}

#[derive(Debug, Default)]
struct Literal {
    text: String,
    line: usize,
    callee_stack: Vec<String>,
    format_args: Vec<String>,
    is_format: bool,
    /// Compared or matched against (`== "x"`, `"x" =>`): a key, not words.
    is_key: bool,
    /// A struct field set straight from it (`kind: "browse".to_string()`).
    is_field_value: bool,
    /// Handed to a translating call, directly or as the format string of a
    /// `format!` that is.
    is_marked: bool,
}

/// One catalog line: its English, the files that say it, its holes.
#[derive(Clone, Debug)]
pub struct Template {
    pub msgid: String,
    pub refs: Vec<String>,
    pub placeholders: Vec<String>,
    /// Where in its first file the line appears, for ordering only.
    pub line: usize,
}

/// A readable placeholder name for one `format!` argument expression.
fn arg_name(expr: &[Token]) -> String {
    #[rustfmt::skip]
    const NOISE: &[&str] = &[
        "to_string", "clone", "as_str", "round", "abs", "max", "min", "floor", "ceil", "trunc",
        "to_lowercase", "to_uppercase", "trim", "unwrap_or", "unwrap_or_default", "unwrap",
        "as_ref", "display", "len", "into", "deref", "borrow", "to_owned",
    ];
    const TYPES: &[&str] = &["as", "f64", "f32", "u32", "i64", "u64", "usize", "i32"];
    const CONTEXTS: &[&str] = &["self", "ctx", "trip", "s", "x"];
    let idents: Vec<&Token> = expr.iter().filter(|t| t.kind == Kind::Ident).collect();
    // `if n == 1 { "" } else { "s" }`: the English plural ending, which a
    // translation leaves out and replaces with a {count:form|forms} choice.
    let strings: Vec<&str> = expr
        .iter()
        .filter(|t| t.kind == Kind::Str)
        .map(|t| t.text.as_str())
        .collect();
    if idents.iter().any(|t| t.text == "if")
        && !strings.is_empty()
        && strings.iter().all(|s| ENGLISH_ENDINGS.contains(s))
    {
        return PLURAL_ENDING.into();
    }
    let Some(last) = idents.last() else {
        return "value".into();
    };
    // Walk back over call groups: `fmt_miles(d)` names the function.
    let mut depth = 0i32;
    let mut names = Vec::new();
    for tok in expr.iter().rev() {
        if tok.text == ")" {
            depth += 1;
        } else if tok.text == "(" {
            depth -= 1;
        } else if tok.kind == Kind::Ident && depth == 0 {
            names.push(tok.text.as_str());
        }
    }
    for name in names {
        if NOISE.contains(&name) || TYPES.contains(&name) || CONTEXTS.contains(&name) {
            continue;
        }
        let mut name = name;
        for prefix in ["fmt_", "format_", "spoken_", "speak_", "say_", "render_"] {
            if name.starts_with(prefix) && name.len() > prefix.len() {
                name = &name[prefix.len()..];
            }
        }
        return name.to_string();
    }
    last.text.clone()
}

fn is_open(t: &Token) -> bool {
    t.kind == Kind::Punct && matches!(t.text.as_str(), "(" | "[" | "{")
}

fn is_close(t: &Token) -> bool {
    t.kind == Kind::Punct && matches!(t.text.as_str(), ")" | "]" | "}")
}

/// Split the comma-separated arguments after a format string literal.
fn literal_args(tokens: &[Token], start: usize) -> Vec<Vec<Token>> {
    let mut args = Vec::new();
    let mut depth = 0;
    let mut cur: Vec<Token> = Vec::new();
    for tok in &tokens[start.min(tokens.len())..] {
        if is_open(tok) {
            depth += 1;
        } else if is_close(tok) {
            if depth == 0 {
                break;
            }
            depth -= 1;
        }
        if tok.kind == Kind::Punct && tok.text == "," && depth == 0 {
            if !cur.is_empty() {
                args.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(tok.clone());
        }
    }
    if !cur.is_empty() {
        args.push(cur);
    }
    args
}

fn literals_in(src: &str) -> Vec<Literal> {
    let tokens = strip_test_modules(lex(src));
    let mut found = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut attr_depth = 0;
    let n = tokens.len();
    let text_at = |k: usize| tokens[k].text.as_str();
    for i in 0..n {
        let tok = &tokens[i];
        if is_open(tok) {
            let callee = if i >= 2 && text_at(i - 1) == "!" && tokens[i - 2].kind == Kind::Ident {
                format!("{}!", tokens[i - 2].text)
            } else if i >= 1 && tokens[i - 1].kind == Kind::Ident {
                tokens[i - 1].text.clone()
            } else if tok.text == "["
                && ((i >= 1 && text_at(i - 1) == "#")
                    || (i >= 2 && text_at(i - 1) == "!" && text_at(i - 2) == "#"))
            {
                "#attr".to_string()
            } else {
                String::new()
            };
            if callee == "#attr" {
                attr_depth += 1;
            }
            stack.push(callee);
        } else if is_close(tok) {
            if stack.pop().is_some_and(|c| c == "#attr") {
                attr_depth -= 1;
            }
        } else if tok.kind == Kind::Str {
            if attr_depth > 0 {
                continue;
            }
            let mut lit = Literal {
                text: tok.text.clone(),
                line: tok.line,
                callee_stack: stack.clone(),
                ..Literal::default()
            };
            let top = stack.last().map(String::as_str);
            lit.is_marked = top.is_some_and(|t| TRANSLATE_CALLS.contains(&t))
                || (stack.len() >= 2
                    && top == Some("format!")
                    && TRANSLATE_CALLS.contains(&stack[stack.len() - 2].as_str()));
            let before: String = tokens[i.saturating_sub(2)..i]
                .iter()
                .map(|t| t.text.as_str())
                .collect();
            let after: String = tokens[(i + 1).min(n)..(i + 3).min(n)]
                .iter()
                .map(|t| t.text.as_str())
                .collect();
            lit.is_key = before.ends_with("==")
                || before.ends_with("!=")
                || ["=>", "|", "==", "!="].iter().any(|p| after.starts_with(p))
                || (before.ends_with('|') && !before.ends_with("||"));
            lit.is_field_value = i >= 2
                && text_at(i - 1) == ":"
                && tokens[i - 2].kind == Kind::Ident
                && (i < 3 || text_at(i - 3) != ":");
            if let Some(top) = top {
                let bare = top.trim_end_matches('!');
                if FORMAT_MACROS.contains(&bare) && top.ends_with('!') {
                    // The format string is the first literal directly after
                    // the opening paren (format!) or after the writer arg
                    // (write!).
                    let prev = text_at(i - 1);
                    if prev == "(" || (prev == "," && (top == "write!" || top == "writeln!")) {
                        lit.is_format = true;
                        if i + 1 < n && text_at(i + 1) == "," {
                            lit.format_args = literal_args(&tokens, i + 2)
                                .iter()
                                .map(|a| arg_name(a))
                                .collect();
                        }
                    }
                }
            }
            found.push(lit);
        }
    }
    found
}

fn skipped(lit: &Literal) -> bool {
    let last = lit.callee_stack.last();
    lit.callee_stack.iter().any(|callee| {
        let bare = callee.trim_end_matches('!');
        if callee.ends_with('!') {
            SKIP_MACROS.contains(&bare)
        } else {
            SKIP_CALLS.contains(&bare) && Some(callee) == last
        }
    })
}

/// Python's `re.match(r"^...$", text)`: `$` also matches before one final
/// newline.
fn full_match(pattern: &Regex, text: &str) -> bool {
    pattern.is_match(text) || text.strip_suffix('\n').is_some_and(|t| pattern.is_match(t))
}

static LOWER_WORD: LazyLock<Regex> = LazyLock::new(|| re(r"^[a-z]{2,}$"));

/// Whether a literal reads as words a player could hear.
fn is_player_text(text: &str, is_format: bool, is_key: bool) -> bool {
    static TWO_LETTERS: LazyLock<Regex> = LazyLock::new(|| re(r"[A-Za-z]{2}"));
    static WORD: LazyLock<Regex> =
        LazyLock::new(|| re(r"^(?:[A-Z][a-z]+[.!?:,]?|[A-Za-z][a-z]+(-[A-Za-z]+)+)$"));
    static LEAD_IN: LazyLock<Regex> =
        LazyLock::new(|| re(r"^[a-z]{3,}, \{[A-Za-z_][A-Za-z0-9_]*(:[^}]*)?\}$"));
    static LABEL: LazyLock<Regex> =
        LazyLock::new(|| re(r"^[A-Z][A-Za-z]+: \{[A-Za-z_][A-Za-z0-9_]*\}$"));
    static KEY_READS: LazyLock<Regex> =
        LazyLock::new(|| re(r"^\{[A-Za-z_][A-Za-z0-9_]*\} [a-z]{2,}\.$"));

    let without = PLACEHOLDER_RE.replace_all(text, "");
    let stripped = py_strip(&without);
    if !TWO_LETTERS.is_match(stripped) {
        return false;
    }
    if PATHISH.is_match(text) {
        return false;
    }
    // Multi-line literals are help text, kept; JSON or code blobs are not.
    if text.contains('\n') && !is_format && py_lstrip(text).starts_with(['{', '[', '<']) {
        return false;
    }
    if !stripped.contains(' ') {
        // A single word: keep Capitalized words ("Off", "Settings"), drop
        // identifiers and keys ("driving", "fuel_stop", "SAPI").
        if matches!(stripped, "True" | "False" | "None") {
            return false;
        }
        if full_match(&WORD, stripped) {
            return true;
        }
        let whole = py_strip(text);
        // A lowercase word leading into a placeholder ("automatic, {name}")
        // is a phrase a value is spoken inside; a label leading into a
        // value ("ELD: {summary}"); a key and what it reads ("{fuel}
        // fuel.").
        if is_format
            && (full_match(&LEAD_IN, whole)
                || full_match(&LABEL, whole)
                || full_match(&KEY_READS, whole))
        {
            return true;
        }
        // A lowercase word is a value spoken in a sentence ("off",
        // "sparse") unless the code only compares or matches it.
        if is_key || !full_match(&LOWER_WORD, stripped) {
            return false;
        }
    }
    true
}

/// `{{engine}}` and `{{pad:horn}}`: the help's control names, filled in
/// before the line is spoken, so each is a hole too.
fn help_tokens(text: &str, names: &mut Vec<String>) -> String {
    HELP_TOKEN_RE
        .replace_all(text, |caps: &regex::Captures| {
            let device = caps.get(1).map_or("control", |m| m.as_str());
            let name = format!("{device}_{}", &caps[2]);
            if !names.contains(&name) {
                names.push(name.clone());
            }
            format!("{HOLE_MARK}{name}{HOLE_MARK}")
        })
        .into_owned()
}

fn marks_to_holes(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(HOLE_MARK) {
        let after = &rest[start + 1..];
        match after.find(HOLE_MARK) {
            Some(end) => {
                out.push_str(&rest[..start]);
                out.push('{');
                out.push_str(&after[..end]);
                out.push('}');
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

fn is_py_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// The catalog form of a literal: `{name}` placeholders, no format specs.
fn to_template(lit: &Literal) -> (String, Vec<String>) {
    let mut names = Vec::new();
    if !lit.is_format {
        let text = help_tokens(&lit.text, &mut names);
        return (marks_to_holes(&text), names);
    }
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut positional = 0usize;
    let mut name_for = |spec: &str| -> String {
        let key = py_strip(spec.split(':').next().unwrap_or("")).to_string();
        let key = if key.is_empty() {
            let idx = positional;
            positional += 1;
            lit.format_args
                .get(idx)
                .cloned()
                .unwrap_or_else(|| format!("value{}", idx + 1))
        } else if is_py_digits(&key) {
            let idx: usize = key.parse().unwrap_or(0);
            lit.format_args
                .get(idx)
                .cloned()
                .unwrap_or_else(|| format!("value{}", idx + 1))
        } else {
            key
        };
        key.rsplit('.').next().unwrap_or("").to_string()
    };
    let mut out = String::new();
    let mut pos = 0;
    let mut seen_specs: HashMap<String, String> = HashMap::new();
    for caps in PLACEHOLDER_RE.captures_iter(&lit.text) {
        let whole = caps.get(0).expect("a match has group 0");
        out.push_str(&lit.text[pos..whole.start()]);
        match whole.as_str() {
            "{{" => out.push('{'),
            "}}" => out.push('}'),
            _ => {
                let spec = caps.get(1).map_or("", |m| m.as_str());
                let raw_key = py_strip(spec.split(':').next().unwrap_or("")).to_string();
                let named = !raw_key.is_empty() && !is_py_digits(&raw_key);
                let name = match seen_specs.get(&raw_key) {
                    Some(name) if named => name.clone(),
                    _ => {
                        let base = name_for(spec);
                        let count = used.entry(base.clone()).or_insert(0);
                        *count += 1;
                        let name = if *count == 1 {
                            base
                        } else {
                            format!("{base}{count}")
                        };
                        if named {
                            seen_specs.insert(raw_key, name.clone());
                        }
                        names.push(name.clone());
                        name
                    }
                };
                out.push(HOLE_MARK);
                out.push_str(&name);
                out.push(HOLE_MARK);
            }
        }
        pos = whole.end();
    }
    out.push_str(&lit.text[pos..]);
    let text = help_tokens(&out, &mut names);
    (marks_to_holes(&text), names)
}

/// Every template in these crates' sources, in first-seen order.
pub fn extract(root: &Path, crates: &[&str]) -> Result<Vec<Template>, String> {
    static ANY_LETTER: LazyLock<Regex> = LazyLock::new(|| re(r"[A-Za-z]"));
    let mut templates: Vec<Template> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for krate in crates {
        let files = util::walk_files(&root.join("crates").join(krate).join("src"));
        for path in files
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        {
            let rel = util::rel(path, root);
            let slashed = format!("/{rel}");
            if SKIP_PATH_PARTS.iter().any(|part| slashed.contains(part)) {
                continue;
            }
            let lits = literals_in(&util::read_text(path)?);
            // A lowercase word the file compares against (`kind ==
            // "browse"`) is a key where a struct field is set to it, too.
            let keys: HashSet<&str> = lits
                .iter()
                .filter(|l| l.is_key && full_match(&LOWER_WORD, &l.text))
                .map(|l| l.text.as_str())
                .collect();
            for lit in &lits {
                if lit.is_field_value && keys.contains(lit.text.as_str()) {
                    continue;
                }
                if skipped(lit) {
                    continue;
                }
                if lit.is_marked {
                    if !ANY_LETTER.is_match(&PLACEHOLDER_RE.replace_all(&lit.text, "")) {
                        continue;
                    }
                } else if !is_player_text(&lit.text, lit.is_format, lit.is_key) {
                    continue;
                }
                let (msgid, names) = to_template(lit);
                let msgid = msgid.trim_matches('\n').to_string();
                if py_strip(&msgid).is_empty() {
                    continue;
                }
                // The file, not the line: a reference that moved with every
                // edit would make the template stale on unrelated changes.
                match index.get(&msgid) {
                    Some(&k) => {
                        if !templates[k].refs.contains(&rel) {
                            templates[k].refs.push(rel.clone());
                        }
                    }
                    None => {
                        index.insert(msgid.clone(), templates.len());
                        templates.push(Template {
                            msgid,
                            refs: vec![rel.clone()],
                            placeholders: names,
                            line: lit.line,
                        });
                    }
                }
            }
        }
    }
    Ok(templates)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str) -> Vec<String> {
        literals_in(src)
            .into_iter()
            .filter(|l| !skipped(l) && is_player_text(&l.text, l.is_format, l.is_key))
            .map(|l| to_template(&l).0)
            .collect()
    }

    #[test]
    fn spoken_lines_are_found_and_plumbing_is_not() {
        let src = r#"
            fn f(ctx: &mut Ctx, miles: f64, n: u32) {
                ctx.say(&format!("{} miles to go, {:.0} stops", fmt_miles(miles), n));
                ctx.say("Settings");
                log::warn!("Not spoken at all");
                let p = Path::new("data/world.json");
                if kind == "browse" {}
                ctx.say(&format!("{n} load{}", if n == 1 { "" } else { "s" }));
            }
            #[cfg(test)]
            mod tests { fn g() { say("Test only line"); } }
        "#;
        assert_eq!(
            texts(src),
            [
                "{miles} miles to go, {n} stops",
                "Settings",
                "{n} load{s_ending}",
            ]
        );
    }

    #[test]
    fn help_tokens_become_holes() {
        let lit = Literal {
            text: "Press {{pad:horn}} to sound the horn".into(),
            ..Literal::default()
        };
        assert_eq!(
            to_template(&lit),
            (
                "Press {pad_horn} to sound the horn".into(),
                vec!["pad_horn".into()]
            )
        );
    }

    #[test]
    fn raw_strings_chars_and_lifetimes_lex() {
        let toks = lex("r#\"a \"b\"\"# 'x' '\\n' 'a x \"c\\u{e9}\"");
        let kinds: Vec<Kind> = toks.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            [
                Kind::Str,
                Kind::Char,
                Kind::Char,
                Kind::Lifetime,
                Kind::Ident,
                Kind::Str
            ]
        );
        assert_eq!(toks[0].text, "a \"b\"");
        assert_eq!(toks[5].text, "c\u{e9}");
    }
}
