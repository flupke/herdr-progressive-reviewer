# Logo

The mark is a check above the meter. The check is the rail's done mark, and the bar under it is
the masthead's meter, green up to the share reviewed. Every colour comes from the Explore page's
tokens (`crates/review-explore-page/assets/tokens.css`):

| Part | Dark | Light | Token |
| --- | --- | --- | --- |
| Tile | `#1b1d21` | `#f5f7f9` + `#d2d4d7` edge | `--panel` (+ `--line`) |
| Check, meter fill | `#3fb950` | `#1a7f37` | `--good` |
| Meter track | `#3a3d42` | `#d2d4d7` | `--line` on `--panel` |
| Wordmark | `#e8e8e8` | `#1f2328` | `--ink` |

The wordmark is "Progressive reviewer" at 700, like the masthead's product name, outlined from
Noto Sans, so no font is needed.

## Files

| File | Use |
| --- | --- |
| `logo-horizontal-dark.svg`, `logo-horizontal-light.svg` | Mark and wordmark: the top of the project's README, by the reader's colour scheme |
| `logo-mark-dark.svg`, `logo-mark-light.svg` | The mark on its own, 48px and up |

The Explore page serves its own icons from `crates/review-explore-page/assets/`:

| File | Use |
| --- | --- |
| `favicon.svg` | Tab icon, a 16px cut that follows the browser's colour scheme |
| `favicon.ico` | Fallback, 16 / 32 / 48 (dark), also at `/favicon.ico` |
| `apple-touch-icon.png` | 180×180, full bleed (iOS rounds the corners) |
| `icon-192.png`, `icon-512.png`, `site.webmanifest` | A phone's home screen |

The 16px cut has a thicker bar than the large mark (6 units of 32, so 3px in a tab), so that its
quarters can be told apart.

## Favicon states

`assets/client/favicon.js` draws the tab's icon from the page's view. At rest, the bar shows the
share of changed lines reviewed, in quarters: full only once every line is reviewed, and at least
a quarter once any line is. While the agent works, as the tab's title says "Agent working…", the
bar shows the page's blue runner instead: four frames, 400 ms each, timed from a worker
(`favicon-ticker.js`) so that a hidden tab keeps it moving. Under `prefers-reduced-motion` it
shows one still frame. Without a round, or a change to measure, the tab shows the plain mark.

The states are `data:` URLs, which the page's Content-Security-Policy allows as images, and the
worker is a file of the page's own (`worker-src 'self'`).
