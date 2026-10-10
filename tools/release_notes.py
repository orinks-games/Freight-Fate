"""Curated changelog utilities for Freight Fate releases.

``stable`` writes the matching version block from ``CHANGELOG.md``.
``nightly`` writes new ``Unreleased`` entries since the previous snapshot.
``should-build-nightly`` decides scheduled snapshots from curated entries or
explicit nightly markers, never from raw commit subjects.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CHANGELOG_PATH = Path("CHANGELOG.md")
NIGHTLY_HEADER = (
    "A preview of the next release, with the newest features and some rough "
    "edges. Back up your saves before you install it."
)
GITHUB_RELEASE_NOTES_SAFE_CHARACTERS = 120_000
FIRST_SNAPSHOT_COMPLETE_LIST = (
    "## Complete change list\n\n"
    "There are more changes than fit on this page. The full list is "
    "CHANGELOG.md in your game folder."
)
# A later snapshot can overflow too: a busy stretch, or a rewrite of the
# curated entries, which makes every bullet read as new to the previous tag.
SNAPSHOT_COMPLETE_LIST = (
    "## Complete change list\n\n"
    "There are more changes than fit on this page. The full list is "
    "CHANGELOG.md in your game folder."
)
STABLE_COMPLETE_LIST = (
    "## Complete change list\n\n"
    "There are more changes than fit on this page. The full list is "
    "CHANGELOG.md in your game folder."
)
# A stable release can open with a hand-written summary of everything since
# the last stable: these headings, under the version's own `## X.Y.Z - date`
# block, with the full per-snapshot list kept in `## X.Y.Z complete change
# list` below it. Nightlies never read them (they are not in
# PLAYER_FACING_SECTIONS), so the summary cannot resurface as new entries.
SUMMARY_SECTIONS = ("Highlights", "New features", "Fixes", "Changes")
STABLE_FULL_CHANGELOG = (
    "## Complete change list\n\n"
    "Every change in this release is listed in the "
    "[full Freight Fate {version} changelog]"
    "(https://github.com/orinks-games/Freight-Fate/blob/v{version}/CHANGELOG.md), "
    "on GitHub and as CHANGELOG.md in your game folder."
)
SECTION_ORDER = ("Added", "Changed", "Improved", "Fixed", "Removed", "Deprecated", "Security")
PLAYER_FACING_SECTIONS = SECTION_ORDER + ("Compatibility",)
INTERNAL_SECTIONS = (
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
)
NIGHTLY_BUILD_MARKERS = ("nightly: build", "[nightly build]")
SKIP_CHANGELOG_MARKERS = ("changelog: none", "[skip changelog]")
# `crates/` was added 2026-09-20. The gate was written when `src/` WAS the
# game; the Rust port moved every line of gameplay to `crates/` and the gate
# was never widened, so for the whole port a change to the shipping runtime
# could land with no entry and CI would not say a word. `data/` and `assets/`
# are what `src/freight_fate/` held besides the Python game: the world data
# and the shipped sounds.
USER_FACING_PATH_PREFIXES = ("data/", "assets/", "docs/", "crates/")
# ... but not a crate's test or bench binaries. Under the Python layout
# `tests/` sat beside the game and was never gated; a Rust test is the same
# kind of change, and the point is to restore the old rule, not tighten it.
# `data/spider/` is the map crawl's tooling scripts and notes, never loaded.
NOT_USER_FACING = re.compile(r"^(?:crates/[^/]+/(?:tests|benches)/|data/spider/)")
USER_FACING_PATHS = {
    "CHANGELOG.md",
    "README.md",
    "pyproject.toml",
    "tools/build_release.py",
    "tools/release_notes.py",
}


@dataclass(frozen=True)
class ChangelogSection:
    title: str
    entries: tuple[str, ...]


@dataclass(frozen=True)
class ReleaseBlock:
    heading: str
    body: str


def run_git(args: list[str]) -> str:
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True, encoding="utf-8").strip()


def git_output_lines(args: list[str]) -> list[str]:
    return [line for line in run_git(args).splitlines() if line]


def changelog_file() -> Path:
    return ROOT / CHANGELOG_PATH


def changelog_at(ref: str) -> str:
    try:
        return run_git(["show", f"{ref}:{CHANGELOG_PATH.as_posix()}"])
    except subprocess.CalledProcessError:
        return ""


def extract_release_block(text: str, heading_pattern: str) -> str:
    match = re.search(heading_pattern, text, re.IGNORECASE | re.MULTILINE)
    if not match:
        return ""
    start = match.end()
    next_heading = re.search(r"^##\s+", text[start:], re.MULTILINE)
    end = start + next_heading.start() if next_heading else len(text)
    return text[start:end].strip()


def unreleased_block(text: str) -> str:
    return extract_release_block(text, r"^##\s+\[?Unreleased\]?\s*$")


def version_block(text: str, version: str) -> str:
    normalized = version.removeprefix("v")
    return extract_release_block(
        text,
        rf"^##\s+\[?v?{re.escape(normalized)}\]?(?:\s+-\s+\d{{4}}-\d{{2}}-\d{{2}})?\s*$",
    )


def parse_sections(markdown: str, *, min_heading_level: int = 3) -> list[ChangelogSection]:
    sections: list[ChangelogSection] = []
    current_title = ""
    current_entries: list[str] = []
    current_entry: list[str] = []

    def flush_entry() -> None:
        nonlocal current_entry
        if current_entry:
            current_entries.append("\n".join(current_entry).rstrip())
            current_entry = []

    def flush_section() -> None:
        nonlocal current_entries
        flush_entry()
        if current_title and current_entries:
            sections.append(ChangelogSection(current_title, tuple(current_entries)))
        current_entries = []

    for line in markdown.splitlines():
        heading = re.match(r"^(#{2,6})\s+(.+?)\s*$", line)
        if heading:
            if len(heading.group(1)) < min_heading_level:
                continue
            flush_section()
            current_title = heading.group(2)
            continue
        if re.match(r"^[-*+]\s+", line):
            flush_entry()
            current_entry.append(line)
            continue
        if current_entry and (line.startswith((" ", "\t")) or not line.strip()):
            current_entry.append(line)

    flush_section()
    return sections


def release_blocks(text: str) -> list[ReleaseBlock]:
    matches = list(re.finditer(r"^##\s+(.+?)\s*$", text, re.MULTILINE))
    blocks: list[ReleaseBlock] = []
    for index, match in enumerate(matches):
        start = match.end()
        end = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        blocks.append(ReleaseBlock(match.group(1).strip(), text[start:end].strip()))
    return blocks


def eligible_sections(markdown: str) -> list[ChangelogSection]:
    return [
        section
        for section in parse_sections(markdown)
        if section.title in PLAYER_FACING_SECTIONS and section.title not in INTERNAL_SECTIONS
    ]


def released_versions() -> set[str]:
    """Versions that already have a published stable tag (``vX.Y.Z``).

    A version block in the changelog is only "staged" until its stable tag
    exists; once released it must not resurface in developer snapshots.
    """
    try:
        tags = git_output_lines(["tag", "--list", "v*.*.*"])
    except subprocess.CalledProcessError:
        return set()
    return {tag.removeprefix("v") for tag in tags}


def nightly_candidate_sections(
    text: str, released: set[str] | None = None
) -> list[ChangelogSection]:
    """Player-facing changelog entries that can feed developer snapshots.

    Release prep sometimes moves curated player-facing notes from
    ``Unreleased`` into the next version block before the stable tag exists.
    Scheduled nightlies still need those entries, while explicitly internal
    buckets should not force a player snapshot. A version block whose stable
    tag already exists has shipped, so it is skipped to avoid re-advertising
    released features in nightly notes.
    """
    released = released or set()
    sections: list[ChangelogSection] = []
    for block in release_blocks(text):
        heading = block.heading.casefold().lstrip("[")
        if heading.startswith("unreleased"):
            sections.extend(eligible_sections(block.body))
            continue
        version_match = re.match(r"v?(\d+\.\d+\.\d+)", heading)
        if version_match:
            if version_match.group(1) in released:
                continue  # already shipped under a stable tag
            sections.extend(eligible_sections(block.body))
    return sections


def normalize_entry(entry: str) -> str:
    entry = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", entry)
    entry = re.sub(r"`([^`]+)`", r"\1", entry)
    entry = re.sub(r"(\*\*|__|\*|_)", "", entry)
    entry = re.sub(r"^[-*+]\s+", "", entry.strip())
    entry = re.sub(r"\s+[-\u2013\u2014]\s+", " - ", entry)
    entry = re.sub(r"\s+", " ", entry)
    return entry.casefold().strip()


def flatten_markdown(body: str) -> list[str]:
    """Release-notes markdown as plain, speakable lines.

    Mirrors ``flatten_markdown`` in crates/freight-fate/src/updater.rs, which
    is how the game's updater reads these notes aloud.
    """
    lines: list[str] = []
    for raw in (body or "").splitlines():
        line = raw.strip()
        if not line or set(line) <= {"-", "=", "*", "_"}:
            continue
        line = re.sub(r"^#{1,6}\s+", "", line)  # headings
        line = re.sub(r"^[-*+]\s+", "", line)  # bullets
        line = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", line)  # links
        line = re.sub(r"(\*\*|__|\*|_|`)", "", line)  # emphasis/code
        if line:
            lines.append(line)
    return lines


def format_entry(entry: str) -> str:
    lines = [line.strip() for line in entry.splitlines() if line.strip()]
    if not lines:
        return ""
    marker_match = re.match(r"^([-*+]\s+)(.*)$", lines[0])
    marker = marker_match.group(1) if marker_match else "- "
    first_text = marker_match.group(2) if marker_match else lines[0]
    return marker + " ".join([first_text, *lines[1:]])


def format_sections(
    sections: list[ChangelogSection],
    *,
    heading_level: int = 2,
    order: tuple[str, ...] = SECTION_ORDER,
) -> str:
    if not sections:
        return "- No user-facing changes"

    by_title: dict[str, list[str]] = {}
    for section in sections:
        by_title.setdefault(section.title, []).extend(section.entries)
    ordered_titles = [title for title in order if title in by_title]
    ordered_titles.extend(title for title in by_title if title not in ordered_titles)

    chunks: list[str] = []
    for title in ordered_titles:
        entries = "\n".join(
            entry for entry in dict.fromkeys(format_entry(e) for e in by_title[title]) if entry
        )
        chunks.append(f"{'#' * heading_level} {title}\n{entries}")
    return "\n\n".join(chunks).strip()


def entries_from_sections(sections: list[ChangelogSection]) -> set[str]:
    return {normalize_entry(entry) for section in sections for entry in section.entries}


def excluded_entries_from_notes(path: str) -> set[str]:
    if not path:
        return set()
    notes_path = Path(path)
    if not notes_path.exists():
        return set()
    return entries_from_sections(
        parse_sections(notes_path.read_text(encoding="utf-8"), min_heading_level=2)
    )


REPUBLISHED_FILE = Path("tools/release_notes_republished.txt")


def republished_entries(released: set[str] | None = None) -> set[str]:
    """Entries a rewrite republished rather than added.

    A snapshot lists the bullets whose text is not in the changelog at the
    previous tag, so a commit that rewords every entry makes the next snapshot
    announce the whole block as new. ``tools/release_notes_republished.txt``
    names that commit (``ref = <sha>``): everything in the changelog there
    counts as already published, except the bullets whose bold lead the file
    lists on ``new = ...`` lines, which were genuinely new when the rewrite
    landed and still have to go out. Delete the file once a snapshot tag
    carries the rewritten text; it is harmless but stale after that.
    """
    path = ROOT / REPUBLISHED_FILE
    if not path.exists():
        return set()
    ref = ""
    still_new: list[str] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        key, _, value = line.partition("=")
        key, value = key.strip(), value.strip()
        if key == "ref":
            ref = value
        elif key == "new":
            still_new.append(normalize_entry(value))
    if not ref:
        return set()
    text = changelog_at(ref)
    if not text:
        raise SystemExit(f"{REPUBLISHED_FILE}: commit {ref} is not in this clone")
    if released is None:
        released = released_versions()
    entries = entries_from_sections(nightly_candidate_sections(text, released))
    return {entry for entry in entries if not any(entry.startswith(lead) for lead in still_new)}


def sections_added_since(
    base_ref: str,
    head_text: str,
    extra_excluded_entries: set[str] | None = None,
    released: set[str] | None = None,
) -> list[ChangelogSection]:
    if released is None:
        released = released_versions()
    base_entries = entries_from_sections(
        nightly_candidate_sections(changelog_at(base_ref), released)
    )
    base_entries.update(republished_entries(released))
    if extra_excluded_entries:
        base_entries.update(extra_excluded_entries)

    added: list[ChangelogSection] = []
    for section in nightly_candidate_sections(head_text, released):
        entries = tuple(
            entry for entry in section.entries if normalize_entry(entry) not in base_entries
        )
        if entries:
            added.append(ChangelogSection(section.title, entries))
    return added


def format_stable_notes(sections: list[ChangelogSection], footer: str = "") -> str:
    # A stable release is read by players upgrading in place, so what decides
    # whether their install or career comes across leads the page.
    body = format_sections(sections, order=("Compatibility",) + SUMMARY_SECTIONS + SECTION_ORDER)
    return f"{body}\n\n{footer}" if footer else body


def stable_notes(version: str) -> str:
    """The version's block, or Unreleased, bounded like a snapshot's notes.

    A block with a curated summary (``SUMMARY_SECTIONS``) is published whole,
    Compatibility first, ending with a link to the full changelog at the tag.
    1.9's Unreleased block alone is past GitHub's limit, so an unbounded
    stable would fail the tag build's size check after every platform built.
    """
    changelog_text = changelog_file().read_text(encoding="utf-8")
    block = version_block(changelog_text, version) or unreleased_block(changelog_text)
    sections = parse_sections(block)
    if any(section.title in SUMMARY_SECTIONS for section in sections):
        # A curated summary is short by construction; it points at the full
        # list instead of being bounded like one.
        footer = STABLE_FULL_CHANGELOG.format(version=version.removeprefix("v"))
        return format_stable_notes(sections, footer)
    sections, was_bounded = bounded_sections(
        sections, "", STABLE_COMPLETE_LIST, render=format_stable_notes
    )
    return format_stable_notes(sections, STABLE_COMPLETE_LIST if was_bounded else "")


def format_nightly_notes(
    sections: list[ChangelogSection],
    changes_heading: str,
    footer: str = "",
    *,
    section_heading_level: int = 2,
) -> str:
    body = format_sections(sections, heading_level=section_heading_level)
    notes = f"{NIGHTLY_HEADER}\n\n## {changes_heading}\n\n{body}"
    return f"{notes}\n\n{footer}" if footer else notes


def first_snapshot_fits(notes: str) -> bool:
    """Whether notes plus the file's final line feed fit the safe limit."""
    return len(notes) + 1 <= GITHUB_RELEASE_NOTES_SAFE_CHARACTERS


def bounded_first_snapshot_sections(
    sections: list[ChangelogSection],
) -> tuple[list[ChangelogSection], bool]:
    """Keep complete recent entries from every section within GitHub's limit."""
    return bounded_sections(
        sections,
        "Changes in this snapshot",
        FIRST_SNAPSHOT_COMPLETE_LIST,
        section_heading_level=3,
    )


def bounded_sections(
    sections: list[ChangelogSection],
    changes_heading: str,
    footer: str,
    *,
    section_heading_level: int = 2,
    render=None,
) -> tuple[list[ChangelogSection], bool]:
    """Keep complete recent entries from every section within GitHub's limit.

    Returns the sections to publish and whether anything was left out; when
    something was, the caller appends ``footer`` so the page says where the
    rest is. Entries are taken in file order, round-robin across sections, so
    the newest of every kind survives rather than all of one section.
    ``render(sections, footer)`` is the page being measured; snapshot notes
    by default.
    """
    if render is None:

        def render(chosen: list[ChangelogSection], tail: str) -> str:
            return format_nightly_notes(
                chosen, changes_heading, tail, section_heading_level=section_heading_level
            )

    if first_snapshot_fits(render(sections, "")):
        return sections, False

    selected: list[list[str]] = [[] for _ in sections]
    offsets = [0 for _ in sections]

    def selected_sections() -> list[ChangelogSection]:
        return [
            ChangelogSection(section.title, tuple(selected[index]))
            for index, section in enumerate(sections)
            if selected[index]
        ]

    while True:
        added_this_round = False
        for index, section in enumerate(sections):
            if offsets[index] >= len(section.entries):
                continue
            entry = section.entries[offsets[index]]
            selected[index].append(entry)
            if first_snapshot_fits(render(selected_sections(), footer)):
                offsets[index] += 1
                added_this_round = True
            else:
                selected[index].pop()
        if not added_this_round:
            break

    return selected_sections(), True


def nightly_notes(
    previous_tag: str = "",
    exclude_notes: str = "",
    exclude_stable_notes: str = "",
    *,
    first_snapshot: bool = False,
) -> str:
    changelog_text = changelog_file().read_text(encoding="utf-8")
    excluded_entries = excluded_entries_from_notes(exclude_notes)
    excluded_entries.update(excluded_entries_from_notes(exclude_stable_notes))
    released = released_versions()
    if previous_tag:
        sections = sections_added_since(previous_tag, changelog_text, excluded_entries, released)
    else:
        sections = nightly_candidate_sections(changelog_text, released)
    changes_heading = (
        "Changes in this snapshot" if first_snapshot else "Changes since the previous snapshot"
    )
    section_heading_level = 3 if first_snapshot else 2
    complete_list = FIRST_SNAPSHOT_COMPLETE_LIST if first_snapshot else SNAPSHOT_COMPLETE_LIST
    sections, was_bounded = bounded_sections(
        sections, changes_heading, complete_list, section_heading_level=section_heading_level
    )
    footer = complete_list if was_bounded else ""
    return format_nightly_notes(
        sections,
        changes_heading,
        footer,
        section_heading_level=section_heading_level,
    )


def current_branch() -> str:
    return run_git(["branch", "--show-current"])


def resolve_base(base: str) -> str:
    if base != "auto":
        return base
    if current_branch() == "main":
        return "origin/main"
    return nearest_release_line()


def nearest_release_line() -> str:
    """Which release line the current branch was cut from.

    Feature and fix branches sit on dev, but a hotfix is cut from main and
    never contains dev -- so comparing it against dev counts dev's bullets as
    already present and rejects the push over a changelog entry that is right
    there. This is measured as the nearest branch point rather than plain
    ancestry, because main is an ancestor of dev as well: ancestry alone would
    send every dev branch that has fallen behind to main instead.
    """
    candidates = []
    for ref in ("origin/dev", "origin/main"):
        try:
            counts = run_git(["rev-list", "--count", "--left-right", f"{ref}...HEAD"])
        except Exception:
            continue  # a clone without that remote branch
        # Both sides of the divergence, not just ours: a hotfix is zero
        # commits behind main and a long way behind dev, which is the whole
        # signal. Counting only our own side would call those two the same.
        candidates.append((sum(int(n) for n in counts.split()), ref))
    # Ties favour dev, which sorts first and is where ordinary work belongs.
    return min(candidates)[1] if candidates else "origin/dev"


def ref_is_ancestor(ancestor: str, descendant: str) -> bool:
    return (
        subprocess.run(
            ["git", "merge-base", "--is-ancestor", ancestor, descendant],
            cwd=ROOT,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        ).returncode
        == 0
    )


def commit_messages(base: str, head: str) -> list[str]:
    commits = git_output_lines(["log", "--no-merges", "--format=%H", f"{base}..{head}"])
    return [run_git(["show", "-s", "--format=%B", commit]) for commit in commits]


def new_commit_messages(base: str, head: str) -> list[str]:
    """Messages of range commits that were authored for this push.

    A dev push that merges release history back from main carries commits
    that already passed this gate when they landed on main; requiring
    fresh changelog bullets (or skip markers) from them would block every
    sync. Exclude anything already reachable from origin/main — on the
    main branch itself the base is origin/main, so nothing is excluded.
    """
    commits = git_output_lines(["log", "--no-merges", "--format=%H", f"{base}..{head}"])
    fresh = [commit for commit in commits if not ref_is_ancestor(commit, "origin/main")]
    return [run_git(["show", "-s", "--format=%B", commit]) for commit in fresh]


def commits_request_nightly_build(base: str, head: str) -> bool:
    return any(
        marker in message.casefold()
        for message in commit_messages(base, head)
        for marker in NIGHTLY_BUILD_MARKERS
    )


def commits_opt_out_of_changelog(base: str, head: str) -> bool:
    messages = new_commit_messages(base, head)
    if not messages:
        # Every commit in the range is already on origin/main: this push
        # only syncs release history, which was gated when it landed there.
        return True
    return all(
        any(marker in message.casefold() for marker in SKIP_CHANGELOG_MARKERS)
        for message in messages
    )


def is_user_facing_path(path: str) -> bool:
    normalized = path.replace("\\", "/")
    if NOT_USER_FACING.match(normalized):
        return False
    return normalized in USER_FACING_PATHS or normalized.startswith(USER_FACING_PATH_PREFIXES)


def changed_files(base: str, head: str) -> list[str]:
    return git_output_lines(["diff", "--name-only", f"{base}..{head}"])


def all_player_facing_sections(text: str) -> list[ChangelogSection]:
    """Every player-facing changelog section, released or not."""
    sections: list[ChangelogSection] = []
    for block in release_blocks(text):
        sections.extend(eligible_sections(block.body))
    return sections


def changelog_added_entries(base: str, head: str) -> list[str]:
    """Player-facing bullets present at ``head`` but not at ``base``.

    Entries count wherever they land in the changelog. A single-push
    release sync moves curated notes from ``Unreleased`` straight under a
    freshly tagged version heading; the gate must still credit them as
    this push's additions rather than dismissing the block as released.
    """
    base_entries = entries_from_sections(all_player_facing_sections(changelog_at(base)))
    head_text = (
        changelog_at(head) if head != "HEAD" else changelog_file().read_text(encoding="utf-8")
    )
    return [
        entry
        for section in all_player_facing_sections(head_text)
        for entry in section.entries
        if normalize_entry(entry) not in base_entries
    ]


def check_command(args: argparse.Namespace) -> int:
    base = resolve_base(args.base)
    files = changed_files(base, args.head)
    user_facing = [path for path in files if is_user_facing_path(path)]
    if not user_facing:
        print("No user-facing paths changed.")
        return 0

    if commits_opt_out_of_changelog(base, args.head):
        print("All commits opt out of the changelog gate via a skip marker.")
        return 0

    if CHANGELOG_PATH.as_posix() not in files:
        print("User-facing paths changed without updating CHANGELOG.md:", file=sys.stderr)
        for path in user_facing:
            print(f"- {path}", file=sys.stderr)
        return 1

    if not changelog_added_entries(base, args.head):
        print(
            "CHANGELOG.md changed, but no new player-facing bullet was added.",
            file=sys.stderr,
        )
        return 1

    print("Found new CHANGELOG.md entries for user-facing changes.")
    return 0


def should_build_nightly_command(args: argparse.Namespace) -> int:
    if not args.previous_tag:
        print("should_build=true")
        print("No previous nightly tag found; building once.", file=sys.stderr)
        return 0

    latest_stable_tag = args.latest_stable_tag
    if latest_stable_tag and ref_is_ancestor(args.head, latest_stable_tag):
        print("should_build=false")
        print("Latest stable release already contains this commit.", file=sys.stderr)
        return 0

    baseline_tag = args.previous_tag
    if latest_stable_tag and ref_is_ancestor(args.previous_tag, latest_stable_tag):
        baseline_tag = latest_stable_tag

    if commits_request_nightly_build(baseline_tag, args.head):
        print("should_build=true")
        print("Nightly build requested by commit marker.", file=sys.stderr)
        return 0

    excluded_entries = excluded_entries_from_notes(args.exclude_notes)
    excluded_entries.update(excluded_entries_from_notes(args.exclude_stable_notes))
    sections = sections_added_since(
        baseline_tag,
        changelog_file().read_text(encoding="utf-8"),
        excluded_entries,
    )
    if sections:
        print("should_build=true")
        print("New curated changelog entries found for nightly build.", file=sys.stderr)
    else:
        print("should_build=false")
        print("No new curated changelog entries or nightly build marker found.", file=sys.stderr)
    return 0


def write_notes_command(args: argparse.Namespace) -> int:
    if args.kind == "stable":
        if not args.version:
            raise SystemExit("stable notes need --version")
        notes = stable_notes(args.version)
    else:
        notes = nightly_notes(
            args.previous_tag,
            args.exclude_notes,
            getattr(args, "exclude_stable_notes", ""),
            first_snapshot=getattr(args, "first_snapshot", False),
        )
    Path(args.output).write_text(notes + "\n", encoding="utf-8", newline="\n")
    print(f"Wrote release notes to {args.output}.")
    return 0


def check_size_command(args: argparse.Namespace) -> int:
    characters = len(Path(args.input).read_bytes().decode("utf-8"))
    if characters > args.max_characters:
        print(
            f"Release notes contain {characters} characters, exceeding the "
            f"{args.max_characters}-character publication limit.",
            file=sys.stderr,
        )
        return 1
    print(f"Release notes fit the publication limit ({characters} characters).")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    for kind in ("stable", "nightly"):
        notes = subparsers.add_parser(kind, help=f"Write {kind} release notes.")
        notes.set_defaults(func=write_notes_command, kind=kind)
        notes.add_argument("--version", default="")
        notes.add_argument("--previous-tag", default="")
        notes.add_argument("--exclude-notes", default="")
        notes.add_argument("--exclude-stable-notes", default="")
        if kind == "nightly":
            notes.add_argument("--first-snapshot", action="store_true")
        notes.add_argument("--output", required=True)

    should_build = subparsers.add_parser(
        "should-build-nightly",
        help="Decide whether a scheduled nightly should build artifacts.",
    )
    should_build.add_argument("--previous-tag", default="")
    should_build.add_argument("--exclude-notes", default="")
    should_build.add_argument("--latest-stable-tag", default="")
    should_build.add_argument("--exclude-stable-notes", default="")
    should_build.add_argument("--head", default="HEAD")
    should_build.set_defaults(func=should_build_nightly_command)

    check = subparsers.add_parser("check", help="Require Unreleased changelog entries.")
    check.add_argument("--base", required=True, help="Base ref, or 'auto'.")
    check.add_argument("--head", default="HEAD")
    check.set_defaults(func=check_command)

    check_size = subparsers.add_parser(
        "check-size", help="Require release notes to fit the publication limit."
    )
    check_size.add_argument("--input", required=True)
    check_size.add_argument("--max-characters", required=True, type=int)
    check_size.set_defaults(func=check_size_command)

    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
