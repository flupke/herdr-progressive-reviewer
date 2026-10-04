// The check of judgements in the Explore page's tests (judgements.ts), run by `make e2e-explore`
// with `node --test`. The samples spell each call from parts, so that this file passes the check.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { unexplainedJudgements } from './judgements.ts';

const call = (method: string) => `  await agent.${method}('the diagram shows two lanes');`;
const lines = (...text: string[]) => text.join('\n');

test('a judgement with no reason above it is reported, with its line', () => {
  const source = lines('test("t", async ({ agent }) => {', call('assert'), call('waitFor'), call('extract'), '});');
  assert.deepEqual(unexplainedJudgements('a.e2e.ts', source), [
    { file: 'a.e2e.ts', line: 2, method: 'assert' },
    { file: 'a.e2e.ts', line: 3, method: 'waitFor' },
    { file: 'a.e2e.ts', line: 4, method: 'extract' },
  ]);
});

test('a judgement under a reason passes, even with more comment lines between', () => {
  const source = lines(
    '  // judgement: the drawn lanes are pixels of a canvas, which no locator reaches',
    '  // and no text of the page names.',
    call('assert'),
  );
  assert.deepEqual(unexplainedJudgements('a.e2e.ts', source), []);
});

test('a marker with no reason, or one above code, explains nothing', () => {
  const empty = lines('  // judgement:', call('assert'));
  const apart = lines('  // judgement: no locator reaches it', '  await page.reload();', call('assert'));
  assert.equal(unexplainedJudgements('a.e2e.ts', empty).length, 1);
  assert.equal(unexplainedJudgements('a.e2e.ts', apart).length, 1);
});

test('a judgement named in a comment, or a locator, is no call', () => {
  const source = lines(
    '// The tests make no judgement (`agent.assert`, `agent.waitFor`, `agent.extract`).',
    ` * ${call('assert')}`,
    "  await screen.getByRole('status').waitFor();",
  );
  assert.deepEqual(unexplainedJudgements('a.e2e.ts', source), []);
});
