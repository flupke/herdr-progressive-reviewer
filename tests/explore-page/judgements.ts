// Fails when a script of the Explore page's tests makes a judgement with no `// judgement:`
// reason above it (.agents/wiki/explore-page-e2e.md). It reads each line alone: a call split
// over lines, or a destructured `agent`, escapes it.
//
// Usage: node judgements.ts [file or directory...], every script under tests/explore-page by
// default.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

type JudgementMethod = 'assert' | 'waitFor' | 'extract';

/** A judgement with no reason above it. */
export interface UnexplainedJudgement {
  file: string;
  /** One-based. */
  line: number;
  method: JudgementMethod;
}

const CALL = /\bagent\s*\??\.\s*(assert|waitFor|extract)\s*\(/g;
const COMMENT = /^\s*(\/\/|\/\*|\*)/;
const LINE_COMMENT = /^\s*\/\//;
const REASON = /^\s*\/\/\s*judgement:\s*\S/;
const SCRIPT = /\.(ts|mts|js|mjs)$/;
const SKIPPED_DIRECTORIES = new Set(['node_modules', '.e2e']);

/** The judgements of one script that no `// judgement:` comment explains. */
export function unexplainedJudgements(file: string, source: string): UnexplainedJudgement[] {
  const lines = source.split('\n');
  return lines.flatMap((text, index) =>
    COMMENT.test(text) || explained(lines, index)
      ? []
      : [...text.matchAll(CALL)].map((match) => ({ file, line: index + 1, method: match[1] as JudgementMethod })),
  );
}

/** Whether the line comments right above a line hold a `// judgement:` reason. */
function explained(lines: string[], index: number): boolean {
  for (let above = index - 1; above >= 0 && LINE_COMMENT.test(lines[above]); above--) {
    if (REASON.test(lines[above])) return true;
  }
  return false;
}

/** The scripts under a path, outside the installed packages and e2e's output. */
function scripts(path: string): string[] {
  if (!statSync(path).isDirectory()) return [path];
  return readdirSync(path, { withFileTypes: true }).flatMap((entry) => {
    const child = join(path, entry.name);
    if (entry.isDirectory()) return SKIPPED_DIRECTORIES.has(entry.name) ? [] : scripts(child);
    return SCRIPT.test(entry.name) ? [child] : [];
  });
}

function main(paths: string[]) {
  const found = paths
    .flatMap(scripts)
    .flatMap((file) => unexplainedJudgements(relative(process.cwd(), file), readFileSync(file, 'utf8')));
  for (const { file, line, method } of found) {
    console.error(
      `${file}:${line}: agent.${method} is a judgement, which calls a model on every run. Check the ` +
        'outcome with expect on a locator; if no locator can reach the fact, say why on the line ' +
        'above, in a comment that starts with "// judgement:" (.agents/wiki/explore-page-e2e.md).',
    );
  }
  if (found.length > 0) process.exit(1);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  main(args.length > 0 ? args : [import.meta.dirname]);
}
