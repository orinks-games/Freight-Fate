#!/usr/bin/env bash
# Usage: skip-tag.sh TAG FROM TO
#
# Exits 0 when every commit on the first-parent line of FROM..TO carries TAG
# (for example "[skip ios]") in its message, case-insensitively, and 1
# otherwise. A pull request merged into dev counts as one commit, its merge
# commit, whose message carries the PR title. An empty range, or a FROM that
# is missing or all zeros (a new branch), is judged by TO alone.
set -euo pipefail
tag="$1" from="${2:-}" to="$3"
if [ -n "$from" ] && [ "$from" != "0000000000000000000000000000000000000000" ] \
    && git cat-file -e "$from^{commit}" 2>/dev/null; then
  commits="$(git rev-list --first-parent "$from..$to")"
else
  commits=""
fi
[ -n "$commits" ] || commits="$(git rev-parse "$to")"
for c in $commits; do
  if ! git log -1 --format=%B "$c" | grep -qiF -- "$tag"; then
    echo "$(git log -1 --format='%h %s' "$c") does not carry $tag."
    exit 1
  fi
done
echo "Every commit in the range carries $tag."
