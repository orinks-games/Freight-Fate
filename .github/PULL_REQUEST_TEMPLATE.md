<!-- Thanks for contributing! See CONTRIBUTING.md for branch targets,
     test commands, and accessibility expectations.

     Target the `dev` branch, not `main`. GitHub defaults new PRs to
     `main`, but `main` is only for stable release work -- change the
     base branch to `dev` before opening this PR. -->

## What changed and why

## What players or maintainers will notice

## Test plan

<!-- Build this list from your diff: one checkbox per thing a reviewer
     should confirm. Check a box only once you have done it, and put what
     you saw or heard under it. Leave a box unchecked, with a note, when a
     step failed or you could not run it.

     Automated checks: list the commands you ran. Drop any that do not
     apply to what you touched. -->

### Automated checks

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --all-targets --locked -- -D warnings`
- [ ] `cargo test -p ff-core -p freight-fate`
- [ ] Adversarial battery: `cargo run -p freight-fate --bin freightfate -- --break-battery`
- [ ] `cargo test -p ff-tools` (build and release tooling only)

### In-game checks

<!-- One box per changed behavior, written as setup, action, and what the
     player should hear. For example:

     - [ ] Staged at a downgrade with cruise at 65: the downgrade
           warning is spoken before the grade starts.
           Heard: (paste the line the game spoke)

     Cover the edge cases the change touches too: the quiet verbosity
     setting, a menu reached from more than one place, a key pressed twice,
     a drive interrupted by rest or a scale. These can be driven through
     the game's agent server; see "Test plan" in CLAUDE.md. -->

- [ ]

### Manual testing

<!-- Anything that turns on how speech or sound feels (timing, loudness,
     whether a line is too much), for a person to try by ear. Write "None"
     if nothing needs it. -->

## Accessibility impact

<!-- Every gameplay path must stay usable by keyboard and screen reader.
     Say how you checked spoken text or keyboard flow, or why this change
     has no accessibility impact. -->

## Changelog

CI requires a `CHANGELOG.md` entry when user-facing paths (such as
`crates/`, `data/` or `docs/`) change. Check one:

- [ ] I added a player-facing bullet under `## Unreleased` in
      `CHANGELOG.md`, written in plain player language and matching the
      style of the existing entries.
- [ ] This change is not player-facing, and every commit message in this
      PR includes `[skip changelog]`.
