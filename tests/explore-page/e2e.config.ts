// e2e tests of the Explore page, run by `make e2e-explore` inside `make check`. Each target
// starts its own standalone page server (crates/review-explore-page-server) on a free port.
import { resolve } from 'node:path';
import type { E2EConfig } from 'e2e';
import { web } from '@e2e-dev/web';
import { models } from './model.ts';
import { nixChromium } from './nix-chromium.ts';

// Cargo's target directory, as `make e2e-explore` builds the server into it.
const cargoTarget = resolve(import.meta.dirname, '../..', process.env.CARGO_TARGET_DIR ?? 'target');

// Port 0: the runner picks a free port for each target, and starts one server for it.
const page = (target: string) => ({
  url: 'http://127.0.0.1:0',
  command: {
    executable: `${cargoTarget}/debug/explore-page-server`,
    // The server's own round, which shows the fixed question, opens at `/?token=e2e`; it is for
    // looking at the page through e2e's MCP server (mcp-cli.mjs). Tests open rounds of their own.
    args: ['--port', '{port}', '--token', 'e2e'],
    log: `.e2e/logs/${target}.log`,
    startupTimeout: 10_000,
  },
  readyUrl: 'http://127.0.0.1:{port}/health',
});

// The dev shell sets E2E_CHROMIUM; without it, Playwright's own download is used.
const browser = process.env.E2E_CHROMIUM ? nixChromium(process.env.E2E_CHROMIUM) : 'chromium';

export default {
  targets: [
    { name: 'desktop', engine: web({ browser }), app: page('desktop') },
    { name: 'phone', engine: web({ browser, viewport: { width: 390, height: 844 } }), app: page('phone') },
  ],
  agents: { default: models },
  retries: 0,
  // A failed test keeps a Playwright trace, beside the page's accessibility tree at the failure.
  trace: 'retain-on-failure',
  workers: 2,
  timeout: 30_000,
} satisfies E2EConfig;
