// The standalone page server (crates/review-explore-page-server), as Cargo builds it into its
// target directory: the e2e tests and the screenshot gallery both start it.
import { resolve } from 'node:path';

const cargoTarget = resolve(import.meta.dirname, '../..', process.env.CARGO_TARGET_DIR ?? 'target');

export const SERVER = `${cargoTarget}/debug/explore-page-server`;
