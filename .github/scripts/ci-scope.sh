#!/usr/bin/env bash
# Decide what a CI run has to cover. Writes two outputs:
#   code=false  the PR only touches docs/ or Markdown: skip builds and tests
#   full=true   desktop E2E and the macOS bundle run too
#
# Env: EVENT (github.event_name), REPO, PR (number), GH_TOKEN.
set -euo pipefail

code=true
if [ "$EVENT" = pull_request ]; then
  # Any failure to list the files means running everything.
  if files=$(gh api --paginate "repos/$REPO/pulls/$PR/files" --jq '.[].filename'); then
    count=$(printf '%s\n' "$files" | grep -c . || true)
    # The files API stops at 3000 entries; past that, assume code changed.
    if [ "$count" -gt 0 ] && [ "$count" -lt 3000 ] \
      && ! printf '%s\n' "$files" | grep -qvE '^docs/|\.md$'; then
      code=false
    fi
  else
    echo "::warning::Could not list the PR's files; running every job"
  fi
fi

full=$code

echo "code=$code full=$full"
echo "code=$code" >> "${GITHUB_OUTPUT:-/dev/null}"
echo "full=$full" >> "${GITHUB_OUTPUT:-/dev/null}"
