#!/bin/bash
# SessionStart hook for Claude Code on the web.
#
# Brings a fresh cloud container up to a contributor's checkout: the pinned
# Rust toolchain, both release lines for the changelog gate, the git hooks,
# and the headless environment the game needs when there is no display, no
# sound card, and no screen reader.
set -euo pipefail

# Local machines already have a working checkout; only the disposable cloud
# container needs provisioning.
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}"

# Headless defaults, matching .github/workflows/ci.yml. Written to
# CLAUDE_ENV_FILE so every command in the session inherits them, not just
# this script.
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
  cat >> "$CLAUDE_ENV_FILE" << 'ENV'
export SDL_VIDEODRIVER=dummy
export SDL_AUDIODRIVER=dummy
export FREIGHT_FATE_NO_SPEECH=1
ENV
fi

# rust-toolchain.toml pins the compiler; this installs it when missing.
rustup show > /dev/null || echo "warning: rustup could not install the pinned toolchain." >&2

# The release-note gate resolves its base by measuring the branch point
# against origin/dev and origin/main, and a cloud container clones a single
# branch -- so on a session started from any other branch the gate cannot
# see one or both release lines, and silently scores the current branch
# against whichever it does have.
#
# The refspecs are spelled out because a --single-branch clone configures
# remote.origin.fetch for that branch alone: `git fetch origin main` there
# lands in FETCH_HEAD and never creates refs/remotes/origin/main. Fetched
# one at a time so a release line that does not exist yet costs only its
# own warning, and non-fatal so a container without network still gets a
# usable checkout.
for release_line in dev main; do
  git fetch --quiet origin \
    "+refs/heads/${release_line}:refs/remotes/origin/${release_line}" || \
    echo "warning: could not fetch ${release_line}; the release-note gate may pick the wrong base." >&2
done

# cargo fmt and the world_data sync check on commit, the release-note gate
# on push.
git config core.hooksPath .githooks
