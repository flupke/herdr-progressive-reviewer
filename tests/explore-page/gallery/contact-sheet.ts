// The contact sheet of a gallery, `index.html`: each state with its screenshots side by side,
// and, given an earlier gallery to compare with, before and after for each image that differs.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import type { GalleryState } from './states.ts';

/** The themes each state is shot in, in the order of the contact sheet. */
export const THEMES = ['light', 'dark'] as const;
export type Theme = (typeof THEMES)[number];

/** One screenshot of a gallery. */
export interface Shot {
  state: string;
  width: number;
  theme: Theme;
  /** The file's name in the gallery's folder. */
  file: string;
}

/** How a screenshot compares with the one of the same name in the earlier gallery. */
type Change = 'same' | 'changed' | 'new';

const escape = (text: string) =>
  text.replace(/[&<>"]/g, (character) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[character]!);

/** The width a screenshot is shown at on the sheet: a phone's larger, to stay readable. */
const shownWidth = (width: number) => Math.round(width >= 600 ? width * 0.35 : width * 0.6);

/** A screenshot as a link to the full-size file. */
function image(source: string, shot: Shot, label: string): string {
  return `<a href="${escape(source)}"><img src="${escape(source)}" width="${shownWidth(shot.width)}" alt="${escape(label)}" loading="lazy"></a>`;
}

/**
 * The contact sheet of the `shots` of `states`, written to `output`, compared with the gallery
 * in `compare` when given, and a summary of the comparison for the run's output.
 */
export function contactSheet(
  states: GalleryState[],
  shots: Shot[],
  output: string,
  compare: string | undefined,
): { html: string; summary: string } {
  const changeOf = (shot: Shot): Change | undefined => {
    if (!compare) return undefined;
    const before = join(compare, shot.file);
    if (!existsSync(before)) return 'new';
    return readFileSync(before).equals(readFileSync(join(output, shot.file))) ? 'same' : 'changed';
  };
  const counts: Record<Change, number> = { same: 0, changed: 0, new: 0 };
  const sections = states.map((state) => {
    const figures = shots
      .filter((shot) => shot.state === state.name)
      .map((shot) => {
        const change = changeOf(shot);
        if (change) counts[change]++;
        const caption = `${shot.width} px, ${shot.theme}`;
        const after = image(shot.file, shot, `${state.name}, ${caption}`);
        const body =
          change === 'changed'
            ? `<div class="pair"><div><p>Before</p>${image(relative(output, join(compare!, shot.file)), shot, `${state.name}, ${caption}, before`)}</div><div><p>After</p>${after}</div></div>`
            : after;
        const mark = change && change !== 'same' ? ` <span class="mark ${change}">${change}</span>` : '';
        return `<figure${change ? ` class="${change}"` : ''}><figcaption>${caption}${mark}</figcaption>${body}</figure>`;
      });
    return `<section id="${escape(state.name)}"><h2>${escape(state.name)}</h2><p>${escape(state.about)}</p><div class="shots">${figures.join('')}</div></section>`;
  });

  const names = new Set(shots.map((shot) => shot.file));
  const removed = compare
    ? readdirSync(compare).filter((file) => file.endsWith('.png') && !names.has(file))
    : [];
  const summary = compare
    ? `, compared with ${compare}: ${counts.changed} changed, ${counts.new} new, ${removed.length} gone, ${counts.same} the same`
    : '';
  const removedList =
    removed.length > 0
      ? `<section><h2>Gone</h2><p>In the earlier gallery only:</p><ul>${removed.map((file) => `<li>${escape(file)}</li>`).join('')}</ul></section>`
      : '';
  const filter = compare
    ? '<label class="filter"><input type="checkbox" id="changed-only"> Only the images that changed or are new</label>'
    : '';
  const index = states.map((state) => `<a href="#${escape(state.name)}">${escape(state.name)}</a>`).join(' ');

  const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Explore gallery</title>
<style>
:root { color-scheme: light dark; --line: #8884; --changed: #c77700; --new: #2a7ab0; }
body { font: 15px/1.4 system-ui, sans-serif; margin: 16px; background: Canvas; color: CanvasText; }
nav { display: flex; flex-wrap: wrap; gap: 4px 12px; font-size: 13px; }
section { border-top: 1px solid var(--line); margin-top: 24px; padding-top: 8px; }
h2 { font: 600 17px/1.3 ui-monospace, monospace; margin: 0; }
.shots { display: flex; gap: 16px; align-items: flex-start; overflow-x: auto; padding-bottom: 8px; }
figure { margin: 0; flex: none; }
figcaption { font-size: 13px; margin-bottom: 4px; }
img { display: block; height: auto; border: 1px solid var(--line); }
.pair { display: flex; gap: 8px; }
.pair p { margin: 0; font-size: 12px; }
.mark { font-weight: 600; text-transform: uppercase; font-size: 11px; }
.mark.changed { color: var(--changed); }
.mark.new { color: var(--new); }
body.changed-only figure.same { display: none; }
</style>
</head>
<body>
<h1>Explore gallery</h1>
<p>${shots.length} screenshots of ${states.length} states, named <code>&lt;state&gt;-&lt;width&gt;-&lt;theme&gt;.png</code>${escape(summary)}.</p>
${filter}
<nav>${index}</nav>
${sections.join('\n')}
${removedList}
${compare ? "<script>document.getElementById('changed-only').addEventListener('change', (event) => document.body.classList.toggle('changed-only', event.target.checked));</script>" : ''}
</body>
</html>
`;
  return { html, summary };
}
