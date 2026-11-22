//! Translating the game's spoken text.
//!
//! The game says everything in English, built with `format!` at hundreds of
//! call sites. Rather than thread a translation call through every one of
//! them, each finished line is translated where it leaves for the speech
//! channels (`GameContext::say` and `say_event`). A catalog line is a
//! template: `It is {tod}.` matches "It is 3:15 PM." and hands `3:15 PM` to
//! the translation, `Es la hora: {tod}.` or however the language puts it.
//!
//! A line no template fills is split into sentences, and each sentence is
//! tried on its own. A sentence is translated whole or not at all: it may
//! be split into clauses only when every clause that is not a name
//! translates, and a template whose holes would keep more English words
//! than the template itself supplies is passed over. So a line never comes
//! out mixed word by word ("Izquierda and Right arrows"); what is not
//! translated stays English, and a missing or partial catalog never costs
//! the player a line, only its language. English itself is the identity:
//! no catalog, no lookups.
//!
//! A menu row's position cue ("4 of 8.") is translated apart from the row
//! (see [`POSITION_CUE`]), so the cue never depends on the row matching.
//!
//! The catalogs are gettext PO files in the repository's `lang/` folder;
//! `lang/README.md` is the translator's guide. `cargo tools l10n-extract`
//! rebuilds the template list (`lang/freight-fate.pot`) from the source.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;

use aho_corasick::{AhoCorasick, MatchKind};
use parking_lot::Mutex;

pub mod matcher;
pub mod plural;
pub mod po;
mod render;

use matcher::{hole_names, parse_source, parse_translation, Part, Seg, Template, TemplateSet};
use render::has_sentence_break;

/// The catalog entry for a menu row's position cue, as `menu.rs` builds it.
pub const POSITION_CUE: &str = "{position} of {count}.";

/// The setting value that follows the operating system's language.
pub const AUTO: &str = "auto";
/// English: the language the game is written in.
pub const ENGLISH: &str = "en";

/// A language the game can speak.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language {
    /// The catalog's file stem and the settings value: `es`, `pt_BR`.
    pub code: &'static str,
    /// The language's name in English.
    pub name: &'static str,
    /// The language's name in that language.
    pub autonym: &'static str,
}

impl Language {
    /// The settings row's name: the English name with the language's own
    /// name after it, "Spanish (Español)", or one name when they are the same.
    pub fn label(&self) -> String {
        let Language { name, autonym, .. } = *self;
        if name == autonym {
            autonym.to_string()
        } else {
            format!("{name} ({autonym})")
        }
    }
}

/// Every language with a catalog, English first, then in settings order.
pub const LANGUAGES: &[Language] = &[
    Language {
        code: "en",
        name: "English",
        autonym: "English",
    },
    Language {
        code: "es",
        name: "Spanish",
        autonym: "Español",
    },
    Language {
        code: "fr",
        name: "French",
        autonym: "Français",
    },
    Language {
        code: "de",
        name: "German",
        autonym: "Deutsch",
    },
    Language {
        code: "pt_BR",
        name: "Brazilian Portuguese",
        autonym: "Português do Brasil",
    },
    Language {
        code: "it",
        name: "Italian",
        autonym: "Italiano",
    },
    Language {
        code: "ru",
        name: "Russian",
        autonym: "Русский",
    },
    Language {
        code: "pl",
        name: "Polish",
        autonym: "Polski",
    },
    Language {
        code: "zh_CN",
        name: "Simplified Chinese",
        autonym: "简体中文",
    },
    Language {
        code: "ja",
        name: "Japanese",
        autonym: "日本語",
    },
    Language {
        code: "tr",
        name: "Turkish",
        autonym: "Türkçe",
    },
    Language {
        code: "vi",
        name: "Vietnamese",
        autonym: "Tiếng Việt",
    },
];

/// Languages a translator added as a file alone, beyond [`LANGUAGES`].
static ADDED: std::sync::OnceLock<Vec<Language>> = std::sync::OnceLock::new();

/// Offer languages that came as a catalog file with no entry in
/// [`LANGUAGES`]: `(code, name, autonym)`, read from the files' headers.
/// The game calls this once, before the language is first resolved; a
/// later call changes nothing. Codes already known are skipped.
pub fn add_languages(found: impl IntoIterator<Item = (String, String, String)>) {
    let mut added: Vec<Language> = Vec::new();
    for (code, name, autonym) in found {
        if language(&code).is_some() || added.iter().any(|lang| lang.code == code) {
            continue;
        }
        // Leaked once per language at launch: the settings row and the
        // resolved code hand these out as `&'static str` like the built-ins.
        added.push(Language {
            code: Box::leak(code.into_boxed_str()),
            name: Box::leak(name.into_boxed_str()),
            autonym: Box::leak(autonym.into_boxed_str()),
        });
    }
    let _ = ADDED.set(added);
}

/// Every language the game has: [`LANGUAGES`], then any added from a file.
pub fn languages() -> impl Iterator<Item = &'static Language> {
    LANGUAGES.iter().chain(ADDED.get().into_iter().flatten())
}

/// The language a code names, if the game has it.
pub fn language(code: &str) -> Option<&'static Language> {
    languages().find(|lang| lang.code == code)
}

/// The catalog for an operating-system locale tag ("es-MX", "pt_BR.UTF-8",
/// "zh-Hans-CN"), or `None` when the game has no catalog for it.
///
/// Any Portuguese gets the Brazilian catalog and any Chinese the simplified
/// one: closer than English for the player, and a screen reader set to
/// either reads them.
pub fn match_locale(tag: &str) -> Option<&'static str> {
    let tag = tag.split(['.', '@']).next().unwrap_or(tag);
    let base = tag
        .split(['-', '_'])
        .next()
        .unwrap_or(tag)
        .to_ascii_lowercase();
    let code = match base.as_str() {
        "pt" => "pt_BR",
        "zh" => "zh_CN",
        other => other,
    };
    // An added catalog may be regional ("pt_PT"); the whole tag picks it
    // before the base language does.
    let whole = tag.replace('-', "_");
    language(&whole)
        .or_else(|| language(code))
        .map(|lang| lang.code)
}

/// The catalog a language setting asks for: `auto` follows `system` (the
/// operating system's locale tag), an unknown value falls back to English.
pub fn resolve(setting: &str, system: Option<&str>) -> &'static str {
    if setting == AUTO || setting.is_empty() {
        return system.and_then(match_locale).unwrap_or(ENGLISH);
    }
    language(setting).map_or(ENGLISH, |lang| lang.code)
}

/// Lines whose translation is remembered (the drive repeats itself).
const CACHE_LIMIT: usize = 8192;

/// One language's compiled catalog.
pub struct Catalog {
    language: String,
    exact: HashMap<String, String>,
    /// The trimmed multi-sentence exact entries, found inside longer lines.
    embedded: Option<AhoCorasick>,
    embedded_keys: Vec<String>,
    templates: TemplateSet,
    /// The translation of [`POSITION_CUE`], when the catalog has one.
    position_cue: Option<String>,
    /// Entries that were rejected while compiling, for the checker.
    pub problems: Vec<String>,
}

impl Catalog {
    /// Compile a PO file's text. Untranslated (empty) and fuzzy entries are
    /// left out; a broken entry is recorded in `problems` and skipped, so
    /// one typo never costs the whole language.
    pub fn from_po(language: &str, text: &str) -> Result<Self, po::PoError> {
        let entries = po::parse(text)?;
        let mut exact = HashMap::new();
        let mut templates = Vec::new();
        let mut problems = Vec::new();
        let mut position_cue = None;
        let mut tails = Vec::new();
        for entry in entries {
            // An entry with a msgctxt tells a translator apart a second use
            // of the same English (the controller's Back button beside the
            // Back row); the game matches finished lines, so the plain entry
            // is the one it speaks.
            if entry.msgid.is_empty()
                || entry.msgstr.is_empty()
                || entry.is_fuzzy()
                || entry.msgctxt.is_some()
            {
                continue;
            }
            if entry.msgid == POSITION_CUE {
                if entry.msgstr.contains("{position}") && entry.msgstr.contains("{count}") {
                    position_cue = Some(entry.msgstr.clone());
                } else {
                    problems.push(format!(
                        "line {}: the position cue needs both {{position}} and {{count}}",
                        entry.line
                    ));
                }
                continue;
            }
            let segs = parse_source(&entry.msgid);
            let names = hole_names(&segs);
            if names.is_empty() {
                exact.insert(entry.msgid, entry.msgstr);
                continue;
            }
            // A template is matched against a trimmed piece, so a piece of a
            // longer line (" Career objective: {terminal_text}") is compiled
            // without the spaces that joined it.
            let segs = parse_source(entry.msgid.trim());
            let translation = parse_translation(entry.msgstr.trim());
            let unknown: Vec<&str> = translation
                .iter()
                .filter_map(|part| match part {
                    Part::Hole(name) | Part::Plural(name, _) => Some(name.as_str()),
                    Part::Lit(_) => None,
                })
                .filter(|name| !names.contains(name))
                .collect();
            if !unknown.is_empty() {
                problems.push(format!(
                    "line {}: the translation names {{{}}}, which the English line does not have",
                    entry.line,
                    unknown.join("}, {")
                ));
                continue;
            }
            // "{flavor} Dispatch starts you on a short standard load." is
            // also the sentence after the lead-in: a line split into
            // sentences reaches it without its hole.
            if let ([Seg::Hole(a), Seg::Lit(tail)], [Part::Hole(b), Part::Lit(out)]) =
                (segs.as_slice(), translation.as_slice())
            {
                if a == b
                    && tail.starts_with(' ')
                    && out.starts_with(' ')
                    && tail.trim_start().starts_with(char::is_uppercase)
                {
                    tails.push((tail.trim().to_string(), out.trim().to_string()));
                }
            }
            match Template::new(segs, translation) {
                Ok(template) => templates.push(template),
                // A joiner like "{a}, {b}" is not an error; the splitter
                // already handles what it would.
                Err(_) => continue,
            }
        }
        for (key, value) in tails {
            exact.entry(key).or_insert(value);
        }
        // A fragment like "Billboard: " is also found by its bare words,
        // the way a piece reaches it after trimming.
        let trimmed: Vec<(String, String)> = exact
            .iter()
            .filter(|(key, _)| key.trim() != key.as_str() && !key.trim().is_empty())
            .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
            .collect();
        for (key, value) in trimmed {
            exact.entry(key).or_insert(value);
        }
        // Match the passage, not its separator: the longest match would eat
        // trailing whitespace and then fail the following word-boundary check.
        let mut embedded_keys: Vec<String> = exact
            .keys()
            .filter(|key| key.trim() == key.as_str() && has_sentence_break(key))
            .cloned()
            .collect();
        embedded_keys.sort();
        let embedded = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostLongest)
            .build(&embedded_keys)
            .map_err(|err| log::error!("translations could not be indexed: {err}"))
            .ok();
        Ok(Self {
            language: language.to_string(),
            embedded,
            embedded_keys,
            exact,
            templates: TemplateSet::new(templates),
            position_cue,
            problems,
        })
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    /// "4 of 8." in the catalog's language, or as English has it.
    fn position_cue(&self, position: &str, count: &str) -> String {
        let Some(cue) = &self.position_cue else {
            return format!("{position} of {count}.");
        };
        let number = |raw: &str| {
            plural::localize_number(&self.language, raw).unwrap_or_else(|| raw.to_string())
        };
        cue.replace("{position}", &number(position))
            .replace("{count}", &number(count))
    }

    /// How many lines the catalog translates.
    pub fn len(&self) -> usize {
        self.exact.len() + self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Text from outside the game that is spoken as it arrives, in English:
/// the driver newspaper's lines, which the site writes. Kept out of the
/// catalog the way a name in a `{station}` hole is, and never logged as a
/// miss, since no translator could supply them. Stored without a final full
/// stop, because a menu row is spoken with one added.
static KEPT_VERBATIM: std::sync::RwLock<Option<HashSet<String>>> = std::sync::RwLock::new(None);

fn verbatim_key(line: &str) -> &str {
    line.trim().trim_end_matches('.')
}

/// Speak `lines` untranslated from now on, in place of the last set kept.
/// An empty list keeps nothing.
pub fn keep_verbatim(lines: impl IntoIterator<Item = String>) {
    let set: HashSet<String> = lines
        .into_iter()
        .map(|line| verbatim_key(&line).to_string())
        .filter(|line| !line.is_empty())
        .collect();
    let mut kept = KEPT_VERBATIM.write().unwrap_or_else(|e| e.into_inner());
    *kept = if set.is_empty() { None } else { Some(set) };
}

/// Whether `line` is one of the lines [`keep_verbatim`] holds.
pub fn is_kept_verbatim(line: &str) -> bool {
    let kept = KEPT_VERBATIM.read().unwrap_or_else(|e| e.into_inner());
    kept.as_ref()
        .is_some_and(|set| set.contains(verbatim_key(line)))
}

/// Translates finished English lines into the player's language.
pub struct Translator {
    catalog: Option<Catalog>,
    cache: Mutex<HashMap<String, String>>,
    misses: Option<Mutex<MissLog>>,
}

/// Where untranslated lines are written, once each, for translators.
struct MissLog {
    path: PathBuf,
    seen: HashSet<String>,
}

impl Default for Translator {
    fn default() -> Self {
        Self::english()
    }
}

impl Translator {
    /// English: every line passes through untouched.
    pub fn english() -> Self {
        Self {
            catalog: None,
            cache: Mutex::new(HashMap::new()),
            misses: None,
        }
    }

    pub fn new(catalog: Catalog) -> Self {
        Self {
            catalog: Some(catalog),
            cache: Mutex::new(HashMap::new()),
            misses: None,
        }
    }

    /// Append every line that is not fully translated to `path`, once per
    /// session: the list a translator works from.
    pub fn log_misses_to(mut self, path: PathBuf) -> Self {
        self.misses = Some(Mutex::new(MissLog {
            path,
            seen: HashSet::new(),
        }));
        self
    }

    /// The catalog's language code, `en` for English.
    pub fn language(&self) -> &str {
        self.catalog.as_ref().map_or(ENGLISH, Catalog::language)
    }

    pub fn is_english(&self) -> bool {
        self.catalog.is_none()
    }

    /// The line in the player's language, as much of it as the catalog
    /// covers.
    pub fn translate<'a>(&self, line: &'a str) -> Cow<'a, str> {
        let Some(catalog) = &self.catalog else {
            return Cow::Borrowed(line);
        };
        if is_kept_verbatim(line) {
            return Cow::Borrowed(line);
        }
        if let Some(hit) = self.cache.lock().get(line) {
            return Cow::Owned(hit.clone());
        }
        let mut missed = Vec::new();
        let out = render::translate(catalog, line, &mut missed);
        if let Some(log) = &self.misses {
            log.lock().record(&missed);
        }
        let mut cache = self.cache.lock();
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(line.to_string(), out.clone());
        Cow::Owned(out)
    }
}

impl MissLog {
    fn record(&mut self, missed: &[String]) {
        let fresh: Vec<&String> = missed
            .iter()
            .filter(|line| self.seen.insert((*line).clone()))
            .collect();
        if fresh.is_empty() {
            return;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path);
        match file {
            Ok(mut file) => {
                for line in fresh {
                    let _ = writeln!(file, "{}", line.replace('\n', "\\n"));
                }
            }
            Err(err) => log::warn!(
                "Could not write untranslated lines to {}: {err}",
                self.path.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests;
