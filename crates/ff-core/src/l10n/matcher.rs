//! Templates: a catalog line with `{name}` holes, matched against a finished
//! English line to recover what filled each hole.

use std::collections::{HashMap, HashSet};

use aho_corasick::{AhoCorasick, MatchKind};

/// One piece of a source template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg {
    Lit(String),
    Hole(String),
}

/// One piece of a translation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Lit(String),
    Hole(String),
    /// `{name:one|few|many}`: a word chosen by the number in `name`.
    Plural(String, Vec<String>),
}

fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Split a source template into literal runs and holes. A brace that does
/// not open a well-formed `{name}` is literal text.
pub fn parse_source(text: &str) -> Vec<Seg> {
    let mut segs = Vec::new();
    let mut lit = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if is_name(&after[..close]) => {
                lit.push_str(&rest[..open]);
                if !lit.is_empty() {
                    segs.push(Seg::Lit(std::mem::take(&mut lit)));
                }
                segs.push(Seg::Hole(after[..close].to_string()));
                rest = &after[close + 1..];
            }
            _ => {
                lit.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    lit.push_str(rest);
    if !lit.is_empty() {
        segs.push(Seg::Lit(lit));
    }
    segs
}

/// Split a translation into literal runs, holes and plural choices.
pub fn parse_translation(text: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            break;
        };
        let inner = &after[..close];
        let (name, forms) = match inner.split_once(':') {
            Some((name, forms)) => (name, Some(forms)),
            None => (inner, None),
        };
        if !is_name(name) {
            lit.push_str(&rest[..=open]);
            rest = after;
            continue;
        }
        lit.push_str(&rest[..open]);
        if !lit.is_empty() {
            parts.push(Part::Lit(std::mem::take(&mut lit)));
        }
        parts.push(match forms {
            Some(forms) => Part::Plural(
                name.to_string(),
                forms.split('|').map(str::to_string).collect(),
            ),
            None => Part::Hole(name.to_string()),
        });
        rest = &after[close + 1..];
    }
    lit.push_str(rest);
    if !lit.is_empty() {
        parts.push(Part::Lit(lit));
    }
    parts
}

/// The hole names a source template carries, in order, once each.
pub fn hole_names(segs: &[Seg]) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for seg in segs {
        if let Seg::Hole(name) = seg {
            if !names.contains(&name.as_str()) {
                names.push(name);
            }
        }
    }
    names
}

/// Letters in a template's literal text: how much of a line it pins down.
fn literal_letters(segs: &[Seg]) -> usize {
    segs.iter()
        .map(|seg| match seg {
            Seg::Lit(text) => text.chars().filter(|c| c.is_alphabetic()).count(),
            Seg::Hole(_) => 0,
        })
        .sum()
}

/// A template with holes, ready to match.
#[derive(Clone, Debug)]
pub struct Template {
    pub segs: Vec<Seg>,
    pub translation: Vec<Part>,
    /// Letters of literal text; the most specific match wins.
    pub weight: usize,
    /// The template reads as one sentence, written with its holes filled
    /// by a placeholder word.
    pub filled: String,
}

/// Why a template cannot take part in matching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unmatchable {
    /// No literal letters at all ("{a}, {b}"): it would match anything.
    TooGeneric,
}

/// Fewer literal letters than this and a template is too generic to trust
/// with words in its holes ("{a} of {b}"); it only matches numbers ("3 of
/// 10").
pub const MIN_LITERAL_LETTERS: usize = 3;

impl Template {
    pub fn new(segs: Vec<Seg>, translation: Vec<Part>) -> Result<Self, Unmatchable> {
        let weight = literal_letters(&segs);
        if weight == 0 {
            return Err(Unmatchable::TooGeneric);
        }
        let filled = segs
            .iter()
            .map(|seg| match seg {
                Seg::Lit(text) => text.as_str(),
                Seg::Hole(_) => "X",
            })
            .collect();
        Ok(Self {
            segs,
            translation,
            weight,
            filled,
        })
    }

    /// The longest literal run: what the prefilter searches a line for.
    fn anchor(&self) -> &str {
        self.segs
            .iter()
            .filter_map(|seg| match seg {
                Seg::Lit(text) => Some(text.as_str()),
                Seg::Hole(_) => None,
            })
            .max_by_key(|text| text.chars().filter(|c| c.is_alphabetic()).count())
            .unwrap_or("")
    }

    /// Too little literal text to trust with anything but numbers.
    pub fn numbers_only(&self) -> bool {
        self.weight < MIN_LITERAL_LETTERS
    }

    /// What filled each hole, if `line` is this template filled in.
    pub fn captures<'a>(&self, line: &'a str) -> Option<HashMap<&str, &'a str>> {
        let mut out = Vec::new();
        if match_from(&self.segs, line, 0, &mut out, &mut HashSet::new()) {
            let mut map = HashMap::new();
            for (name, value) in out {
                // A name used twice must hold the same text both times.
                if let Some(previous) = map.insert(name, value) {
                    if previous != value {
                        return None;
                    }
                }
            }
            Some(map)
        } else {
            None
        }
    }
}

/// Backtracking match of `segs` against `line[pos..]`.
///
/// Whether the rest of a template matches from a position does not depend
/// on how the line got there, so `failed` remembers every (segments left,
/// position) that cannot match and the search never tries one twice. The
/// first match found is the same one; without it a template of several
/// comma-separated holes tried every way of cutting a long line at its
/// commas, seconds per line.
fn match_from<'s, 'a>(
    segs: &'s [Seg],
    line: &'a str,
    pos: usize,
    out: &mut Vec<(&'s str, &'a str)>,
    failed: &mut HashSet<(usize, usize)>,
) -> bool {
    let key = (segs.len(), pos);
    if failed.contains(&key) {
        return false;
    }
    let matched = match_here(segs, line, pos, out, failed);
    if !matched {
        failed.insert(key);
    }
    matched
}

fn match_here<'s, 'a>(
    segs: &'s [Seg],
    line: &'a str,
    pos: usize,
    out: &mut Vec<(&'s str, &'a str)>,
    failed: &mut HashSet<(usize, usize)>,
) -> bool {
    let Some((first, rest)) = segs.split_first() else {
        return pos == line.len();
    };
    match first {
        Seg::Lit(text) => {
            line[pos..].starts_with(text.as_str())
                && match_from(rest, line, pos + text.len(), out, failed)
        }
        Seg::Hole(name) => {
            let Some((next, after)) = rest.split_first() else {
                // The last hole takes the rest of the line.
                out.push((name, &line[pos..]));
                return true;
            };
            let next_text = match next {
                Seg::Lit(text) => text.as_str(),
                // Two holes side by side. The second is usually a tail that
                // opens with a comma (", then 74 miles on it"), so try each
                // ", " from the right first; failing that this one takes
                // nothing and the next decides (a leading "{prefix}" is
                // usually empty).
                Seg::Hole(_) => {
                    let splits = line[pos..]
                        .rmatch_indices(", ")
                        .map(|(at, _)| pos + at)
                        .chain(std::iter::once(pos));
                    for at in splits {
                        let depth = out.len();
                        out.push((name, &line[pos..at]));
                        if match_from(rest, line, at, out, failed) {
                            return true;
                        }
                        out.truncate(depth);
                    }
                    return false;
                }
            };
            // Only a space between this hole and the next ("How to play.
            // {controls} {page_title}"): a sentence ends at the split, so try
            // the spaces after a full stop first, farthest first, before
            // every space nearest first.
            if next_text.trim().is_empty() {
                let ends: Vec<usize> = line[pos..]
                    .match_indices(next_text)
                    .map(|(at, _)| pos + at)
                    .filter(|&at| at > pos && line[..at].ends_with(['.', '!', '?']))
                    .collect();
                for &at in ends.iter().rev() {
                    let depth = out.len();
                    out.push((name, &line[pos..at]));
                    if match_from(after, line, at + next_text.len(), out, failed) {
                        return true;
                    }
                    out.truncate(depth);
                }
            }
            // Try each place the next literal appears, nearest first. A hole
            // may be empty: optional pieces ("{rest}") often are.
            let mut from = pos;
            while from <= line.len() {
                let Some(found) = line[from..].find(next_text) else {
                    return false;
                };
                let at = from + found;
                let depth = out.len();
                out.push((name, &line[pos..at]));
                if match_from(after, line, at + next_text.len(), out, failed) {
                    return true;
                }
                out.truncate(depth);
                from = at + line[at..].chars().next().map_or(1, char::len_utf8);
            }
            false
        }
    }
}

/// Every template, behind a prefilter that finds the few whose longest
/// literal run occurs in a line.
pub struct TemplateSet {
    templates: Vec<Template>,
    finder: Option<AhoCorasick>,
    by_anchor: Vec<Vec<usize>>,
}

impl TemplateSet {
    pub fn new(templates: Vec<Template>) -> Self {
        let mut anchors: Vec<String> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut by_anchor: Vec<Vec<usize>> = Vec::new();
        for (i, template) in templates.iter().enumerate() {
            let anchor = template.anchor().to_string();
            let slot = *index.entry(anchor.clone()).or_insert_with(|| {
                anchors.push(anchor);
                by_anchor.push(Vec::new());
                by_anchor.len() - 1
            });
            by_anchor[slot].push(i);
        }
        let finder = AhoCorasick::builder()
            .match_kind(MatchKind::Standard)
            .build(&anchors)
            .map_err(|err| log::error!("translation templates could not be indexed: {err}"))
            .ok();
        Self {
            templates,
            finder,
            by_anchor,
        }
    }

    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    /// The most specific template `line` fills in, with its captures.
    /// `accept` can still turn a match down.
    pub fn best_match<'a>(
        &self,
        line: &'a str,
        accept: impl Fn(&Template, &HashMap<&str, &'a str>) -> bool,
    ) -> Option<(&Template, HashMap<&str, &'a str>)> {
        let finder = self.finder.as_ref()?;
        let mut candidates: Vec<usize> = Vec::new();
        for found in finder.find_overlapping_iter(line) {
            candidates.extend(&self.by_anchor[found.pattern().as_usize()]);
        }
        candidates.sort_unstable();
        candidates.dedup();
        candidates.sort_by_key(|&i| std::cmp::Reverse(self.templates[i].weight));
        candidates.into_iter().find_map(|i| {
            let template = &self.templates[i];
            template
                .captures(line)
                .filter(|caps| accept(template, caps))
                .map(|caps| (template, caps))
        })
    }
}
