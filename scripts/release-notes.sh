#!/usr/bin/env bash
# Prints the CHANGELOG.md body for one released version — everything between
# its "## [<version>] - <date>" heading and the next "## [" heading, with the
# heading itself and surrounding blank lines stripped. Used by
# .github/workflows/release.yml as the --notes-file input to
# `gh release create` (the release title already carries the version, so the
# notes body does not repeat it).
#
# Usage: scripts/release-notes.sh <version>
# Example: scripts/release-notes.sh 0.8.0
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <version>" >&2
    exit 1
fi

version="$1"
# Resolve relative to this script, not the caller's cwd (issue #14 /
# CONTRIBUTING.md script-path convention: "$0"-relative, no assumed cwd).
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
changelog="$root/CHANGELOG.md"
heading="## [$version]"

if [ ! -f "$changelog" ]; then
    echo "error: $changelog not found" >&2
    exit 1
fi

section="$(awk -v want="$heading" '
  index($0, want) == 1 { found = 1; next }
  found && index($0, "## [") == 1 { exit }
  found { print }
' "$changelog")"

if [ -z "$section" ]; then
    echo "error: no \"$heading\" section in $changelog" >&2
    exit 1
fi

# Trim leading/trailing blank lines so the notes file starts and ends on
# real content.
printf '%s\n' "$section" | sed -e '/./,$!d' | tac | sed -e '/./,$!d' | tac
