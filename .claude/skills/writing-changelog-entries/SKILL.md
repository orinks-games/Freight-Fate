---
name: writing-changelog-entries
description: Use when adding or editing a bullet in CHANGELOG.md, writing release notes or What's New text, or when a changelog entry has grown past two sentences
---

# Writing changelog entries

## Overview

A changelog bullet is one change, stated once, in the words a player would
use to look for it. Release notes are built from these bullets and read
aloud by screen readers, and the nightly page is a short list of them, so
every extra sentence is one the player has to sit through to reach the next
change. The lead sentence is the entry; everything after it is optional.

## The entry

An entry has exactly these parts, in this order:

1. **Bold lead.** One present-tense sentence saying what is different now.
   It must stand alone as the whole entry.
2. One sentence, at most, of what the player hears, sees, or does now. Only
   when the lead cannot carry it.
3. A credit or reference, in parentheses, at the end.

Length: under 25 words. 40 is the ceiling. Count them before saving.

Example, and this is the entire entry:

- **Speed-limit drop warnings no longer double up.** The advance warning
  before a big posted-limit drop speaks once now.

## Specific, not vague; short, not mechanical

Name the exact thing the player knows (the feature, the place, the key)
and the exact behaviour they will notice. Leave out how the game decides
it. A second sentence is for a consequence the player will meet in play,
never an edge case or a rule only the code knows.

- Too vague: **Fixed training course fatigue.** It does not say what the
  player will notice now.
- Too mechanical: **Sitting through a training course now tires you, about
  half as fast as driving.** A multi-day course counts only its last
  daytime class day.
- Right: **Training classes now leave you tired, at about half the rate of
  driving.** If a class leaves you drowsy, you are told to sleep before you
  drive.

A fix to something that already shipped must read as a fix. Name the
feature the player already has, so the bullet reads as a change to it
and not as a new feature arriving. When one cycle brings several fixes to
the same feature, merge them into one bullet unless a player would look
each one up on its own.

## Where the rest goes

| Material | Goes to |
|---|---|
| Root cause, why it happened | the commit message |
| How it works, what it reads, the thresholds | the PR body |
| Every sub-case, every affected road, item or menu | the manual under docs/, or its own bullet if a player would search for it on its own |
| The before-and-after story | nowhere; the lead says what is different |
| Reassurance that something else is unchanged | nowhere |
| Statistics, counts, percentages | ROADMAP.md |
| Maintainer-facing changes (CI, harness, agent tooling, headless runs) | no bullet; `[skip changelog]` in the commit |

## Checklist before saving

- The lead alone tells the player what changed.
- It names the specific feature or place and what the player notices, and
  says nothing about how the game decides it.
- One sentence after the lead, or none.
- Under 25 words, 40 at most.
- The canonical noun from docs/ontology.md. Keys, settings and menu rows are
  named; files, functions, constants and data sources are not.
- Section is one of Added, Changed, Improved, Fixed, Removed, Deprecated,
  Security, Compatibility. Any other heading is dropped from release notes.
  The one exception is a stable release's curated summary, directly under
  its `## X.Y.Z - date` heading: Compatibility, Highlights, New features,
  Fixes, Changes. It is published whole as the stable notes; the detailed
  bullets stay under `## X.Y.Z complete change list`.
- No symbols, tables, or jargon: it is read aloud.

## Common mistakes

- One bullet carrying a whole feature's sub-parts. Split by what a player
  would look up, not by implementation part, and send the rest to the manual.
- Editing an old bullet to satisfy the CI gate. The gate needs a new bullet.
- A second sentence that restates the lead in longer words.
- A second sentence that explains an internal rule or edge case
  ("counts only the last class day", "judged by the local clock").
- A Fixed bullet worded so it sounds like a new feature.
- Naming a key or setting the player does not have on their controls.
