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
//! `lang/README.md` is the translator's guide. `tools/l10n/extract.py`
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

use matcher::{hole_names, parse_source, parse_translation, Part, Seg, Template, TemplateSet};

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
    /// The language's name in that language, as the settings row reads it.
    pub autonym: &'static str,
}

/// Every language with a catalog, English first, then in settings order.
pub const LANGUAGES: &[Language] = &[
    Language {
        code: "en",
        autonym: "English",
    },
    Language {
        code: "es",
        autonym: "Español",
    },
    Language {
        code: "fr",
        autonym: "Français",
    },
    Language {
        code: "de",
        autonym: "Deutsch",
    },
    Language {
        code: "pt_BR",
        autonym: "Português do Brasil",
    },
    Language {
        code: "it",
        autonym: "Italiano",
    },
    Language {
        code: "ru",
        autonym: "Русский",
    },
    Language {
        code: "pl",
        autonym: "Polski",
    },
    Language {
        code: "zh_CN",
        autonym: "简体中文",
    },
    Language {
        code: "ja",
        autonym: "日本語",
    },
];

/// Languages a translator added as a file alone, beyond [`LANGUAGES`].
static ADDED: std::sync::OnceLock<Vec<Language>> = std::sync::OnceLock::new();

/// Offer languages that came as a catalog file with no entry in
/// [`LANGUAGES`]: `(code, autonym)` pairs, read from the files' headers.
/// The game calls this once, before the language is first resolved; a
/// later call changes nothing. Codes already known are skipped.
pub fn add_languages(found: impl IntoIterator<Item = (String, String)>) {
    let mut added: Vec<Language> = Vec::new();
    for (code, autonym) in found {
        if language(&code).is_some() || added.iter().any(|lang| lang.code == code) {
            continue;
        }
        // Leaked once per language at launch: the settings row and the
        // resolved code hand these out as `&'static str` like the built-ins.
        added.push(Language {
            code: Box::leak(code.into_boxed_str()),
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

/// Placeholder words whose value is a place or a proper name: never sent
/// through the catalog, so "Mobile" stays a city in Alabama and never
/// becomes a phone. A hole is a name when one of its words (split at `_`,
/// trailing digits dropped) starts with one of these: `city2`, `the_city`,
/// `destination`, `driver_name`. Matched by word, not anywhere in the name,
/// so `replace` is not a place and `open_road_target` is not a road.
const PROPER_NAME_HOLES: &[&str] = &[
    "city", "town", "village", "county", "state", "road", "street", "highway", "route", "exit",
    "where", "dest", "origin", "home", "station", "place", "artist", "song", "handle", "driver",
    "shipper", "receiver", "brand", "chain", "carrier", "company", "landmark", "rcity",
];

/// A last word that makes a hole a value about a place, not its name:
/// `place_callouts`, `exit_speed`, `road_class`.
const NOT_NAME_HEADS: &[&str] = &[
    "callouts",
    "direction",
    "class",
    "step",
    "summary",
    "done",
    "next",
    "word",
    "speed",
    "hint",
    "note",
    "clause",
    "target",
    "level",
    "mph",
    "s",
    "min",
];

/// Holes that read like a place but hold a condition ("charged").
const NOT_NAME_HOLES: &[&str] = &["air_state", "brake_state"];

/// The help's control names (`{{engine}}` in the source, `{control_engine}`
/// in the catalog): a key or button name, kept as the keyboard reads it.
const CONTROL_HOLE_PREFIXES: &[&str] = &["control_", "key_", "pad_"];

fn is_proper_name_hole(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    if CONTROL_HOLE_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
    {
        return true;
    }
    if NOT_NAME_HOLES.contains(&name.as_str()) {
        return false;
    }
    let words: Vec<&str> = name
        .split('_')
        .map(|word| word.trim_end_matches(|c: char| c.is_ascii_digit()))
        .filter(|word| !word.is_empty())
        .collect();
    if words
        .last()
        .is_none_or(|last| NOT_NAME_HEADS.contains(last))
    {
        return false;
    }
    words
        .iter()
        .any(|word| PROPER_NAME_HOLES.iter().any(|part| word.starts_with(part)))
}

/// How deep a line is split and its captures translated in turn.
// A drive-start line nests that deep: the line, its objective, the trip
// progress inside it, and the distance inside that.
const MAX_DEPTH: usize = 6;
/// Lines whose translation is remembered (the drive repeats itself).
const CACHE_LIMIT: usize = 8192;

/// One language's compiled catalog.
pub struct Catalog {
    language: String,
    exact: HashMap<String, String>,
    /// The multi-sentence exact entries, found inside longer lines.
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
        let mut embedded_keys: Vec<String> = exact
            .keys()
            .filter(|key| has_sentence_break(key))
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
        if let Some(hit) = self.cache.lock().get(line) {
            return Cow::Owned(hit.clone());
        }
        let mut missed = Vec::new();
        let out = match split_position_cue(line) {
            Some((row, position, count)) => {
                let row = Pass {
                    catalog,
                    missed: &mut missed,
                }
                .line(row, 0)
                .unwrap_or_else(|| row.to_string());
                let cue = catalog.position_cue(position, count);
                let sep = if ends_full_width(&row) { "" } else { " " };
                format!("{row}{sep}{cue}")
            }
            None => Pass {
                catalog,
                missed: &mut missed,
            }
            .line(line, 0)
            .map(|out| {
                if starts_lowercase(line) {
                    out
                } else {
                    capitalize_first_letter(&out)
                }
            })
            .unwrap_or_else(|| line.to_string()),
        };
        let out = contract_articles(&catalog.language, out);
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

/// One translation of one line: the catalog, and the pieces nothing
/// translated.
struct Pass<'c, 'm> {
    catalog: &'c Catalog,
    missed: &'m mut Vec<String>,
}

impl Pass<'_, '_> {
    /// A whole line or one of its pieces. `None` when nothing in it was
    /// translated; the piece is then recorded as missed.
    fn line(&mut self, text: &str, depth: usize) -> Option<String> {
        let out = self.piece(text, depth, true);
        if out.is_none() && has_words(text) {
            self.missed.push(text.trim().to_string());
        }
        out
    }

    fn piece(&mut self, text: &str, depth: usize, split: bool) -> Option<String> {
        let core = text.trim();
        if core.is_empty() {
            return None;
        }
        let lead = &text[..text.len() - text.trim_start().len()];
        let trail = &text[text.trim_end().len()..];
        let wrap = |inner: String| format!("{lead}{inner}{trail}");
        if let Some(hit) = self.catalog.exact.get(core) {
            return Some(wrap(hit.clone()));
        }
        // "ELD status on duty not driving" inside "ELD: {summary}" is the
        // catalog's whole sentence without its full stop. A whole sentence
        // beats any template a loose fragment of it fills ("{driven} driving").
        if !core.ends_with(['.', '!', '?', ':', ';', ',']) {
            if let Some(hit) = self.catalog.exact.get(&format!("{core}.")) {
                let out = hit
                    .strip_suffix(['.', '\u{3002}'])
                    .map(str::to_string)
                    .unwrap_or_else(|| hit.clone());
                return Some(wrap(out));
            }
        }
        // "In the right lane." is the catalog's exact "In the right lane"
        // with a full stop: that beats any template the stopped line also
        // fills ("{at} the {noun}.").
        if let Some(hit) = core
            .strip_suffix('.')
            .filter(|bare| !bare.ends_with(['.', ':', ',', ';']))
            .and_then(|bare| self.catalog.exact.get(bare))
        {
            let out = self.with_full_stop(hit.clone());
            return Some(wrap(out));
        }
        let direct = self.lookup(core, depth);
        // "Dispatch board." is the catalog's "Dispatch board" with the full
        // stop a menu adds after an item.
        // A lead-in like "Billboard:" keeps its colon the same way.
        // The heavier of the two wins: "Deliveries: 1, 0 percent on time."
        // is the catalog's "Deliveries: {deliveries}, {f} percent on time"
        // with a stop, not a looser template that ends in one.
        let stopped = core
            .chars()
            .next_back()
            .filter(|c| matches!(c, '.' | ':' | ',' | ';'))
            .and_then(|mark| {
                let bare = &core[..core.len() - mark.len_utf8()];
                if bare.ends_with(['.', ':', ',', ';']) {
                    return None;
                }
                let (weight, mut out) = self.lookup(bare, depth)?;
                if mark == '.' {
                    out = self.with_full_stop(out);
                } else {
                    out.push(mark);
                }
                Some((weight, out))
            });
        let best = match (direct, stopped) {
            (Some(direct), Some(stopped)) if stopped.0 > direct.0 => Some(stopped),
            (Some(direct), _) => Some(direct),
            (None, stopped) => stopped,
        };
        if let Some((_, out)) = best {
            return Some(wrap(out));
        }
        if depth >= MAX_DEPTH {
            return None;
        }
        // "ELD status on duty not driving" after "ELD: " is the catalog's
        // sentence without its full stop.
        if !core.ends_with(['.', '!', '?', ':', ';', ',']) {
            if let Some((_, out)) = self.lookup(&format!("{core}."), depth) {
                let out = out
                    .strip_suffix(['.', '\u{3002}'])
                    .map(str::to_string)
                    .unwrap_or(out);
                return Some(wrap(out));
            }
        }
        if split {
            if let Some(out) = self.embedded(core, depth) {
                return Some(wrap(out));
            }
        }
        for (clauses, splitter) in [
            (false, split_sentences as fn(&str) -> Vec<(&str, &str)>),
            (true, split_clauses),
        ] {
            // A capture is split into sentences only when it holds several;
            // clauses always, since they must all translate.
            if !clauses && !split {
                continue;
            }
            let pieces = splitter(core);
            if pieces.len() < 2 {
                continue;
            }
            let mut any = false;
            let mut whole = true;
            let mut out = String::new();
            let mut missed = Vec::new();
            for (piece, sep) in &pieces {
                let translated = {
                    let mut inner = Pass {
                        catalog: self.catalog,
                        missed: &mut missed,
                    };
                    // A sentence is a line of its own; only clauses go deeper.
                    let deeper = if clauses { depth + 1 } else { depth };
                    let out = inner.line(piece, deeper);
                    // A sentence opens with a capital in the translation as
                    // in English: "451 miles remaining" is "Quedan 451 millas".
                    if clauses || starts_lowercase(piece) {
                        out
                    } else {
                        out.map(|out| capitalize_first_letter(&out))
                    }
                };
                any |= translated.is_some();
                whole &= translated.is_some() || !has_words(piece) || is_name_like(piece);
                let piece_out = translated.unwrap_or_else(|| (*piece).to_string());
                let sep = if sep.trim().is_empty() && ends_full_width(&piece_out) {
                    ""
                } else {
                    sep
                };
                out.push_str(&piece_out);
                out.push_str(sep);
            }
            // Clauses are words of one sentence: translated all together,
            // or the sentence stays English rather than come out mixed.
            if any && (whole || !clauses) {
                self.missed.extend(missed);
                return Some(wrap(out));
            }
        }
        None
    }

    /// A line with a whole multi-sentence catalog entry inside it, such as a
    /// billboard after its lead-in: the entry is translated in place and
    /// the text around it piece by piece. Splitting into sentences first
    /// would break the entry into sentences the catalog does not have.
    fn embedded(&mut self, core: &str, depth: usize) -> Option<String> {
        let finder = self.catalog.embedded.as_ref()?;
        let mut out = String::new();
        let mut last = 0;
        let mut any = false;
        for found in finder.find_iter(core) {
            let (start, end) = (found.start(), found.end());
            let opens = start == 0 || core[..start].ends_with(char::is_whitespace);
            let closes = end == core.len()
                || core[end..].starts_with(|c: char| c.is_whitespace() || c.is_ascii_punctuation());
            if !opens || !closes {
                continue;
            }
            let gap = &core[last..start];
            if !gap.trim().is_empty() {
                let translated = self.line(gap, depth);
                out.push_str(translated.as_deref().unwrap_or(gap));
            } else {
                out.push_str(gap);
            }
            let key = &self.catalog.embedded_keys[found.pattern().as_usize()];
            out.push_str(&self.catalog.exact[key]);
            last = end;
            any = true;
        }
        if !any {
            return None;
        }
        let gap = &core[last..];
        if !gap.trim().is_empty() {
            let translated = self.line(gap, depth);
            out.push_str(translated.as_deref().unwrap_or(gap));
        } else {
            out.push_str(gap);
        }
        Some(out)
    }

    /// A translated piece with its sentence ended the way the language ends
    /// one, unless the translation already did.
    /// A whole-piece match: exact, then a template, then the same with the
    /// first letter's case flipped, since a fragment that opens a sentence
    /// ("Confirmed truck parking.") is the catalog's lowercase fragment.
    /// With the match's weight: a template's literal letters, or
    /// `usize::MAX` for an exact entry.
    fn lookup(&mut self, core: &str, depth: usize) -> Option<(usize, String)> {
        if let Some(hit) = self.catalog.exact.get(core) {
            return Some((usize::MAX, hit.clone()));
        }
        let mut chars = core.chars();
        let first = chars.next()?;
        let upper = first.is_uppercase();
        let flipped: Option<String> = if upper {
            Some(first.to_lowercase().chain(chars).collect())
        } else if first.is_lowercase() {
            Some(first.to_uppercase().chain(chars).collect())
        } else {
            None
        };
        let direct = self.template(core, depth);
        // A fragment the game capitalised to open a sentence ("Divided, two
        // lanes your side") fills its lowercase template, and that template
        // wins over a looser one that would swallow "Divided" into a hole.
        if let Some(lower) = flipped.as_deref().filter(|_| upper) {
            if !self.catalog.exact.contains_key(lower) {
                if let Some((weight, out)) = self.template(lower, depth) {
                    if direct.as_ref().is_none_or(|(best, _)| weight > *best) {
                        return Some((weight, capitalize(&out)));
                    }
                }
            }
        }
        if direct.is_some() {
            return direct;
        }
        let flipped = flipped?;
        let hit = self.catalog.exact.get(&flipped)?;
        Some((
            usize::MAX,
            if upper { capitalize(hit) } else { hit.clone() },
        ))
    }

    fn with_full_stop(&self, mut out: String) -> String {
        if !out.trim_end().ends_with(['.', '!', '?', '。', '！', '？']) {
            let base = self.catalog.language.split('_').next().unwrap_or_default();
            out.push(if matches!(base, "zh" | "ja") {
                '。'
            } else {
                '.'
            });
        }
        out
    }

    /// The best template the line fills, rendered with its captures, and
    /// that template's literal weight.
    /// A name with a catalog frame of its own whose every hole is a name
    /// ("freight terminal Macon Terminal in Macon"): the frame translated,
    /// the names kept.
    fn place_phrase(&self, raw: &str) -> Option<String> {
        if !raw.contains(' ') {
            return None;
        }
        let (template, captures) = self.catalog.templates.best_match(raw, |template, caps| {
            // "{place_name} in {city}" has few letters of its own, but here
            // every hole is a name, so it is a frame around names.
            template.segs.iter().any(|seg| {
                    matches!(seg, Seg::Lit(text) if text.ends_with(" in ") || text.contains(" near "))
                })
                && caps.iter().all(|(name, value)| {
                    is_proper_name_hole(name) && reads_as_name(value) && !value.contains(": ")
                })
        })?;
        let mut out = String::new();
        for part in &template.translation {
            match part {
                Part::Lit(text) => out.push_str(text),
                Part::Hole(name) => {
                    let raw = captures.get(name.as_str()).copied().unwrap_or_default();
                    let bare = name
                        .starts_with("the_")
                        .then(|| raw.strip_prefix("the ").or(raw.strip_prefix("The ")))
                        .flatten();
                    out.push_str(bare.unwrap_or(raw));
                }
                Part::Plural(..) => return None,
            }
        }
        Some(out)
    }

    fn template(&mut self, core: &str, depth: usize) -> Option<(usize, String)> {
        // A one-sentence template never swallows several sentences into a
        // hole: "It is {tod}." must not match "It is 9:00. Fuel is low.",
        // which the sentence splitter translates whole.
        let (template, captures) = self.catalog.templates.best_match(core, |template, caps| {
            // "{place}{state} on {highway}" has few letters of its own, but
            // its holes are names: it matches when what fills them reads as
            // names ("Groton", ", South Dakota", "US-12"), never words a loose
            // match would swallow ("ELD status on duty not driving").
            if template.numbers_only()
                && !caps.iter().all(|(name, value)| {
                    is_number(value) || (is_proper_name_hole(name) && reads_as_name(value))
                })
            {
                return false;
            }
            // A name never holds a lead-in: "{carrier} is looking for ..."
            // must not take "Career objective: Northstar Freight Lines" as
            // the carrier and leave the label English; nor does a hole that
            // opens a template: "{name} Company Yard" must not take
            // "company yard: Chicago" and say the label twice.
            if caps.iter().any(|(name, value)| {
                value.contains(": ")
                    && (is_proper_name_hole(name)
                        || (matches!(template.segs.first(), Some(Seg::Hole(first)) if first == name)
                            && !template.filled.contains(':')))
            }) {
                return false;
            }
            // A name that opens a template reads as one: "{the_city} metro
            // freight market" must not take "12 tons of general freight
            // from Aberdeen yard in Aberdeen to the Fargo" as its city.
            if let Some(Seg::Hole(first)) = template.segs.first() {
                if is_proper_name_hole(first)
                    && caps
                        .get(first.as_str())
                        .is_some_and(|value| !reads_as_place(value))
                {
                    return false;
                }
            }
            // Only a hole that ends the template or opens a sentence of it
            // may hold sentences of its own ("ELD: {summary}", "How to play.
            // {controls} {page_title}"): "{pct} percent {direction}
            // ahead{length}. {advice}" must not take a whole help text as
            // its percent and leave it English.
            let multi = has_sentence_break(&template.filled);
            !caps.iter().any(|(name, value)| {
                has_sentence_break(value) && !(multi && holds_sentences(&template.segs, name))
            })
        })?;
        let mut out = String::new();
        // English words the holes would keep, untranslated, inside the
        // translated frame.
        let mut kept_english = 0;
        for part in &template.translation {
            match part {
                Part::Lit(text) => out.push_str(text),
                Part::Hole(name) => {
                    let raw = captures.get(name.as_str()).copied().unwrap_or_default();
                    if is_proper_name_hole(name) && depth < MAX_DEPTH {
                        // A name hole can hold a place phrase around names:
                        // "retail distribution hub {place_name} in {city}"
                        // says its kind and "in" in the language, the names
                        // as they are.
                        if let Some(phrase) = self.place_phrase(raw) {
                            out.push_str(&phrase);
                            continue;
                        }
                    }
                    if is_proper_name_hole(name) || depth >= MAX_DEPTH {
                        // "{the_city}" holds "the Chicago" in English; the
                        // translation brings its own article, if any.
                        let bare = name
                            .starts_with("the_")
                            .then(|| raw.strip_prefix("the ").or(raw.strip_prefix("The ")))
                            .flatten();
                        out.push_str(bare.unwrap_or(raw));
                    } else if let Some(number) =
                        plural::localize_number(&self.catalog.language, raw)
                    {
                        out.push_str(&number);
                    } else {
                        // A capture is often a phrase the catalog knows
                        // ("heavy traffic"); an unknown one (a number, a
                        // name) is simply kept, and not a miss.
                        let mut ignored = Vec::new();
                        let translated = Pass {
                            catalog: self.catalog,
                            missed: &mut ignored,
                        }
                        .piece(raw, depth + 1, has_sentence_break(raw));
                        // A measure ("2 miles") or a name kept as is is
                        // not a word left behind.
                        if translated.is_none() && !is_name_like(raw) && !is_measure(raw) {
                            kept_english += raw.chars().filter(|c| c.is_alphabetic()).count();
                        }
                        let translated = match translated {
                            // A capture that opens a sentence of the frame
                            // opens with a capital: ". {progress}" is
                            // ". Quedan 451 millas", not ". quedan".
                            Some(text)
                                if !starts_lowercase(raw)
                                    && out.trim_end().ends_with(['.', '!', '?']) =>
                            {
                                Some(capitalize_first_letter(&text))
                            }
                            other => other,
                        };
                        out.push_str(translated.as_deref().unwrap_or(raw));
                    }
                }
                Part::Plural(name, forms) => {
                    let raw = captures.get(name.as_str()).copied().unwrap_or_default();
                    out.push_str(plural::choose(&self.catalog.language, forms, raw));
                }
            }
        }
        // "Lifetime earnings" in "{what} en total" is a word-by-word
        // translation, not a translated line: as much English as frame.
        if kept_english >= template.weight {
            return None;
        }
        // Every source hole must have been captured for the line to match;
        // a template whose segments carry a hole the translation drops is
        // still a faithful match (the translator chose to drop it).
        debug_assert!(template
            .segs
            .iter()
            .all(|seg| !matches!(seg, Seg::Hole(name) if !captures.contains_key(name.as_str()))));
        Some((template.weight, out))
    }
}

/// `text` with its first letter capitalized.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The first letter is lowercase ("the Down arrow ..."): English did not
/// open a sentence with a capital, so the translation need not either.
fn starts_lowercase(text: &str) -> bool {
    text.chars()
        .find(|c| c.is_alphanumeric())
        .is_some_and(char::is_lowercase)
}

/// `text` with its first letter (after any digits or marks) capitalized:
/// "451 millas" stays, "quedan 451 millas" becomes "Quedan 451 millas".
fn capitalize_first_letter(text: &str) -> String {
    let Some((at, first)) = text.char_indices().find(|(_, c)| c.is_alphanumeric()) else {
        return text.to_string();
    };
    if !first.is_lowercase() {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..at]);
    out.extend(first.to_uppercase());
    out.push_str(&text[at + first.len_utf8()..]);
    out
}

/// A number as the game writes one: "3", "1,250", "2.5", "3:15".
fn is_number(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_digit())
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | ':' | '-' | ' '))
}

/// A trailing menu position cue: "Settings. 4 of 8." is the row
/// "Settings." with position 4 of 8. The row must end a sentence, so
/// "Results 26 to 50 of 52." is not one.
fn split_position_cue(line: &str) -> Option<(&str, &str, &str)> {
    let body = line.strip_suffix('.')?;
    let (rest, count) = body.rsplit_once(' ')?;
    let rest = rest.strip_suffix(" of")?;
    let (row, position) = rest.rsplit_once(' ')?;
    let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    if !digits(count) || !digits(position) {
        return None;
    }
    let row = row.trim_end();
    row.ends_with(['.', '!', '?', ':'])
        .then_some((row, position, count))
}

/// A piece that reads as a name or a number, not words to translate:
/// ".977 Smooth Jazz", "Bemidji Company Yard", "US-12". Every word opens
/// with a capital letter, a digit or a mark, and there is more than one
/// word or a digit: a lone capitalized word ("Deliveries") is as likely
/// a label the catalog lacks as a name.
fn is_name_like(text: &str) -> bool {
    let mut words = text.split_whitespace().peekable();
    if words.peek().is_none() {
        return false;
    }
    let capitalized = text.split_whitespace().all(|word| {
        !word
            .chars()
            .find(|c| c.is_alphanumeric())
            .is_some_and(char::is_lowercase)
    });
    capitalized && (text.split_whitespace().count() > 1 || text.chars().any(|c| c.is_ascii_digit()))
}

/// What fills a name hole reads as a name: empty, or every word opens with a
/// capital, a digit or a mark ("Groton", ", South Dakota", "US-12").
fn reads_as_name(text: &str) -> bool {
    text.split_whitespace().all(|word| {
        !word
            .chars()
            .find(|c| c.is_alphanumeric())
            .is_some_and(char::is_lowercase)
    })
}

/// Reads as a place's name with its small words: "Amazon Fulfillment
/// Center KRB1 in Kenosha", "the Fargo", never "12 tons of general freight".
fn reads_as_place(text: &str) -> bool {
    const SMALL: &[&str] = &[
        "the", "of", "in", "near", "and", "at", "on", "de", "la", "du",
    ];
    text.split_whitespace().all(|word| {
        SMALL.contains(&word)
            || !word
                .chars()
                .find(|c| c.is_alphanumeric())
                .is_some_and(char::is_lowercase)
    })
}

/// A number and its unit, as a readout says one: "2 miles", "3:15 PM".
fn is_measure(text: &str) -> bool {
    text.trim_start().starts_with(|c: char| c.is_ascii_digit())
        && text.split_whitespace().count() <= 2
}

fn has_words(text: &str) -> bool {
    text.chars().filter(|c| c.is_alphabetic()).take(2).count() >= 2
}

/// Spanish joins a preposition to the article after it, which
/// two catalog pieces put side by side do not: "por debajo de" + "el
/// límite" is "por debajo del límite". A capital "El" is a name (El Paso)
/// and stays apart.
fn contract_articles(language: &str, out: String) -> String {
    let pairs: &[(&str, &str)] = match language.split('_').next().unwrap_or_default() {
        "es" => &[(" de el ", " del "), (" a el ", " al ")],
        _ => return out,
    };
    let mut out = out;
    for (apart, joined) in pairs {
        if out.contains(apart) {
            out = out.replace(apart, joined);
        }
    }
    out
}

/// The hole may hold whole sentences: it is the template's last part, or
/// a sentence of the template ends before it or opens after it.
fn holds_sentences(segs: &[Seg], hole: &str) -> bool {
    let Some(at) = segs
        .iter()
        .position(|seg| matches!(seg, Seg::Hole(name) if name == hole))
    else {
        return false;
    };
    if at + 1 == segs.len() {
        return true;
    }
    // "{flavor} Dispatch starts you on ...": a sentence of the template
    // opens right after it.
    if let Some(Seg::Lit(text)) = segs.get(at + 1) {
        if text.starts_with(char::is_whitespace)
            && text.trim_start().starts_with(char::is_uppercase)
        {
            return true;
        }
    }
    match at.checked_sub(1).map(|before| &segs[before]) {
        Some(Seg::Lit(text)) => text
            .trim_end()
            .ends_with(['.', '!', '?', '\u{3002}', '\u{FF01}', '\u{FF1F}']),
        _ => false,
    }
}

fn has_sentence_break(text: &str) -> bool {
    split_sentences(text).len() > 1
}

/// Ends in a full-width sentence mark (Chinese, Japanese), after which no
/// space is written.
fn ends_full_width(text: &str) -> bool {
    text.trim_end()
        .ends_with(['。', '！', '？', '：', '；', '，', '、'])
}

/// Sentences, each with the whitespace that followed it: a break is a
/// `.`, `!` or `?` followed by a space and then a capital letter, a digit
/// or a quote.
fn split_sentences(text: &str) -> Vec<(&str, &str)> {
    split_at_marks(text, |text, at| {
        let before = text[..at].chars().next_back();
        if !matches!(before, Some('.' | '!' | '?')) {
            return None;
        }
        let rest = &text[at..];
        let gap = rest.len() - rest.trim_start().len();
        if gap == 0 {
            return None;
        }
        let next = rest[gap..].chars().next()?;
        (next.is_uppercase() || next.is_ascii_digit() || next == '"').then_some(gap)
    })
}

/// Clauses: split at ", ", ": ", "; " and " -- ", keeping the separator.
fn split_clauses(text: &str) -> Vec<(&str, &str)> {
    split_at_marks(text, |text, at| {
        let rest = &text[at..];
        for sep in [", ", ": ", "; ", " -- "] {
            if rest.starts_with(sep) {
                return Some(sep.len());
            }
        }
        None
    })
}

/// Walk `text`; `mark(text, at)` returns the separator length starting at
/// byte `at` when a piece ends there.
fn split_at_marks(text: &str, mark: impl Fn(&str, usize) -> Option<usize>) -> Vec<(&str, &str)> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while at < text.len() {
        if at > start {
            if let Some(len) = mark(text, at) {
                pieces.push((&text[start..at], &text[at..at + len]));
                at += len;
                start = at;
                continue;
            }
        }
        at += text[at..].chars().next().map_or(1, char::len_utf8);
    }
    if start < text.len() {
        pieces.push((&text[start..], ""));
    }
    pieces
}

#[cfg(test)]
mod tests;
