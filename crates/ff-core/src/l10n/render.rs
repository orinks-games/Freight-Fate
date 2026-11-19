//! Match and render finished English lines against a compiled catalog.

use std::collections::HashMap;

use super::matcher::{Part, Seg, Template};
use super::{plural, Catalog};

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

fn is_version_hole(template: &Template, name: &str) -> bool {
    name.eq_ignore_ascii_case("version")
        || name.eq_ignore_ascii_case("format_version")
        || (name == "line"
            && template.segs.windows(2).any(|pair| {
                matches!(pair, [Seg::Lit(before), Seg::Hole(hole)]
                    if hole == name && before.ends_with("Freight Fate "))
            }))
}

fn is_career_name_hole(template: &Template, name: &str) -> bool {
    match name {
        "driver_name" | "career_name" | "save_name" => true,
        // Existing catalogs also use {name} for both careers and translatable
        // labels. Identify the career frames without changing their PO keys.
        "name" => [
            "Continue X:",
            "X: level ",
            "Welcome back, X.",
            "Load X,",
            "Load the newest save for X.",
            "Manage X.",
            "X: career from ",
            "X was made in Freight Fate ",
            "There is already a career named X from ",
            "Loaded over existing driver named X.",
            "X could not be opened.",
            "X reset.",
            "X could not be deleted.",
            "X deleted",
            "X closed out.",
            "Close out X.",
            "Confirm X for X.",
            "Yes, reset X",
            "Yes, delete X",
            "Achievements for X",
            "X: X of X earned",
            "X has earned X of X achievements.",
            "Current career: X",
            "X. No career shared yet.",
            "X: backup ",
            "X needs attention: the cloud copy ",
            "X is backed up",
            "Picked up X where you left off ",
            "X changed on another device.",
            "Cloud backup removed X,",
        ]
        .iter()
        .any(|prefix| template.filled.starts_with(prefix)),
        _ => false,
    }
}

/// How deep a line is split and its captures translated in turn.
// A drive-start line nests that deep: the line, its objective, the trip
// progress inside it, and the distance inside that.
const MAX_DEPTH: usize = 6;

pub(super) fn translate(catalog: &Catalog, line: &str, missed: &mut Vec<String>) -> String {
    let mut memo = Memo::new();
    let mut verbatim = Verbatim::new(line);
    let out = match split_position_cue(line) {
        Some((row, position, count)) => {
            let row = Pass {
                catalog,
                missed,
                memo: &mut memo,
                verbatim: &mut verbatim,
            }
            .line(row, 0)
            .unwrap_or_else(|| row.to_string());
            let cue = catalog.position_cue(position, count);
            let sep = if ends_full_width(&row) { "" } else { " " };
            format!("{row}{sep}{cue}")
        }
        None => Pass {
            catalog,
            missed,
            memo: &mut memo,
            verbatim: &mut verbatim,
        }
        .line(line, 0)
        .map(|out| {
            if starts_lowercase(line) {
                out
            } else {
                verbatim.capitalize_first_letter(&out)
            }
        })
        .unwrap_or_else(|| line.to_string()),
    };
    verbatim.restore(contract_articles(&catalog.language, out))
}

/// One translation of one line: the catalog, and the pieces nothing
/// translated.
struct Pass<'c, 'm, 's> {
    catalog: &'c Catalog,
    missed: &'m mut Vec<String>,
    memo: &'m mut Memo,
    verbatim: &'m mut Verbatim<'s>,
}

/// Keep player names out of sentence capitalization and article contraction.
/// The memoized pieces share these tokens until the whole line is rendered.
struct Verbatim<'s> {
    source: &'s str,
    marker: Option<char>,
    values: Vec<String>,
}

impl<'s> Verbatim<'s> {
    fn new(source: &'s str) -> Self {
        Self {
            source,
            marker: None,
            values: Vec::new(),
        }
    }

    fn push(&mut self, out: &mut String, value: &str) {
        use std::fmt::Write as _;
        let marker = self
            .marker
            .or_else(|| ('\u{f0000}'..='\u{ffffd}').find(|&marker| !self.source.contains(marker)));
        let Some(marker) = marker else {
            out.push_str(value);
            return;
        };
        self.marker = Some(marker);
        let _ = write!(out, "{marker}{}{marker}", self.values.len());
        self.values.push(value.to_owned());
    }

    fn restore(self, out: String) -> String {
        let Some(marker) = self.marker else {
            return out;
        };
        let mut rest = out.as_str();
        let mut restored = String::new();
        while let Some((before, after)) = rest.split_once(marker) {
            restored.push_str(before);
            let capture = after.split_once(marker).and_then(|(id, tail)| {
                let index = id.parse::<usize>().ok()?;
                Some((self.values.get(index)?, tail))
            });
            if let Some((value, tail)) = capture {
                restored.push_str(value);
                rest = tail;
            } else {
                restored.push(marker);
                rest = after;
            }
        }
        restored.push_str(rest);
        restored
    }

    fn capitalize_first_letter(&self, text: &str) -> String {
        capitalize_first_letter(text, self.marker)
    }
}

/// Every piece already worked out in one line's translation, by its text,
/// depth and whether it may split, with the misses it recorded. The
/// matcher tries the same capture under many templates and clause splits,
/// so a long line (the driving keyboard help) asked for the same pieces
/// millions of times: minutes on the game thread before this.
type Memo = HashMap<(String, usize, bool), (Option<String>, Vec<String>)>;

impl Pass<'_, '_, '_> {
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
        let key = (text.to_string(), depth, split);
        if let Some((out, missed)) = self.memo.get(&key) {
            self.missed.extend(missed.iter().cloned());
            return out.clone();
        }
        let start = self.missed.len();
        let out = self.piece_uncached(text, depth, split);
        let missed = self.missed.get(start..).unwrap_or_default().to_vec();
        self.memo.insert(key, (out.clone(), missed));
        out
    }

    fn piece_uncached(&mut self, text: &str, depth: usize, split: bool) -> Option<String> {
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
                        memo: &mut *self.memo,
                        verbatim: &mut *self.verbatim,
                    };
                    // A sentence is a line of its own; only clauses go deeper.
                    let deeper = if clauses { depth + 1 } else { depth };
                    let out = inner.line(piece, deeper);
                    // A sentence opens with a capital in the translation as
                    // in English: "451 miles remaining" is "Quedan 451 millas".
                    if clauses || starts_lowercase(piece) {
                        out
                    } else {
                        out.map(|out| inner.verbatim.capitalize_first_letter(&out))
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
        self.lead_in(core, depth).map(wrap)
    }

    /// "Job 2 of 7: 19 tons of general freight from ... in Pendleton,
    /// Oregon to ..." is a lead-in the catalog has ("Job {index} of
    /// {total}: ") before a sentence it has. The clause split breaks that
    /// sentence at the commas inside its place names, so the lead-in and
    /// the rest are each tried whole: both translate, or neither does.
    fn lead_in(&mut self, core: &str, depth: usize) -> Option<String> {
        for (at, _) in core.match_indices(": ") {
            let (head, rest) = (&core[..at + 2], &core[at + 2..]);
            if rest.trim().is_empty() || !has_words(rest) {
                continue;
            }
            let mut missed = Vec::new();
            let mut inner = Pass {
                catalog: self.catalog,
                missed: &mut missed,
                memo: &mut *self.memo,
                verbatim: &mut *self.verbatim,
            };
            let Some(head_out) = inner.piece(head, depth + 1, false) else {
                continue;
            };
            let Some(rest_out) = inner.piece(rest, depth + 1, has_sentence_break(rest)) else {
                continue;
            };
            let rest_out = if starts_lowercase(rest) {
                rest_out
            } else {
                inner.verbatim.capitalize_first_letter(&rest_out)
            };
            self.missed.extend(missed);
            let sep = if head_out.ends_with(char::is_whitespace) || ends_full_width(&head_out) {
                ""
            } else {
                " "
            };
            return Some(format!("{head_out}{sep}{rest_out}"));
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
                    if is_version_hole(template, name) {
                        out.push_str(raw);
                        continue;
                    }
                    if is_career_name_hole(template, name) {
                        let spoken = if raw == "Driver" {
                            self.catalog.exact.get(raw).map_or(raw, String::as_str)
                        } else {
                            raw
                        };
                        self.verbatim.push(&mut out, spoken);
                        continue;
                    }
                    // A quantity is said the language's way even if its
                    // hole looks like a name, such as "{take_home}".
                    if let Some(number) = plural::localize_number(&self.catalog.language, raw) {
                        out.push_str(&number);
                        continue;
                    }
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
                    } else {
                        // A capture is often a phrase the catalog knows
                        // ("heavy traffic"); an unknown one (a number, a
                        // name) is simply kept, and not a miss.
                        let mut ignored = Vec::new();
                        let translated = Pass {
                            catalog: self.catalog,
                            missed: &mut ignored,
                            memo: &mut *self.memo,
                            verbatim: &mut *self.verbatim,
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
                                Some(self.verbatim.capitalize_first_letter(&text))
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
fn capitalize_first_letter(text: &str, verbatim_marker: Option<char>) -> String {
    let Some((at, first)) = text
        .char_indices()
        .find(|&(_, c)| c.is_alphanumeric() || Some(c) == verbatim_marker)
    else {
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

pub(super) fn has_sentence_break(text: &str) -> bool {
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
