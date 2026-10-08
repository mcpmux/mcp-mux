#!/usr/bin/env bash
# The `ci-ok` gate. GitHub counts a skipped check as passing, so the gate
# can't trust skips: it reads every job's result (NEEDS = toJSON(needs)) and
# the `changes` job's scope, and fails unless each job passed or was skipped
# because this run didn't need it.
set -euo pipefail

result() { jq -r --arg j "$1" '.[$j].result // "missing"' <<<"$NEEDS"; }
output() { jq -r --arg o "$1" '.changes.outputs[$o] // ""' <<<"$NEEDS"; }

jq -r 'to_entries[] | "  \(.key): \(.value.result)"' <<<"$NEEDS"

required=(changes)
if [ "$(result changes)" = success ]; then
  code=$(output code)
  full=$(output full)
  echo "scope: code=$code full=$full"
  # Anything but true/false means the scope is unknown: require every job.
  [[ "$code" =~ ^(true|false)$ ]] || code=true
  [[ "$full" =~ ^(true|false)$ ]] || full=true
  if [ "$code" = true ]; then
    required+=(rust-check ts-check rust-test test-report)
  fi
  if [ "$full" = true ]; then
    required+=(build e2e-desktop)
  fi
fi

problems=()
while read -r job; do
  [ -n "$job" ] && problems+=("$job: $(result "$job")")
done < <(jq -r 'to_entries[] | select(.value.result == "failure" or .value.result == "cancelled") | .key' <<<"$NEEDS")

for job in "${required[@]}"; do
  r=$(result "$job")
  case "$r" in
    success | failure | cancelled) ;;
    *) problems+=("$job: $r, but this run needs it") ;;
  esac
done

if [ ${#problems[@]} -gt 0 ]; then
  for p in "${problems[@]}"; do echo "::error::$p"; done
  exit 1
fi
echo "All required jobs passed."
