# Contributing to Freight Fate

Thanks for helping make Freight Fate better. This project is audio-first and
accessibility-first, so contributions should keep blind and low-vision players
at the center of every change.

## Contributions by release line

Freight Fate 1.9 is in development on `feat/career-1.9`, with preview
snapshots available for testing. The 1.8 line is in maintenance and accepts
bug fixes only.

For a new feature, open an issue first so maintainers can agree on its scope
and release line. Check [the roadmap](ROADMAP.md) for the work planned for
each release.

## Branch targets

- Open feature, fix, data, and documentation pull requests against `dev`.
- Use `main` only for stable release, hotfix, or release-sync work.
- If your PR targets the wrong branch, a maintainer may retarget it before
  review.

## Building

The prerequisites are in [Run from source](README.md#run-from-source): Rust
through rustup and CMake on every platform, plus the Visual Studio Build
Tools on Windows, the Xcode Command Line Tools on macOS, and GCC 13, ALSA's
development files and the SDL2 development packages on Linux. The game's
engine, Hearken, is a git dependency that Cargo fetches; there is no audio
library to download. To build against a local Hearken checkout, see the
untracked `[patch]` recipe in [CLAUDE.md](CLAUDE.md#rust-engineering-practices).

## Before opening a pull request

- Keep practical code files at or below 1000 lines. Split large code or test
  files into cohesive modules instead of adding more to an oversized file.
- Gameplay is Rust on Career 1.9: run `cargo test -p ff-core` and
  `cargo test -p freight-fate` for anything the player can hear or do. That
  local run is the full suite; the per-push CI skips the whole-map sweeps
  (`#[cfg_attr(ci_quick, ignore = ...)]`) and the nightly snapshot runs them.
- Changes to the build, release, translation or data tools: run
  `cargo test -p ff-tools`.
- For user-facing changes, include how you checked the spoken text, keyboard
  flow, or other accessibility impact.
- Include a sandboxed agent-server session when live gameplay testing is
  authorized. See [agent-server testing](CLAUDE.md#rust----gameplay-and-what-ci-gates)
  for the controls and evidence to record. Keep automated tests, agent play,
  and the owner's listening pass distinct in the results.
- For player-facing changes, add a `CHANGELOG.md` entry (see Changelog
  entries below); CI enforces this.

## Accessibility expectations

- Every gameplay path must remain usable by keyboard and screen reader.
- Speech text should be clear, player-facing, and free of maintainer or CI
  jargon.
- Do not replace spoken information with visual-only cues.
- If you add or change menu items, driving prompts, warnings, settings, or
  status text, test the spoken result.

## Translations

Every spoken line is translated where it leaves for the speech channels, from
the PO files in `lang/`; [lang/README.md](lang/README.md) is the translator's
guide. After changing spoken text, run `cargo tools l10n-extract`
so the template and every language file pick up the new line; CI fails
while they are stale. A translation correction can arrive as a pull
request against a `lang/*.po` file or as a Translation correction issue.

## World and route data

World data changes are welcome. Please keep them deterministic and offline:

- Route data must load without network access during normal play.
- Add sources or source notes for real-world facilities, stops, speed limits,
  tolls, interchanges, or other mapped data.
- Do not include experimental regions in playable data unless the index or
  loader explicitly enables them.
- Avoid raw OpenStreetMap tags or source-only text in player-facing names.
- After data changes, run the world and route tests, such as:

  ```powershell
  cargo test -p ff-core data_world
  ```

  and check the index: `cargo tools index-world --check`.

## Changelog entries

Nightly and stable release notes are built only from the curated entries in
`CHANGELOG.md` -- never from commit subjects -- so a player-facing change
without an entry ships silently. CI fails a pull request that changes
user-facing paths without adding one: anything under `data/` (except
`data/spider/`), `assets/`, `docs/` or `crates/` (except a crate's `tests/`
and `benches/`), plus `CHANGELOG.md`, `README.md`, `pyproject.toml`,
`cargo tools build-release` and `cargo tools release-notes`.

- Add a bullet under `## Unreleased` in the fitting section (`Added`,
  `Changed`, `Fixed`, and so on).
- Write for players, not maintainers: a bold lead sentence that says what
  is different now, then at most one sentence about what they will hear or
  notice. Under 25 words. Entries are read aloud by screen readers, so avoid
  jargon, tables, and decorative symbols. The full recipe, and where the
  rationale and sub-cases go instead, is in
  `.claude/skills/writing-changelog-entries/SKILL.md`.
- A change with nothing player-facing in it (internal refactors, CI, tests,
  tooling) can skip the entry by putting `[skip changelog]` or
  `changelog: none` in every commit message of the pull request.

## Pull request notes

In your PR body, briefly say:

- what changed and why;
- what players or maintainers will notice;
- a test plan: one checkbox per behavior the change touches, checked off
  once you have confirmed it, with what you heard;
- any accessibility impact.

Small PRs are easiest to review, but cohesive data restructures are fine when
the tests show the playable world still loads and routes correctly.
