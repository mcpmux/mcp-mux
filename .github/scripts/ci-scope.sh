#!/usr/bin/env bash
# Decide what a CI run has to cover. Writes two outputs:
#   code=false  the PR only touches docs/ or Markdown: skip builds and tests
#   full=true   desktop E2E and the macOS bundle run too: always on main and
#               in the merge queue, on a PR only with the full-ci label
#
# Env: EVENT (github.event_name), REPO, PR (number), GH_TOKEN,
#      FULL_CI_LABEL (true when the PR has the full-ci label).
set -euo pipefail

code=true
if [ "$EVENT" = pull_request ]; then
  # If the files can't be listed, treat the PR as a code change.
  if files=$(gh api --paginate "repos/$REPO/pulls/$PR/files" --jq '.[].filename'); then
    count=$(printf '%s\n' "$files" | grep -c . || true)
    # The files API stops at 3000 entries; past that, assume code changed.
    if [ "$count" -gt 0 ] && [ "$count" -lt 3000 ] \
      && ! printf '%s\n' "$files" | grep -qvE '^docs/|\.md$'; then
      code=false
    fi
  else
    echo "::warning::Could not list the PR's files; treating it as a code change"
  fi
fi

full=$code
if [ "$EVENT" = pull_request ] && [ "${FULL_CI_LABEL:-false}" != true ]; then
  full=false
fi

echo "code=$code full=$full"
echo "code=$code" >> "${GITHUB_OUTPUT:-/dev/null}"
echo "full=$full" >> "${GITHUB_OUTPUT:-/dev/null}"
