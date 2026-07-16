#!/usr/bin/env bash
#GABARIT description = "Extract failing test names from a CI log"
#GABARIT created-by = "claude-code"
#GABARIT created-at = 2026-07-16
#GABARIT test = "../fixtures/ci-sample.log --json"
#USAGE arg "<logfile>" help="Path to the CI log to scan"
#USAGE flag "--json" help="Emit one JSON object per line instead of plain names"
set -euo pipefail

# Args arrive both as $usage_<name> env vars (typed, defaults applied) and as
# positional argv. We use the env vars here.
while IFS= read -r line; do
  case "$line" in
    FAIL*|*"... FAILED"*)
      name="${line#FAIL }"
      name="${name%% ...*}"
      if [ "${usage_json:-false}" = "true" ]; then
        printf '{"test":"%s"}\n' "$name"
      else
        printf '%s\n' "$name"
      fi
      ;;
  esac
done < "$usage_logfile"
