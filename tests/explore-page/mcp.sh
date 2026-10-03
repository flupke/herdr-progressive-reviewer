#!/usr/bin/env bash
# Starts e2e's MCP server for the Explore page tests, over stdio, from any directory: with this
# project's e2e and config, inside the repository's Nix dev shell (which provides the browser and
# turns e2e's telemetry off). Register it with `claude mcp add e2e -- <path to this file>`.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# The dev shell sets E2E_TELEMETRY_DISABLED on every platform.
if [ -z "${E2E_TELEMETRY_DISABLED:-}" ]; then
  exec nix develop "$here/../.." --command "$here/mcp.sh" "$@"
fi
if [ ! -x "$here/node_modules/.bin/e2e" ]; then
  echo 'e2e is not installed: run `nix develop --command make e2e-explore-deps` once.' >&2
  exit 1
fi
exec "$here/node_modules/.bin/e2e" mcp --config "$here/e2e.config.ts" --headless "$@"
