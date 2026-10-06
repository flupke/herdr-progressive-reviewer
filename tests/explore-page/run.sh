#!/usr/bin/env bash
# Runs the e2e tests of the Explore page, then fails when the page broke its content security
# policy or, after a full run, when the replay cache holds recordings the run did not look up,
# and prints the end of the servers' logs when the run failed.
#
# Steps with a valid recording under .e2e/cache replay without a model; a new step, or one whose
# replay no longer matches the page, goes to the model that model.ts names, and the cache
# is updated. The tests make no judgement (agent.assert, agent.waitFor, agent.extract), which
# would call a model on every run.
# model.ts picks the route: a stored ChatGPT login wins over the Anthropic key. The
# Anthropic key, passed whenever one is found, comes from ANTHROPIC_API_KEY, or else from the
# ANTHROPIC_API_KEY line of ~/.secrets, and from nothing else in that file.
#
# Arguments go to `e2e run` as they are (.agents/wiki/explore-page-e2e.md).
{ set +x; } 2>/dev/null
set -uo pipefail
cd "$(dirname "$0")"

if [ -z "${E2E_TELEMETRY_DISABLED:-}" ]; then
  echo 'Run this inside the Nix dev shell, which turns e2e telemetry off: nix develop --command make ..., or scripts/dev-shell make ... in a jj workspace' >&2
  exit 2
fi

# Prints the value of the last ANTHROPIC_API_KEY line of ~/.secrets (`NAME=value` or
# `export NAME=value`, optionally quoted), and nothing when there is none.
secrets_key() {
  local line value found=''
  [ -r "$HOME/.secrets" ] || return 0
  while IFS= read -r line || [ -n "$line" ]; do
    line="${line%$'\r'}"
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line#export }"
    case "$line" in
      ANTHROPIC_API_KEY=*)
        value="${line#ANTHROPIC_API_KEY=}"
        case "$value" in
          \"*) value="${value#\"}"; value="${value%%\"*}" ;;
          \'*) value="${value#\'}"; value="${value%%\'*}" ;;
          *) value="${value%%[[:space:]]*}" ;;
        esac
        found="$value"
        ;;
    esac
  done < "$HOME/.secrets"
  printf '%s' "$found"
}

rm -f .e2e/logs/*.log
# A full run notes the cache entries it looks up (cache-lookups.ts); a partial run does not.
lookups=.e2e/cache-lookups.txt
rm -f "$lookups"
if [ "$#" -eq 0 ]; then
  mkdir -p .e2e
  : > "$lookups"
  export E2E_CACHE_LOOKUPS="$PWD/$lookups"
fi
key="${ANTHROPIC_API_KEY:-$(secrets_key)}"
if [ -n "$key" ]; then
  ANTHROPIC_API_KEY="$key" node_modules/.bin/e2e run "$@"
else
  node_modules/.bin/e2e run "$@"
fi
status=$?
if grep -h 'csp violation' .e2e/logs/*.log 2>/dev/null; then
  echo 'The Explore page broke its content security policy'
  status=1
fi
if [ "$status" -ne 0 ]; then tail -n 20 .e2e/logs/*.log 2>/dev/null; fi

# After a run of every test that passed, every recording has been looked up: one that was not
# belongs to a test that was renamed, changed or removed, and no run replays it. The report says
# whether every test ran: a `.only` or a skip leaves some out.
recordings() {
  local file
  for file in .e2e/cache/*.json; do
    if [ -e "$file" ]; then basename "$file" .json; fi
  done
}
every_test_ran() {
  jq -e '.run.summary | .executed == .discovered' .e2e/report.json > /dev/null
}
if [ "$status" -eq 0 ] && [ "$#" -eq 0 ] && every_test_ran; then
  if [ ! -s "$lookups" ]; then
    echo 'The run looked up no recording: cache-lookups.ts did not replace the replay cache' >&2
    exit 1
  fi
  orphans="$(comm -23 <(recordings | sort) <(sort -u "$lookups"))"
  if [ -n "$orphans" ]; then
    {
      echo 'No test looked up these recordings in a full run: the tests that made them were renamed,'
      echo 'changed or removed. Delete them, and commit the removal:'
      # One line per orphan: the keys are hex digests, split on the newlines between them.
      # shellcheck disable=SC2086
      printf '  rm tests/explore-page/.e2e/cache/%s.json\n' $orphans
    } >&2
    status=1
  fi
fi
exit "$status"
