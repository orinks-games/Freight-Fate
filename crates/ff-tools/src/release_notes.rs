//! `release-notes`: curated changelog utilities for Freight Fate releases.
//!
//! `stable` writes the matching version block from `CHANGELOG.md`.
//! `nightly` writes new `Unreleased` entries since the previous snapshot.
//! `should-build-nightly` decides scheduled snapshots from curated entries or
//! explicit nightly markers, never from raw commit subjects. `check` is the
//! CI changelog gate, and `check-size` keeps notes under GitHub's limit.

use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

use crate::cli::{Args, Spec, UsageError};
use crate::util::{self, py_splitlines, py_strip, Result};

const CHANGELOG_PATH: &str = "CHANGELOG.md";
/// A snapshot's notes open with this many changes, one line each, before
/// the full entries (owner, 2026-10-25: a short summary, then the changelog,
/// in place of the old "back up your saves" preview blurb).
const SUMMARY_LIMIT: usize = 12;
const GITHUB_RELEASE_NOTES_SAFE_CHARACTERS: usize = 120_000;
const COMPLETE_LIST: &str = "## Complete change list\n\n\
There are more changes than fit on this page. The full list is \
CHANGELOG.md in your game folder.";
// A later snapshot can overflow too: a busy stretch, or a rewrite of the
// curated entries, which makes every bullet read as new to the previous tag.
const FIRST_SNAPSHOT_COMPLETE_LIST: &str = COMPLETE_LIST;
const SNAPSHOT_COMPLETE_LIST: &str = COMPLETE_LIST;
const STABLE_COMPLETE_LIST: &str = COMPLETE_LIST;
/// A stable release can open with a hand-written summary of everything since
/// the last stable: these headings, under the version's own `## X.Y.Z - date`
/// block, with the full per-snapshot list kept in `## X.Y.Z complete change
/// list` below it. Nightlies never read them (they are not in
/// [`PLAYER_FACING_SECTIONS`]), so the summary cannot resurface as new entries.
const SUMMARY_SECTIONS: &[&str] = &["Highlights", "New features", "Fixes", "Changes"];
const STABLE_FULL_CHANGELOG: &str = "## Complete change list\n\n\
Every change in this release is listed in the \
[full Freight Fate {version} changelog]\
(https://github.com/orinks-games/Freight-Fate/blob/v{version}/CHANGELOG.md), \
on GitHub and as CHANGELOG.md in your game folder.";
const SECTION_ORDER: &[&str] = &[
    "Added",
    "Changed",
    "Improved",
    "Fixed",
    "Removed",
    "Deprecated",
    "Security",
];
const INTERNAL_SECTIONS: &[&str] = &[
    "Build",
    "CI",
    "Developer",
    "Development",
    "Docs",
    "Documentation",
    "Internal",
    "Notes",
    "Tests",
    "Tooling",
];
const NIGHTLY_BUILD_MARKERS: &[&str] = &["nightly: build", "[nightly build]"];
const SKIP_CHANGELOG_MARKERS: &[&str] = &["changelog: none", "[skip changelog]"];
/// `crates/` was added 2026-09-20: the Rust port moved every line of gameplay
/// there. `data/` and `assets/` are the world data and the shipped sounds.
const USER_FACING_PATH_PREFIXES: &[&str] = &["data/", "assets/", "docs/", "crates/"];
const USER_FACING_PATHS: &[&str] = &["CHANGELOG.md", "README.md"];
/// The tools crate is not shipped, except what builds and describes a
/// release (`release.rs`, `release/`, `release_notes.rs`).
const TOOLS_CRATE: &str = "crates/ff-tools/";
const RELEASE_TOOLING: &str = "crates/ff-tools/src/release";
const REPUBLISHED_FILE: &str = "tools/release_notes_republished.txt";

fn player_facing(title: &str) -> bool {
    SECTION_ORDER.contains(&title) || title == "Compatibility"
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

/// A crate's test or bench binaries, the map crawl's scripts, and the tools
/// crate (bar the release tooling).
fn not_user_facing(path: &str) -> bool {
    static NOT: OnceLock<Regex> = OnceLock::new();
    re(&NOT, r"^(?:crates/[^/]+/(?:tests|benches)/|data/spider/)").is_match(path)
        || (path.starts_with(TOOLS_CRATE) && !path.starts_with(RELEASE_TOOLING))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub title: String,
    pub entries: Vec<String>,
}

struct ReleaseBlock {
    heading: String,
    body: String,
}

fn root() -> std::path::PathBuf {
    util::repo_root()
}

fn git_lines(args: &[&str]) -> Result<Vec<String>> {
    Ok(util::git(&root(), args)?
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

fn changelog_at(git_ref: &str) -> String {
    util::git(&root(), &["show", &format!("{git_ref}:{CHANGELOG_PATH}")]).unwrap_or_default()
}

fn changelog_text() -> Result<String> {
    util::read_text(&root().join(CHANGELOG_PATH))
}

fn extract_release_block(text: &str, heading_pattern: &str) -> String {
    let heading = Regex::new(&format!("(?im){heading_pattern}")).expect("valid regex");
    let Some(m) = heading.find(text) else {
        return String::new();
    };
    let start = m.end();
    static NEXT: OnceLock<Regex> = OnceLock::new();
    let end = re(&NEXT, r"(?m)^##\s+")
        .find(&text[start..])
        .map_or(text.len(), |n| start + n.start());
    py_strip(&text[start..end]).to_string()
}

fn unreleased_block(text: &str) -> String {
    extract_release_block(text, r"^##\s+\[?Unreleased\]?\s*$")
}

fn version_block(text: &str, version: &str) -> String {
    let normalized = version.strip_prefix('v').unwrap_or(version);
    extract_release_block(
        text,
        &format!(
            r"^##\s+\[?v?{}\]?(?:\s+-\s+\d{{4}}-\d{{2}}-\d{{2}})?\s*$",
            regex::escape(normalized)
        ),
    )
}

pub fn parse_sections(markdown: &str, min_heading_level: usize) -> Vec<Section> {
    static HEADING: OnceLock<Regex> = OnceLock::new();
    static BULLET: OnceLock<Regex> = OnceLock::new();
    let heading_re = re(&HEADING, r"^(#{2,6})\s+(.+?)\s*$");
    let bullet_re = re(&BULLET, r"^[-*+]\s+");
    let mut sections = Vec::new();
    let mut title = String::new();
    let mut entries: Vec<String> = Vec::new();
    let mut entry: Vec<String> = Vec::new();

    fn flush_entry(entry: &mut Vec<String>, entries: &mut Vec<String>) {
        if !entry.is_empty() {
            entries.push(
                entry
                    .join("\n")
                    .trim_end_matches(util::py_isspace)
                    .to_string(),
            );
            entry.clear();
        }
    }

    for line in py_splitlines(markdown) {
        if let Some(h) = heading_re.captures(line) {
            if h[1].len() < min_heading_level {
                continue;
            }
            flush_entry(&mut entry, &mut entries);
            if !title.is_empty() && !entries.is_empty() {
                sections.push(Section {
                    title: title.clone(),
                    entries: std::mem::take(&mut entries),
                });
            }
            entries.clear();
            title = h[2].to_string();
            continue;
        }
        if bullet_re.is_match(line) {
            flush_entry(&mut entry, &mut entries);
            entry.push(line.to_string());
            continue;
        }
        if !entry.is_empty() && (line.starts_with([' ', '\t']) || py_strip(line).is_empty()) {
            entry.push(line.to_string());
        }
    }
    flush_entry(&mut entry, &mut entries);
    if !title.is_empty() && !entries.is_empty() {
        sections.push(Section { title, entries });
    }
    sections
}

fn release_blocks(text: &str) -> Vec<ReleaseBlock> {
    static HEADING: OnceLock<Regex> = OnceLock::new();
    let heading = re(&HEADING, r"(?m)^##\s+(.+?)\s*$");
    let matches: Vec<_> = heading.captures_iter(text).collect();
    let mut blocks = Vec::new();
    for (index, caps) in matches.iter().enumerate() {
        let whole = caps.get(0).expect("match");
        let start = whole.end();
        let end = matches
            .get(index + 1)
            .map_or(text.len(), |next| next.get(0).expect("match").start());
        blocks.push(ReleaseBlock {
            heading: py_strip(&caps[1]).to_string(),
            body: py_strip(&text[start..end]).to_string(),
        });
    }
    blocks
}

fn eligible_sections(markdown: &str) -> Vec<Section> {
    parse_sections(markdown, 3)
        .into_iter()
        .filter(|s| player_facing(&s.title) && !INTERNAL_SECTIONS.contains(&s.title.as_str()))
        .collect()
}

/// Versions that already have a published stable tag (`vX.Y.Z`): a version
/// block is only "staged" until its tag exists, and once released it must
/// not resurface in developer snapshots.
fn released_versions() -> HashSet<String> {
    git_lines(&["tag", "--list", "v*.*.*"])
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.strip_prefix('v').unwrap_or(&t).to_string())
        .collect()
}

/// Player-facing entries that can feed developer snapshots: `Unreleased`
/// plus any version block whose stable tag does not exist yet.
fn nightly_candidate_sections(text: &str, released: &HashSet<String>) -> Vec<Section> {
    static VERSION: OnceLock<Regex> = OnceLock::new();
    let version = re(&VERSION, r"^v?(\d+\.\d+\.\d+)");
    let mut sections = Vec::new();
    for block in release_blocks(text) {
        let heading = block.heading.to_lowercase();
        let heading = heading.trim_start_matches('[');
        if heading.starts_with("unreleased") {
            sections.extend(eligible_sections(&block.body));
            continue;
        }
        if let Some(m) = version.captures(heading) {
            if released.contains(&m[1]) {
                continue; // already shipped under a stable tag
            }
            sections.extend(eligible_sections(&block.body));
        }
    }
    sections
}

pub fn normalize_entry(entry: &str) -> String {
    static LINK: OnceLock<Regex> = OnceLock::new();
    static CODE: OnceLock<Regex> = OnceLock::new();
    static EMPH: OnceLock<Regex> = OnceLock::new();
    static BULLET: OnceLock<Regex> = OnceLock::new();
    static DASH: OnceLock<Regex> = OnceLock::new();
    static SPACE: OnceLock<Regex> = OnceLock::new();
    let entry = re(&LINK, r"\[([^\]]+)\]\([^)]+\)").replace_all(entry, "$1");
    let entry = re(&CODE, r"`([^`]+)`").replace_all(&entry, "$1");
    let entry = re(&EMPH, r"(\*\*|__|\*|_)").replace_all(&entry, "");
    let entry = re(&BULLET, r"^[-*+]\s+").replace(py_strip(&entry), "");
    let entry = re(&DASH, r"\s+[-\x{2013}\x{2014}]\s+").replace_all(&entry, " - ");
    let entry = re(&SPACE, r"\s+").replace_all(&entry, " ");
    py_strip(&entry.to_lowercase()).to_string()
}

fn format_entry(entry: &str) -> String {
    static MARKER: OnceLock<Regex> = OnceLock::new();
    let lines: Vec<&str> = py_splitlines(entry)
        .into_iter()
        .map(py_strip)
        .filter(|l| !l.is_empty())
        .collect();
    let Some(first) = lines.first() else {
        return String::new();
    };
    let (marker, first_text) = match re(&MARKER, r"^([-*+]\s+)(.*)$").captures(first) {
        Some(c) => (c[1].to_string(), c[2].to_string()),
        None => ("- ".to_string(), first.to_string()),
    };
    let mut parts = vec![first_text];
    parts.extend(lines[1..].iter().map(|s| s.to_string()));
    format!("{marker}{}", parts.join(" "))
}

fn format_sections(sections: &[Section], heading_level: usize, order: &[&str]) -> String {
    if sections.is_empty() {
        return "- No user-facing changes".into();
    }
    let mut by_title: Vec<(String, Vec<String>)> = Vec::new();
    for section in sections {
        match by_title.iter_mut().find(|(t, _)| *t == section.title) {
            Some((_, entries)) => entries.extend(section.entries.iter().cloned()),
            None => by_title.push((section.title.clone(), section.entries.clone())),
        }
    }
    let mut titles: Vec<String> = order
        .iter()
        .filter(|t| by_title.iter().any(|(bt, _)| bt == *t))
        .map(|t| t.to_string())
        .collect();
    for (t, _) in &by_title {
        if !titles.contains(t) {
            titles.push(t.clone());
        }
    }
    let mut chunks = Vec::new();
    for title in titles {
        let entries = &by_title.iter().find(|(t, _)| *t == title).expect("title").1;
        let mut seen: Vec<String> = Vec::new();
        for e in entries {
            let formatted = format_entry(e);
            if !seen.contains(&formatted) {
                seen.push(formatted);
            }
        }
        let body: Vec<String> = seen.into_iter().filter(|e| !e.is_empty()).collect();
        chunks.push(format!(
            "{} {title}\n{}",
            "#".repeat(heading_level),
            body.join("\n")
        ));
    }
    py_strip(&chunks.join("\n\n")).to_string()
}

fn entries_from_sections(sections: &[Section]) -> HashSet<String> {
    sections
        .iter()
        .flat_map(|s| s.entries.iter().map(|e| normalize_entry(e)))
        .collect()
}

fn excluded_entries_from_notes(path: &str) -> HashSet<String> {
    if path.is_empty() {
        return HashSet::new();
    }
    match std::fs::read_to_string(path) {
        Ok(text) => entries_from_sections(&parse_sections(&text, 2)),
        Err(_) => HashSet::new(),
    }
}

/// Entries a rewrite republished rather than added. A snapshot lists the
/// bullets whose text is not in the changelog at the previous tag, so a
/// commit that rewords every entry makes the next snapshot announce the
/// whole block as new. The republished file names that commit
/// (`ref = <sha>`): everything in the changelog there counts as already
/// published, except the bullets whose bold lead it lists on `new = ...`
/// lines.
fn republished_entries(released: &HashSet<String>) -> Result<HashSet<String>> {
    let path = root().join(REPUBLISHED_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(HashSet::new());
    };
    let mut git_ref = String::new();
    let mut still_new = Vec::new();
    for raw in py_splitlines(&text) {
        let line = py_strip(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap_or((line, ""));
        let (key, value) = (py_strip(key), py_strip(value));
        if key == "ref" {
            git_ref = value.to_string();
        } else if key == "new" {
            still_new.push(normalize_entry(value));
        }
    }
    if git_ref.is_empty() {
        return Ok(HashSet::new());
    }
    let text = changelog_at(&git_ref);
    if text.is_empty() {
        return Err(format!(
            "{REPUBLISHED_FILE}: commit {git_ref} is not in this clone"
        ));
    }
    Ok(
        entries_from_sections(&nightly_candidate_sections(&text, released))
            .into_iter()
            .filter(|e| !still_new.iter().any(|lead| e.starts_with(lead.as_str())))
            .collect(),
    )
}

fn sections_added_since(
    base_ref: &str,
    head_text: &str,
    extra_excluded: &HashSet<String>,
    released: &HashSet<String>,
) -> Result<Vec<Section>> {
    let mut base = entries_from_sections(&nightly_candidate_sections(
        &changelog_at(base_ref),
        released,
    ));
    base.extend(republished_entries(released)?);
    base.extend(extra_excluded.iter().cloned());
    Ok(nightly_candidate_sections(head_text, released)
        .into_iter()
        .filter_map(|s| {
            let entries: Vec<String> = s
                .entries
                .into_iter()
                .filter(|e| !base.contains(&normalize_entry(e)))
                .collect();
            (!entries.is_empty()).then_some(Section {
                title: s.title,
                entries,
            })
        })
        .collect())
}

fn stable_order() -> Vec<&'static str> {
    let mut order = vec!["Compatibility"];
    order.extend(SUMMARY_SECTIONS);
    order.extend(SECTION_ORDER);
    order
}

fn format_stable_notes(sections: &[Section], footer: &str) -> String {
    // A stable release is read by players upgrading in place, so what decides
    // whether their install or career comes across leads the page.
    let body = format_sections(sections, 2, &stable_order());
    if footer.is_empty() {
        body
    } else {
        format!("{body}\n\n{footer}")
    }
}

fn stable_notes(version: &str) -> Result<String> {
    let text = changelog_text()?;
    let mut block = version_block(&text, version);
    if block.is_empty() {
        block = unreleased_block(&text);
    }
    let sections = parse_sections(&block, 3);
    if sections
        .iter()
        .any(|s| SUMMARY_SECTIONS.contains(&s.title.as_str()))
    {
        // The curated summary leads, then the whole change list under it
        // (owner, 2026-10-25), bounded like any list; the link to the full
        // changelog closes the page either way.
        let footer = STABLE_FULL_CHANGELOG
            .replace("{version}", version.strip_prefix('v').unwrap_or(version));
        let (kept, _) = bounded_sections(&sections, &footer, &|chosen, tail| {
            format_stable_notes(chosen, tail)
        });
        return Ok(format_stable_notes(&kept, &footer));
    }
    let (sections, bounded) = bounded_sections(&sections, STABLE_COMPLETE_LIST, &|chosen, tail| {
        format_stable_notes(chosen, tail)
    });
    Ok(format_stable_notes(
        &sections,
        if bounded { STABLE_COMPLETE_LIST } else { "" },
    ))
}

/// An entry's one-line summary: its bold lead sentence, or else its first
/// sentence, as plain text.
fn lead_of(entry: &str) -> String {
    static BOLD: OnceLock<Regex> = OnceLock::new();
    let bold = re(&BOLD, r"^[-*+]\s+\*\*(.+?)\*\*");
    let lead = match bold.captures(py_strip(entry)) {
        Some(c) => c[1].to_string(),
        None => {
            let plain = format_entry(entry);
            let plain = plain.trim_start_matches(['-', '*', '+', ' ']);
            match plain.find(". ") {
                Some(end) => plain[..=end].to_string(),
                None => plain.to_string(),
            }
        }
    };
    let mut lead = py_strip(lead.trim_end_matches(',')).to_string();
    // A summary line, not the entry again: an entry with no lead sentence
    // keeps its first 20 words.
    let words: Vec<&str> = lead.split_whitespace().collect();
    if words.len() > 20 {
        lead = format!("{}...", words[..20].join(" "));
    }
    if lead.chars().count() > 200 {
        lead = format!("{}...", lead.chars().take(200).collect::<String>());
    }
    if lead.ends_with(['.', '!', '?']) {
        lead
    } else {
        format!("{lead}.")
    }
}

/// "## Summary": each change's lead, in the order the notes list them, at
/// most [`SUMMARY_LIMIT`] of them.
fn summary_block(sections: &[Section], order: &[&str]) -> String {
    let mut leads: Vec<String> = Vec::new();
    let mut titles: Vec<&str> = order.to_vec();
    for s in sections {
        if !titles.contains(&s.title.as_str()) {
            titles.push(&s.title);
        }
    }
    for title in titles {
        for s in sections.iter().filter(|s| s.title == title) {
            for e in &s.entries {
                let lead = lead_of(e);
                if !leads.contains(&lead) {
                    leads.push(lead);
                }
            }
        }
    }
    if leads.is_empty() {
        return String::new();
    }
    let more = leads.len().saturating_sub(SUMMARY_LIMIT);
    let mut lines: Vec<String> = leads
        .iter()
        .take(SUMMARY_LIMIT)
        .map(|l| format!("- {l}"))
        .collect();
    if more > 0 {
        lines.push(format!(
            "- And {more} more {}, listed below.",
            if more == 1 { "change" } else { "changes" }
        ));
    }
    format!("## Summary\n\n{}", lines.join("\n"))
}

fn format_nightly_notes(
    sections: &[Section],
    changes_heading: &str,
    footer: &str,
    section_heading_level: usize,
) -> String {
    let body = format_sections(sections, section_heading_level, SECTION_ORDER);
    let summary = summary_block(sections, SECTION_ORDER);
    let notes = if summary.is_empty() {
        format!("## {changes_heading}\n\n{body}")
    } else {
        format!("{summary}\n\n## {changes_heading}\n\n{body}")
    };
    if footer.is_empty() {
        notes
    } else {
        format!("{notes}\n\n{footer}")
    }
}

fn fits(notes: &str) -> bool {
    notes.chars().count() < GITHUB_RELEASE_NOTES_SAFE_CHARACTERS
}

/// Keep complete recent entries from every section within GitHub's limit.
///
/// Returns the sections to publish and whether anything was left out; when
/// something was, the caller appends `footer` so the page says where the
/// rest is. Entries are taken in file order, round-robin across sections, so
/// the newest of every kind survives rather than all of one section.
fn bounded_sections(
    sections: &[Section],
    footer: &str,
    render: &dyn Fn(&[Section], &str) -> String,
) -> (Vec<Section>, bool) {
    if fits(&render(sections, "")) {
        return (sections.to_vec(), false);
    }
    let mut selected: Vec<Vec<String>> = vec![Vec::new(); sections.len()];
    let mut offsets = vec![0usize; sections.len()];
    let chosen = |selected: &[Vec<String>]| -> Vec<Section> {
        sections
            .iter()
            .zip(selected)
            .filter(|(_, picked)| !picked.is_empty())
            .map(|(s, picked)| Section {
                title: s.title.clone(),
                entries: picked.clone(),
            })
            .collect()
    };
    loop {
        let mut added = false;
        for (index, section) in sections.iter().enumerate() {
            if offsets[index] >= section.entries.len() {
                continue;
            }
            selected[index].push(section.entries[offsets[index]].clone());
            if fits(&render(&chosen(&selected), footer)) {
                offsets[index] += 1;
                added = true;
            } else {
                selected[index].pop();
            }
        }
        if !added {
            break;
        }
    }
    (chosen(&selected), true)
}

fn nightly_notes(
    previous_tag: &str,
    exclude_notes: &str,
    exclude_stable_notes: &str,
    first_snapshot: bool,
) -> Result<String> {
    let text = changelog_text()?;
    let mut excluded = excluded_entries_from_notes(exclude_notes);
    excluded.extend(excluded_entries_from_notes(exclude_stable_notes));
    let released = released_versions();
    let sections = if previous_tag.is_empty() {
        nightly_candidate_sections(&text, &released)
    } else {
        sections_added_since(previous_tag, &text, &excluded, &released)?
    };
    let heading = if first_snapshot {
        "Changes in this snapshot"
    } else {
        "Changes since the previous snapshot"
    };
    let level = if first_snapshot { 3 } else { 2 };
    let complete = if first_snapshot {
        FIRST_SNAPSHOT_COMPLETE_LIST
    } else {
        SNAPSHOT_COMPLETE_LIST
    };
    let (sections, bounded) = bounded_sections(&sections, complete, &|chosen, tail| {
        format_nightly_notes(chosen, heading, tail, level)
    });
    Ok(format_nightly_notes(
        &sections,
        heading,
        if bounded { complete } else { "" },
        level,
    ))
}

fn resolve_base(base: &str) -> Result<String> {
    if base != "auto" {
        return Ok(base.to_string());
    }
    if util::git(&root(), &["branch", "--show-current"])? == "main" {
        return Ok("origin/main".into());
    }
    Ok(nearest_release_line())
}

/// Which release line the current branch was cut from, measured as the
/// nearest branch point: a hotfix is cut from main and never contains dev,
/// and main is an ancestor of dev as well, so ancestry alone cannot tell.
fn nearest_release_line() -> String {
    let mut candidates: Vec<(u64, &str)> = Vec::new();
    for git_ref in ["origin/dev", "origin/main"] {
        let Ok(counts) = util::git(
            &root(),
            &[
                "rev-list",
                "--count",
                "--left-right",
                &format!("{git_ref}...HEAD"),
            ],
        ) else {
            continue; // a clone without that remote branch
        };
        // Both sides of the divergence: a hotfix is zero commits behind main
        // and a long way behind dev, which is the whole signal.
        let total = counts
            .split_whitespace()
            .filter_map(|n| n.parse::<u64>().ok())
            .sum();
        candidates.push((total, git_ref));
    }
    // Ties favour dev, which sorts first and is where ordinary work belongs.
    candidates
        .into_iter()
        .min()
        .map_or("origin/dev".into(), |(_, r)| r.to_string())
}

fn ref_is_ancestor(ancestor: &str, descendant: &str) -> bool {
    util::git_ok(
        &root(),
        &["merge-base", "--is-ancestor", ancestor, descendant],
    )
}

fn commit_messages(base: &str, head: &str, fresh_only: bool) -> Result<Vec<String>> {
    let commits = git_lines(&[
        "log",
        "--no-merges",
        "--format=%H",
        &format!("{base}..{head}"),
    ])?;
    let mut out = Vec::new();
    for commit in commits {
        // A dev push that merges release history back from main carries
        // commits that already passed this gate when they landed on main.
        if fresh_only && ref_is_ancestor(&commit, "origin/main") {
            continue;
        }
        out.push(util::git(&root(), &["show", "-s", "--format=%B", &commit])?);
    }
    Ok(out)
}

fn has_marker(message: &str, markers: &[&str]) -> bool {
    let folded = message.to_lowercase();
    markers.iter().any(|m| folded.contains(m))
}

fn commits_request_nightly_build(base: &str, head: &str) -> Result<bool> {
    Ok(commit_messages(base, head, false)?
        .iter()
        .any(|m| has_marker(m, NIGHTLY_BUILD_MARKERS)))
}

fn commits_opt_out_of_changelog(base: &str, head: &str) -> Result<bool> {
    let messages = commit_messages(base, head, true)?;
    if messages.is_empty() {
        // Every commit in the range is already on origin/main: this push
        // only syncs release history, which was gated when it landed there.
        return Ok(true);
    }
    Ok(messages
        .iter()
        .all(|m| has_marker(m, SKIP_CHANGELOG_MARKERS)))
}

pub fn is_user_facing_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    if not_user_facing(&normalized) {
        return false;
    }
    USER_FACING_PATHS.contains(&normalized.as_str())
        || USER_FACING_PATH_PREFIXES
            .iter()
            .any(|p| normalized.starts_with(p))
}

fn all_player_facing_sections(text: &str) -> Vec<Section> {
    release_blocks(text)
        .iter()
        .flat_map(|b| eligible_sections(&b.body))
        .collect()
}

/// Player-facing bullets present at `head` but not at `base`, wherever they
/// land in the changelog: a single-push release sync moves curated notes
/// from `Unreleased` straight under a freshly tagged version heading.
fn changelog_added_entries(base: &str, head: &str) -> Result<Vec<String>> {
    let base_entries = entries_from_sections(&all_player_facing_sections(&changelog_at(base)));
    let head_text = if head == "HEAD" {
        changelog_text()?
    } else {
        changelog_at(head)
    };
    Ok(all_player_facing_sections(&head_text)
        .into_iter()
        .flat_map(|s| s.entries)
        .filter(|e| !base_entries.contains(&normalize_entry(e)))
        .collect())
}

fn check(base: &str, head: &str) -> Result<i32> {
    let base = resolve_base(base)?;
    let files = git_lines(&["diff", "--name-only", &format!("{base}..{head}")])?;
    let user_facing: Vec<&String> = files.iter().filter(|p| is_user_facing_path(p)).collect();
    if user_facing.is_empty() {
        println!("No user-facing paths changed.");
        return Ok(0);
    }
    if commits_opt_out_of_changelog(&base, head)? {
        println!("All commits opt out of the changelog gate via a skip marker.");
        return Ok(0);
    }
    if !files.iter().any(|f| f == CHANGELOG_PATH) {
        eprintln!("User-facing paths changed without updating CHANGELOG.md:");
        for path in user_facing {
            eprintln!("- {path}");
        }
        return Ok(1);
    }
    if changelog_added_entries(&base, head)?.is_empty() {
        eprintln!("CHANGELOG.md changed, but no new player-facing bullet was added.");
        return Ok(1);
    }
    println!("Found new CHANGELOG.md entries for user-facing changes.");
    Ok(0)
}

fn should_build_nightly(args: &Args) -> Result<i32> {
    let previous = args.string("previous-tag", "");
    let head = args.string("head", "HEAD");
    if previous.is_empty() {
        println!("should_build=true");
        eprintln!("No previous nightly tag found; building once.");
        return Ok(0);
    }
    let latest_stable = args.string("latest-stable-tag", "");
    if !latest_stable.is_empty() && ref_is_ancestor(&head, &latest_stable) {
        println!("should_build=false");
        eprintln!("Latest stable release already contains this commit.");
        return Ok(0);
    }
    let mut baseline = previous.clone();
    if !latest_stable.is_empty() && ref_is_ancestor(&previous, &latest_stable) {
        baseline = latest_stable;
    }
    if commits_request_nightly_build(&baseline, &head)? {
        println!("should_build=true");
        eprintln!("Nightly build requested by commit marker.");
        return Ok(0);
    }
    let mut excluded = excluded_entries_from_notes(&args.string("exclude-notes", ""));
    excluded.extend(excluded_entries_from_notes(
        &args.string("exclude-stable-notes", ""),
    ));
    let sections = sections_added_since(
        &baseline,
        &changelog_text()?,
        &excluded,
        &released_versions(),
    )?;
    if sections.is_empty() {
        println!("should_build=false");
        eprintln!("No new curated changelog entries or nightly build marker found.");
    } else {
        println!("should_build=true");
        eprintln!("New curated changelog entries found for nightly build.");
    }
    Ok(0)
}

fn check_size(input: &Path, max: usize) -> Result<i32> {
    let bytes =
        std::fs::read(input).map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    let text = String::from_utf8(bytes).map_err(|e| format!("{}: {e}", input.display()))?;
    let characters = text.chars().count();
    if characters > max {
        eprintln!(
            "Release notes contain {characters} characters, exceeding the \
             {max}-character publication limit."
        );
        return Ok(1);
    }
    println!("Release notes fit the publication limit ({characters} characters).");
    Ok(0)
}

const USAGE: &str = "usage: release-notes <stable|nightly|should-build-nightly|check|check-size> [options]
  stable --version X.Y.Z --output FILE
  nightly [--previous-tag T] [--exclude-notes F] [--exclude-stable-notes F] [--first-snapshot] --output FILE
  should-build-nightly [--previous-tag T] [--latest-stable-tag T] [--exclude-notes F] [--exclude-stable-notes F] [--head REF]
  check --base REF|auto [--head REF]
  check-size --input FILE --max-characters N";

pub fn main(argv: &[String]) -> Result<i32> {
    let Some((command, rest)) = argv.split_first() else {
        return Err(USAGE.into());
    };
    let usage = |e: UsageError| format!("{e}\n{USAGE}");
    match command.as_str() {
        "stable" | "nightly" => {
            let args = Args::parse(
                rest,
                &Spec {
                    values: &[
                        "version",
                        "previous-tag",
                        "exclude-notes",
                        "exclude-stable-notes",
                        "output",
                    ],
                    flags: if command == "nightly" {
                        &["first-snapshot"]
                    } else {
                        &[]
                    },
                },
            )
            .map_err(usage)?;
            let output = args.required("output").map_err(usage)?;
            let notes = if command == "stable" {
                let version = args.string("version", "");
                if version.is_empty() {
                    return Err("stable notes need --version".into());
                }
                stable_notes(&version)?
            } else {
                nightly_notes(
                    &args.string("previous-tag", ""),
                    &args.string("exclude-notes", ""),
                    &args.string("exclude-stable-notes", ""),
                    args.flag("first-snapshot"),
                )?
            };
            util::write_text(Path::new(&output), &format!("{notes}\n"))?;
            println!("Wrote release notes to {output}.");
            Ok(0)
        }
        "should-build-nightly" => {
            let args = Args::parse(
                rest,
                &Spec {
                    values: &[
                        "previous-tag",
                        "exclude-notes",
                        "latest-stable-tag",
                        "exclude-stable-notes",
                        "head",
                    ],
                    flags: &[],
                },
            )
            .map_err(usage)?;
            should_build_nightly(&args)
        }
        "check" => {
            let args = Args::parse(
                rest,
                &Spec {
                    values: &["base", "head"],
                    flags: &[],
                },
            )
            .map_err(usage)?;
            check(
                &args.required("base").map_err(usage)?,
                &args.string("head", "HEAD"),
            )
        }
        "check-size" => {
            let args = Args::parse(
                rest,
                &Spec {
                    values: &["input", "max-characters"],
                    flags: &[],
                },
            )
            .map_err(usage)?;
            let input = args.required("input").map_err(usage)?;
            let max = args.number("max-characters", 0usize).map_err(usage)?;
            if args.value("max-characters").is_none() {
                return Err(format!("--max-characters is required\n{USAGE}"));
            }
            check_size(Path::new(&input), max)
        }
        other => Err(format!("unknown release-notes command {other:?}\n{USAGE}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANGELOG: &str = "# Changelog\n\n## Unreleased\n\n### Added\n\n\
- **Night radio.** A new station after dark.\n  It plays jazz.\n- Second entry\n\n\
### Tests\n\n- internal thing\n\n## 1.9.2 - 2026-10-01\n\n### Fixed\n\n- Old fix\n";

    #[test]
    fn unreleased_and_version_blocks() {
        assert!(unreleased_block(CHANGELOG).starts_with("### Added"));
        assert_eq!(version_block(CHANGELOG, "v1.9.2"), "### Fixed\n\n- Old fix");
        assert_eq!(version_block(CHANGELOG, "1.9.3"), "");
    }

    #[test]
    fn sections_skip_internal_buckets_and_released_versions() {
        let released: HashSet<String> = ["1.9.2".to_string()].into_iter().collect();
        let sections = nightly_candidate_sections(CHANGELOG, &released);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title, "Added");
        assert_eq!(sections[0].entries.len(), 2);
        assert_eq!(
            sections[0].entries[0],
            "- **Night radio.** A new station after dark.\n  It plays jazz."
        );
        let unreleased = nightly_candidate_sections(CHANGELOG, &HashSet::new());
        assert_eq!(unreleased.len(), 2);
    }

    #[test]
    fn formatting_joins_continuations_and_dedupes() {
        let sections = vec![
            Section {
                title: "Fixed".into(),
                entries: vec!["- a\n  b".into(), "- a\n  b".into()],
            },
            Section {
                title: "Added".into(),
                entries: vec!["* c".into()],
            },
        ];
        assert_eq!(
            format_sections(&sections, 2, SECTION_ORDER),
            "## Added\n* c\n\n## Fixed\n- a b"
        );
        assert_eq!(
            format_sections(&[], 2, SECTION_ORDER),
            "- No user-facing changes"
        );
    }

    #[test]
    fn normalization_ignores_markup() {
        assert_eq!(
            normalize_entry("- **Bold** `code` [link](http://x) \u{2014}  done"),
            "bold code link - done"
        );
    }

    #[test]
    fn a_snapshot_opens_with_a_summary_of_its_leads() {
        let sections = vec![
            Section {
                title: "Fixed".into(),
                entries: vec!["- **The horn works again.** It was silent.".into()],
            },
            Section {
                title: "Added".into(),
                entries: vec![
                    "- **Night radio.** A new station after dark.".into(),
                    "- Plain entry with no lead. Second sentence.".into(),
                ],
            },
        ];
        let notes = format_nightly_notes(&sections, "Changes in this snapshot", "", 3);
        assert!(
            notes.starts_with(
                "## Summary\n\n- Night radio.\n- Plain entry with no lead.\n\
                 - The horn works again.\n\n## Changes in this snapshot"
            ),
            "{notes}"
        );
        assert!(!notes.contains("Back up your saves"));
    }

    #[test]
    fn a_long_summary_counts_what_it_left_out() {
        let entries: Vec<String> = (0..15)
            .map(|n| format!("- **Change {n}.** Detail."))
            .collect();
        let sections = vec![Section {
            title: "Added".into(),
            entries,
        }];
        let summary = summary_block(&sections, SECTION_ORDER);
        assert!(summary.contains("- Change 11."), "{summary}");
        assert!(!summary.contains("- Change 12."), "{summary}");
        assert!(
            summary.ends_with("- And 3 more changes, listed below."),
            "{summary}"
        );
    }

    #[test]
    fn user_facing_paths() {
        assert!(is_user_facing_path("crates/ff-core/src/lib.rs"));
        assert!(!is_user_facing_path("crates/ff-core/tests/it/x.rs"));
        assert!(!is_user_facing_path("data/spider/x.md"));
        assert!(is_user_facing_path("data/radio_catalog.json"));
        assert!(!is_user_facing_path("crates/ff-tools/src/l10n/extract.rs"));
        assert!(is_user_facing_path("crates/ff-tools/src/release.rs"));
        assert!(is_user_facing_path("CHANGELOG.md"));
        assert!(!is_user_facing_path("tools/appimage/AppRun"));
    }

    #[test]
    fn bounding_keeps_round_robin_entries_within_the_limit() {
        let big = "x".repeat(50_000);
        let sections = vec![
            Section {
                title: "Added".into(),
                entries: vec![format!("- a{big}"), format!("- b{big}")],
            },
            Section {
                title: "Fixed".into(),
                entries: vec![format!("- {big}")],
            },
        ];
        let (kept, bounded) = bounded_sections(&sections, COMPLETE_LIST, &|c, t| {
            format_nightly_notes(c, "Changes", t, 2)
        });
        assert!(bounded);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].entries.len(), 1);
        assert_eq!(kept[1].entries.len(), 1);
    }
}
