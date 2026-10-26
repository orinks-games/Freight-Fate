# Agent Contributor Guide

The guide lives in **`CLAUDE.md`** in this same directory -- read that
file. It is the canonical copy rather than an include, so that every agent
harness picks the rules up directly instead of having to follow a pointer
to find them. This file is the pointer for harnesses that look here first.

Opening a PR: write the test plan and drive it through the agent server,
as `CLAUDE.md` describes under **Test plan**.

Full contributor policy: `CONTRIBUTING.md`. Feature status per release
line: `ROADMAP.md`.

## Rust best practices

The canonical Rust engineering rules are in `CLAUDE.md` under **Rust
engineering practices**. In particular: keep `unsafe` narrowly wrapped and
documented; never perform unbounded I/O, IPC, joins, or window-manager work in
`Drop` or the game loop; use explicit idempotent shutdown with cancellation and
measured bounds; treat external systems as fallible; and run the pinned format,
Clippy, test, and adversarial gates before shipping Rust gameplay changes.
