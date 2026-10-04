// The replay cache of a full run of the e2e tests, which notes the entries the run looks up, so
// run.sh can name the recordings that no test replays any more (docs/development.md, "Agent
// steps and the model"). e2e reports no cache keys, so this replaces its file store with the
// same store, wrapped to append the key of every lookup to the file E2E_CACHE_LOOKUPS names.
// Without that variable the config keeps e2e's own store, which `npx e2e cache` needs.
import { appendFileSync } from 'node:fs';
import { join } from 'node:path';
import type { E2EConfig } from 'e2e';
// e2e does not export its file store: this is the one `e2e run` builds (dist/cache/context.js).
// Check both again when e2e moves to another version.
import { FileCacheStore, MAX_CACHE_WIRE_BYTES } from './node_modules/e2e/dist/cache/store.js';

/** e2e's file store, which appends the key of each lookup to a file. */
class LookupNotingStore extends FileCacheStore {
  private readonly lookups: string;

  constructor(lookups: string, writable: boolean) {
    super({ directory: join(import.meta.dirname, '.e2e/cache'), maxBytes: MAX_CACHE_WIRE_BYTES, writable });
    this.lookups = lookups;
  }

  override read(keyHash: string) {
    appendFileSync(this.lookups, `${keyHash}\n`);
    return super.read(keyHash);
  }
}

/** The cache options of the config: a store that notes its lookups when run.sh asks for one. */
export function cacheLookups(): Pick<E2EConfig, 'cache'> {
  const lookups = process.env.E2E_CACHE_LOOKUPS;
  if (!lookups) return {};
  // As e2e builds its own store, CI keeps the cache read-only.
  const ci = process.env.CI?.trim().toLowerCase();
  return { cache: { store: new LookupNotingStore(lookups, !ci || ci === '0' || ci === 'false') } };
}
