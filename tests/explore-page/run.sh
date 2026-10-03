#!/usr/bin/env bash
# Runs the e2e tests of the Explore page, then fails when the page broke its content security
# policy, and prints the end of the servers' logs when the run failed.
#
# Steps with a valid recording under .e2e/cache replay without a model; a new step, or one whose
# replay no longer matches the page, goes to the acting model that model.ts names, and the cache
# is updated. Judgments (agent.assert) call model.ts's judge on every run. model.ts picks the route: a stored ChatGPT login wins over the Anthropic key. The
# Anthropic key, passed whenever one is found, comes from ANTHROPIC_API_KEY, or else from the
# ANTHROPIC_API_KEY line of ~/.secrets, and from nothing else in that file.
#
# Arguments go to `e2e run` as they are (docs/development.md, "e2e tests").
{ set +x; } 2>/dev/null
set -uo pipefail
cd "$(dirname "$0")"

if [ -z "${E2E_TELEMETRY_DISABLED:-}" ]; then
  echo 'Run this inside the Nix dev shell, which turns e2e telemetry off: nix develop --command make ...' >&2
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
exit "$status"
