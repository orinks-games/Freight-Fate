---
name: batched-agent-testing
description: Run multi-step Freight Fate `--agent-server` test plans in one turn using `cargo tools agent-script` to conserve LLM token usage instead of interactive MCP round-trips.
---

# Batched Agent-Server Testing

## When to use

Use `cargo tools agent-script` whenever you need to drive a multi-step
`freightfate --agent-server` playtest (PR test plans, driving/braking
sequences, menu flows, or audio/transcript checks).

Driving `--agent-server` interactively one MCP tool call per turn sends the
full conversation context back to the model on every step. Running the same
sequence through `cargo tools agent-script` executes every step in a single
turn and prints only the filtered lines you need.

## Usage

Build the binary once (`cargo build -p freight-fate --bin freightfate` or
`--release`), then pipe a JSON array of steps on stdin (`-`) or pass a file
path:

```bash
cargo tools agent-script --filter "[sound],[spoken],[engine]" - <<'EOF'
[
  {"tool": "scenario", "args": {"truck": "heavy", "settings": {"music_volume": 0.0, "radio_enabled": false}}},
  {"tool": "start_at", "args": {"feature": "departure"}},
  {"label": "accelerate", "tool": "pedal", "args": {"key": "up", "seconds": 8.0}, "expect": ["vehicle/turbo_spool"]},
  {"label": "brake", "tool": "pedal", "args": {"key": "down", "seconds": 8.0}, "expect": ["vehicle/brake_hiss_bed"]}
]
EOF
```

## Step schema

Each object in the JSON array supports:

- `tool` (required): MCP tool name (`scenario`, `start_at`, `press`, `select`, `pedal`, `cruise`, `hold`, `release`, `listen`, `wait_for`, `status`, `quit_game`, etc.).
- `args` (optional): JSON object of arguments for that tool (defaults to `{}`).
- `label` (optional): Heading printed for the step.
- `expect` (optional): Array of substrings that must appear in the step's output (exits non-zero if any is missing).
- `forbid` (optional): Array of substrings that must NOT appear in the step's output (exits non-zero if any matches).

`quit_game` is sent automatically at the end if omitted, and an isolated
temporary `FREIGHT_FATE_DATA_DIR` is created and cleaned up automatically
(pass `--keep-dir` to keep it).

## Flags

- `--bin PATH`: Explicit `freightfate` binary path (defaults to `target/release/freightfate`, then `target/debug/freightfate`).
- `--filter PATTERNS`: Comma-separated substrings; only output lines containing at least one pattern are printed.
- `--verbose`: Include quiet steps (`(quiet -- nothing new to hear)`).
- `--json`: Emit structured JSON instead of plain text.
- `--keep-dir`: Keep the temporary `FREIGHT_FATE_DATA_DIR` and print its path.
