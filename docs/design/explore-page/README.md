# Handoff: Explore page redesign (herdr-progressive-reviewer, PR #85)

## Overview

The Explore page is the web view of an Explore round: an agent explains the design of a change, then asks the reviewer one question at a time, with a quiz and a conclusion the reviewer can implement. The reviewer stares at this page for hours, so the redesign aims at cognitive economy, clarity and calm: one place to read (left), one place to act (right), one meaning per colour, nothing drawn that does not help the current decision.

This bundle documents the **reference set** agreed in the design review: turns 10 (five stages), 12 (phone swipe), 15 (agent chat) and 16 (light theme) of `Explore Variations.dc.html`, plus the live prototypes `HairlineMeter`, `BlindPickPanel`, `SwipeDemo` and `ChatDrawer`. Earlier turns (1–9, 11, 13, 14) are explorations; where they are the only record of a decision (status cards in 13, phone layouts in 11) this README says so.

The page's existing CSS (`crates/review-explore-page/assets/*.css`) and the review document `design-review.md` (findings 1–31) remain the detailed spec for anything not covered here; this redesign keeps their tokens, type base and most components, and changes the frame, the hierarchy and a few behaviours.

## About the design files

The files in this bundle are **design references written in HTML** — prototypes that show the intended look and behaviour. They are not production code to copy. The task is to **recreate these designs in the page's existing environment**: server-rendered Askama templates in `crates/review-explore-page/templates/`, plain CSS in `assets/`, and the small `page.js` / `diagrams.js` scripts, with no framework. Where a prototype uses React-style state (the meter, the blind pick, the swipe, the chat), the behaviour is specified below so it can be written as a few lines of vanilla JS.

## Fidelity

**High-fidelity for layout, type, colour, spacing and copy.** Recreate as drawn. Two exceptions: (1) **diagrams are striped placeholders** — the real page draws them with the vendored Mermaid 11 using the configuration in `design-review.md` finding 30; (2) the **quiz item, the chat messages and the per-file numbers are invented** to fit the mock change; use real data.

## The system in one paragraph

Dark and light through the page's `light-dark()` tokens. System UI at 16px/1.55, a 72ch measure for prose, the page capped at **1440px** (84rem today; widen to 90rem). **Desk and panel**: the reading column is `minmax(0,1fr)`, the action panel is **416px** (480px on the conclusion), column gap 40px, body gutter 32px. The panel is a `--panel` surface, radius 12px, padding 16px, sticky at `top:16px`. Sections have **no borders**: hairlines, space and type weight group things. The masthead is one 57px row: product name, review title, the round rail, a speech-bubble (chat) icon, an overflow `⋯` menu; its bottom hairline **is the progress meter**. Colour: blue = you act here or this is selected; green = done, marked, correct; amber = check before you act; red = failed or destructive; purple = the agent's judgement (its recommendation, the conclusion callout). Grey tints at 6–12% only.

## Screens

Pixel values are at 1440px wide. Every screen shares the masthead (below) and, from Q1 on, the desk-and-panel grid.

### Masthead (all screens)

- Row: `display:flex; align-items:center; gap:28px; padding:14px 32px 15px; position:relative`. No border; two absolutely positioned lines at the bottom (see Meter).
- Left: **Explore** 15px/700, then the review title 15px `--muted`, one line, ellipsis, with the revision id in 13px mono.
- Rail (13px): done steps `✓ Design ▾`, `✓ Q1` with the check in `--good`; the current step in `--accent` 650 with a 2px underline (`box-shadow: inset 0 -2px 0 accent`, `padding-bottom:1px`); later steps `--muted` at 70% opacity. Steps: Design · Q1…Qn · Quiz (with score once known, "Quiz 1/3") · Conclusion. The current step may carry a state: "Q2 · working".
- **Design ▾** opens the design map in place (popover 384px, `--panel`, 1px line, radius 10, shadow `0 14px 36px rgba(0,0,0,.55)`, padding 12): eyebrow "Design of the change", the thesis 14px/600, the four parts as rows (22px numbered disc, name 650, gist 12.5px muted), footer link "Open the design →" in accent. Each part links to the design screen at that part.
- Chat icon (on a desktop, at the left of the row since the project owner's request; see "Agent chat"): 30×28, a 16×12 rounded speech bubble drawn with a 2px border (radius `6px 6px 6px 1px`), `--muted`; when the chat is open: `accent` on `accent 16%` background. Unread badge: 16px disc, accent, 10px/700 count, at top-right (-4px).
- `⋯` (28×28, muted, 16px): menu 300px with "Copy the round's link", "Open the agent's conversation", a hairline, "Reset this round… · closes it for good". Choosing Reset replaces the menu with the hint and the red **Confirm reset**. Reset lives nowhere else.
- Tab title by stage: "Q3 · your turn — <review>", "Agent working… — <review>", "Retry needed — <review>", "Conclusion — <review>".
- Tab icon: the logo's mark (`docs/logo/`), whose bar is the meter in small: the share of changed lines reviewed, in quarters, full only once every line is; while the agent works, the blue runner, 4 frames of 400ms, still under reduced motion. Without a round, the plain mark.

### Meter: lines covered by the exploration (masthead hairline)

At rest: a 1px hairline (`line`) across the masthead bottom, and over it a **2px** bar in `--good` from the left, width = marked lines / changed lines. Nothing else is drawn, no number.

Hover (activation area: an 8px-tall strip, `bottom:-3px`, the full width — same size as the grown line, so the target never moves):
- The bar grows to **8px** (`bottom:-3px`), 180ms ease, and splits into segments: your answers in `--good`; Jev + not-relevant in `--good` at 40%; what the current question will mark in a hatched accent (`repeating-linear-gradient(135deg, accent 0 5px, accent 35% 5px 10px)`); the rest a `white 8%` track. 1px canvas-coloured separators between segments.
- A floating window opens under the pointer 120ms later (220ms fade + 6px rise), 520px, `--panel`, 1px line, radius 10, shadow, padding 14: "**39% reviewed** · 52 of 135 changed lines · `+125 −10` · 4 files", a legend with counts (your answers · Jev and not relevant · this question marks when you answer · left), a hairline, then one row per file: path in mono, a bar scaled to the largest file (6px, same segments), "N left", and "cited here" in accent for files the current question cites. Stays while the pointer is on it; leave = 180ms grace; click pins it.
- On answer sent: the green edge advances to the new share over **800ms** `cubic-bezier(.2,.8,.2,1)`; the hatched segment fades out (500ms); a one-shot glow (`box-shadow: 0 0 16px 3px good 90%`, opacity 0→1→0 over 1.2s) on the done bar; the rail ticks the question; the window's numbers update at once.
- Prototype: `HairlineMeter.dc.html` (turn 9a).

> **Departure from this handoff, at the project owner's request (2026-10):** the window is no longer a fixed 520px. On a change of 43 files under one deep directory, 520px cut every path after about 24 characters, so more than ten rows read the same and no file name showed. The window now sizes to its content: **520px at least**, as wide as its longest path needs, **880px at most**, and never past the viewport's gutters or over the panel beside the reading column; it keeps the width it opened with while it stays open, so numbers that change after an answer do not move its edges. The path column takes the room the longest path needs, the bar column is a fixed 160px, and "N left · cited here" sizes to its content against the right edge. A path that still does not fit loses its **start** to an ellipsis ("…ery/batching/replies_sent_after_the_pane_closed_join_the_last_batch.rs"), so the directory gives way before the file name, which is cut only when it alone does not fit, as on a phone; each row is named by its full path, which is also its title. Past the viewport's height the list of files scrolls inside the window under the totals and the legend; the rows keep the review's order of the files, by path, as the file list shows them. The phone window is unchanged but for the same cut.

### 1. Design screen — turn 10a (phone 11a)

The round's first screen, and the screen "Design ▾" opens later. Nothing but the design.

- Rail: Design current, Q1 next (ink, no check).
- Reading column: eyebrow "DESIGN OF THE CHANGE · READ BEFORE QUESTION 1", the change's **thesis** as h2 (22px/1.35, 650, ≤72ch), meta "4 parts · about 4 minutes · 4 files" (14px muted). Then four parts, 44px apart. Part header: 26px numbered disc (1px `line-strong` border; filled accent when in view), the part name as an eyebrow, "1 / 4" right-aligned 12px muted. Thesis 18px/1.45 600. Then the figure, then the table, then prose.
- Figures: `--panel`, radius 8, padding 12, 1px line in the light theme (none needed in dark), max-width 820; legend row 13px muted with 11px swatches (new = good, changed = warn, unchanged = line-strong). Scroll sideways when wider than the column (finding 30).
- Tables: `border-collapse`, 15px, `th` 600 with a 1px `line-strong` bottom border, `td` with a 1px `line` bottom border, padding 8px 10px, **no vertical borders**; status cells tinted at 10–12% with the 17px round mark (✓ good, ! warn, ✗ bad; 11px/700 white).
- Panel (416, sticky): eyebrow "DESIGN MAP"; the four parts as rows (24px disc, name 15px/650, gist 13px muted; the part in view gets `accent 12%` background and a filled disc; parts read get a green disc). A hairline, then eyebrow "THEN · QUESTION 1" + door chip, the question text 15px/600, "Its choices and evidence open on the next screen." 13px muted, and the primary **Go to question N →** (full width, 44px). N is always the current question: "Go to question 2" when the design is reopened from Q2.
- Scroll-spy: a 15-line IntersectionObserver marks the part in view; links still work without it.

### 2. Question screen — turn 10b (phone 11b; after Send: 16a)

- Above the question, the **previous turn** as one hairlined block (no border, no fill): two columns `1fr 1.3fr`, 15px. Left: eyebrow "YOU ANSWERED Q1", the choice 600, the comment italic muted. Right: eyebrow "THE AGENT RECORDED", the recap with the recorded choice in 600, then the reply, muted. Under both: "Follow-ups · … · …" 13px muted and a quiet underlined "Cancel this answer…" (opens the hint + a secondary "Confirm: cancel my answer"). On phones each part clamps to two lines.
- Question head: eyebrow row "QUESTION 2" + chips (12px/650, 1px border in the chip's colour, radius 999: "One-way door" warn, "Two-way door" good, "Mixed door"/"Door unknown" neutral, "Blind pick" purple); the **question text as h2** 22px/1.35 650 ≤72ch.
- Context prose at 16px, the comparison table (as above, ≤820px), then **Door** and **Blast radius** as hairlined summary rows: `▸` 12px muted, the label 650, the lead muted on one line with ellipsis; open, the lead wraps and the body follows.
- Citations: eyebrow "CITATIONS", the note, the code frame (`--panel`, radius 8, no border; a head row `path` 13px mono 600 + "new 20–27" muted, 1px line under it; rows 14px/1.6 mono; two number columns muted; added rows `good 9%` with a 3px `good` gutter bar on the sign cell, removed rows `bad 9%` with a `bad` bar; when every row is added, drop the row tint and keep the gutter). Then "▸ 1 more citation" 14px muted.
- **Panel** (416, sticky, `--panel`, radius 12, padding 16, gap 14):
  - Before Send on a blind question: a 14px muted line "One-way door: the agent's recommendation shows when you send, so your first read of the choices is your own. You can still change your pick after."
  - Eyebrow "CHOICES"; choice cards: `--canvas` surface, 1px `line`, radius 8, padding 10px 12px, 18px radio aligned to the first line (`margin-top:3px`, `accent-color`); **selected**: 1px accent border + `inset 0 0 0 1px accent` + `accent 10%` background. Hover: `line-strong` border. Focus-visible: 2px accent outline, offset 2.
  - After Send (blind) or at once (two-way): the recommended card gets a tag row (padding-left 28px) with **◆ Agent recommends** (12px/650, purple on purple 16%, radius 999) and the reason under a dashed hairline, 14px. A **reveal line** at the top of the panel: "**The agent recommends another choice.** Read its reason, then keep yours or change it." on purple 14%, or "**You and the agent picked the same choice.**" on good 14%. Not warn or red.
  - Eyebrow "COMMENT · optional" (the "optional" in normal weight), textarea on `--canvas`, 1px line, radius 8, `field-sizing: content`, min 4.5lh, max 14lh.
  - **Gain line** (replaces "Will mark … when you answer"): a `--canvas` block, radius 8, padding 12: "▸ Answering marks **12 lines reviewed** · 3 not relevant" with "39% → **50%**" in 13px mono at the right, and a 6px bar (done in good, the gain hatched accent). The `▸` expands the list of lines.
  - **Send answer**: full width, 44px, `--accent-fill`, white 16px/600, radius 8. Dimmed (35% fill, 60% text) until a choice is picked.
- **Blind pick behaviour (decided, turn 10b):** there is no Pick button. The first Send reveals the recommendation (reveal line + tag + reason) without sending; the button becomes **Confirm answer**; the reviewer may change the pick, then confirms. Redirect after confirm lands on `#answer` (finding 12). Prototype: `BlindPickPanel.dc.html`.

### 3. Waiting — turn 10c (based on 4a)

- Rail: "Q2 · working" current.
- Reading column: a **progress status card** (see Status cards) first: glyph "…", title "The agent is working on your answer to question 2", "Sent 0:42 ago" (tabular numerals), a 3px indeterminate bar (`kind 20%` track, a 30%-wide `kind` runner sliding 1.6s; static at 50% opacity under `prefers-reduced-motion`), "You can leave this tab; its title changes when the next question is ready." Then the answered question, text in `--muted`, Door/Blast rows, "▸ 2 citations".
- Panel: eyebrow "YOUR ANSWER TO QUESTION 2", the sent choice in a `--canvas` card (choice 600, comment italic muted, a "changed after your first pick" purple tag when it applies), "✓ Marked 12 lines reviewed · 3 not relevant" 14px muted, a secondary **Stop waiting** (full width, 40px, 1px `line-strong`, ink text) with "Your answer stays; Retry sends it again." 13px muted under it, then quiet "Cancel this answer…".

### 4. Quiz — turn 10d (based on 4b)

- Rail: "Quiz 2/3" current.
- Reading column: eyebrow "QUIZ · QUESTION 2 OF 3" + three 7px dots (good = correct, bad = wrong, outline = pending); the item as h2 22px; "The quiz checks what you took from the design; nothing here marks lines." 14px muted. After Check: eyebrow "PROOF", the proof note, the code frame.
- Panel: after Check, a verdict line at the top (bad 10% with a 20px ✗ disc and "**Not quite.** …", or good 14% with ✓); eyebrow "ANSWERS"; answer rows as cards (`--canvas`, 1px line): the correct one with a `good` border, an 18px ✓ disc and a **Correct** tag (good on good 16%); a wrong pick with a `bad 60%` border, ✗ disc and **Your pick** tag (accent on accent 16%); others muted. Before Check: plain radio cards and a primary **Check**. After: primary **Next question →** and a centred quiet "Skip the quiz". Land on `#quiz` after Check.

### 5. Conclusion — turn 10e (phone 11c; implementation states 13c)

- Rail: all done, "Quiz 1/3" done, Conclusion current.
- Reading column (≤72ch): eyebrow "THE ROUND IS OVER", h2 "Conclusion" 22px, the lead paragraph 17px, eyebrow "YOUR DECISIONS" and a hairlined list: "Q1" 13px/700 muted in a 2.4rem column, the question 14px muted, the kept choice 600 with a tag — "as recommended" (purple), "changed after your first pick" (accent) — or none; then **Limitations.** and **Remaining uncertainty.** as run-in 650 labels, eyebrow "FUTURE WORK" and its text.
- Panel (**480px**): a score banner (warn 12% below two-thirds, good above): "**Quiz 1 of 3** You missed what a crash loses. See the answers" (14px, link to the results); eyebrow "TO BE IMPLEMENTED · 10 ITEMS"; the list textarea on `--canvas`, 15px/1.45, `field-sizing: content; min-height: 16lh` (server sets `rows` = lines + 1 as fallback) so every item shows; "Implement asks the agent to implement this list, and nothing else." 14px muted; primary **Implement 10 items** (count the non-empty lines); "▸ Not ready? Reply to the agent instead" — Reply opens the chat panel (below).
- Implementation states (turn 13c), each with one primary action: **paused** — info card "The request was never sent", eyebrow "SAVED REQUEST · 10 ITEMS", the list rendered read-only (`--canvas`, radius 8), primary **Send the saved request**, "▸ Edit before sending" reveals the textarea and a secondary "Send a new request"; **unknown** — warn card "The agent may or may not have the request … **Check the agent's conversation first.**", the list muted, only a secondary **Send a new request anyway**; **sent** — ok card "The agent received the implementation request · Sent at 14:36", a secondary **Start a new round…** (the Reset action, confirmed), "▸ Reply to the agent".

### Start cover and status cards — turn 13 (light: 16b)

- Start cover: masthead without rail; 48px below, eyebrow "NO ROUND IS RUNNING", the review title as h1 26px/1.25 650, a muted meta line (revision · repository · `+125 −10` in 4 files · "Jev will mark 18 lines first"), then two equal columns (≥14rem each): primary **Start** with "The agent explains the design, then asks one question at a time."; secondary **Start with Challenger** with "A second agent with fresh context proposes questions too. Turns take longer." When nothing is left to review, the ok card "Every changed line is reviewed" replaces the eyebrow and both buttons are disabled.
- **Status card** (one component, five kinds): `display:grid; grid-template-columns:auto 1fr; gap:4px 12px; padding:14px 16px; border-radius:10px; border:1px solid kind 45%; background: kind 8%; max-width: 72ch + 34px`. Glyph: 26px disc in the kind colour, canvas-coloured mark 14px/800 (… i ! ✕ ✓). Title 650; reason 15px muted (verbatim errors in mono `code`); next step 15px ink (bold the imperative when it matters: "**Check the agent's conversation before you retry.**"); action row `gap:14px`, primary first, hint 13px muted inline. Kinds: progress and info = accent; warn; danger; ok. Mapping of states to kinds and copy: `design-review.md` finding 6 and 23.

### Phone (390px) — turn 11, 12, 15b

- Masthead: "Explore" 15px/700, a spacer, two chips (12px/650, radius 999): **✓ Design ▾** (1px `line-strong`) and the current step (accent border and text: "Q2 · 2 of 3", "Design · part 1 of 4", "Conclusion"), the chat bubble, `⋯`. The review title moves into `⋯`. The meter line is unchanged.
- Body gutter 16px; no section boxes. The panel goes full-bleed (`margin:0 -16px; border-radius:0`), primary actions 48px tall. Tables and code scroll sideways with the first column sticky (`position:sticky; left:0; background: canvas`). The question eyebrow gains a "Choices ↓" link to `#answer`. Turn strip parts clamp to two lines; follow-ups hidden.
- Design screen: the map is a full-bleed `--panel` block under the thesis; part headers are `position:sticky; top:0` (canvas background, hairline under); flowcharts are redrawn `TD`; a bar pinned at the bottom holds "Current · Question N of M · door" and the primary **Go to question N →**.
- **Swipe between screens** (turn 12a): screens Design · Q1 · Q2 … sit in a horizontal track; `touch-action: pan-y`. Drag threshold **48px** or velocity > 0.5 px/ms; once crossed, the target chip fills (`accent-fill`, white text) and a 3px accent edge lights on the side you are heading to; release before = spring back. Rubber-band (×0.35) at the ends. Snap: 280ms `cubic-bezier(.2,.8,.2,1)`. Frames that scroll sideways (tables, code, diagrams) take the gesture first. Haptic tick at the threshold where available. Prototype: `SwipeDemo.dc.html`.
- Chat on the phone: a bottom sheet to two thirds (560px of 844), draggable to full, radius `16px 16px 0 0`, grabber 36×4, same header, thread and composer; the page behind dims to 45%.

### Agent chat — turn 15 (prototype `ChatDrawer.dc.html`)

One conversation per round, available on every screen; it is the "ask the agent" of the question screens and the "Reply" of the conclusion.

> **Departure from this handoff, at the project owner's request (2026-10):** on a desktop the chat no longer lies over the answer column, which hid the choices or the list to implement exactly when the reviewer discussed them with the agent. It is a column at the **window's left gutter**, 416px wide, drawn as the answer panel is (`--panel` surface in both themes, no border, radius 12, sticky under the masthead, then at the panel's 16px while the page scrolls), its messages scrolling above the composer, and its bubble moves to the **left of the masthead**, above it. From 1424px the page makes room for it: the reading column starts after it and the 40px gap, narrowing down to 448px (384px beside the conclusion's 480px panel), while the panel keeps its width and place; on a window wide enough (2352px) it fits in the margin of the centred page. From 1120px to 1424px it lies over the page's left margin and the left of the reading column as a drawer with a shadow and no scrim, and never covers the panel. The phone sheet and its bubble beside `⋯` are unchanged. `15a-chat.png` shows the chat as first designed.

- Opens from the masthead bubble (and from "Not ready? Reply…"). Desktop, as first designed: a **416px panel over the answer column** — `position:absolute; top:81px (under the masthead + 24px); right:32px; bottom:24px`, `#161719`-ish surface (`--panel` slightly darker in dark; `--panel` in light), 1px line, radius 12, shadow `0 18px 48px rgba(0,0,0,.55)`. Slides in 24px → 0 and fades over 280ms; the reading column is never covered; the choices wait underneath until the chat closes.
- Header: "Agent" 15px/650, "this round · 3 messages" 13px muted, × at the right. Body: messages 15px; yours as eyebrow "YOU · Q2 · 14:21" + text 600, with an optional **quote** (blockquote: 3px accent 60% left rule, accent 6% fill, 13px muted); the agent's in a `--panel` block with eyebrow "AGENT · 14:22", citations as accent links that open the citation in place. Pending reply: an info-styled row "… The agent is answering · 0:31". Composer: optional quote chip (same style, × removes it), textarea "Ask, challenge, or add context…", a secondary-accent **Send** (1px accent border, accent text) with "The question stays open. ⌘↵" 12px muted.
- **Selection → quote**: selecting text in the reading column (question, context, Door, citations, design) shows one popup option, **Add to chat** (13px/650 accent on accent 14%, in a `--panel` pill with shadow). It opens the chat if closed and attaches the passage as the composer's quote. Selected text is highlighted `accent 28%`.
- Mechanics: a chat message is a free-text wakeup that does not answer (today's "None of the above" + text); the question stays open; the agent's reply lands in the chat, not above the question. Messages carry the question they were asked under. Unread reply: badge on the bubble and the tab title.

## Interactions & behaviour (summary)

| Element | Trigger | Behaviour | Timing |
| --- | --- | --- | --- |
| Meter | hover the 8px strip | grow 2→8px, split, window after a grace | 180ms ease; window 220ms, 120ms delay; leave grace 180ms |
| Meter | answer sent | green edge advances, hatch fades, glow, rail ticks | 800ms `cubic-bezier(.2,.8,.2,1)`; glow 1.2s |
| Blind pick | first Send | reveal line + tag + reason; button → Confirm answer | instant |
| Blind pick | Confirm | post `/answer`, redirect `/#answer` | — |
| Choice card | hover / focus | `line-strong` border / 2px accent outline | 150ms |
| Chat | bubble, Reply, Add to chat | panel slides in over the answer column (since the owner's request: column slides in at the window's left, the page makes room) | 280ms `cubic-bezier(.2,.8,.2,1)` |
| Chat | Esc, ×, bubble | closes; unread badge resets | 220ms |
| Design ▾ | click | map popover; parts link into the design screen | — |
| ⋯ | click | menu; Reset → confirmation in place | — |
| Swipe (phone) | drag ≥48px or flick | page turns; chip fills at threshold | snap 280ms |
| Progress bar | working states | indeterminate runner | 1.6s loop; static under reduced motion |
| Tab title | stage change | "Q3 · your turn", "Agent working…", "Retry needed", "Conclusion" | — |
| Tab icon | marks change; agent works | meter in quarters; runner while working | 4 × 400ms loop; still under reduced motion |

## State

Per page load the server knows: stage (start · design · question n · working · interrupted kinds · quiz item · conclusion · implement states), the round rail (questions answered, quiz score), the design (thesis, four parts with thesis/body/figures), the current question (text, door kind, blind flag, choices, recommendation + reason, citations, pending marks), the previous turn (your choice, comment, the agent's recap, reply, follow-ups), the review marks (per file: changed, marked by whom, pending), and the chat thread. Client-only state: meter hover/pinned; blind pick revealed/confirmed; chat open and composer draft (persist draft in `sessionStorage`); scroll-spy; swipe index; design-map popover and ⋯ menu open.

## Design tokens

```
--canvas       light-dark(#ffffff, #121212)
--panel        light-dark(#f5f7f9, #1b1d21)      chat surface dark: #161719
--ink          light-dark(#1f2328, #e8e8e8)
--muted        light-dark(#59636e, #a3a9b1)
--line         ink 15%   (dark: rgba(255,255,255,.12))
--line-strong  ink 30%   (dark: rgba(255,255,255,.28))
--code-bg      ink 7%
--accent       light-dark(#0969da, #4493f8)      text, borders, selection, links
--accent-fill  light-dark(#0969da, #1f6feb)      primary buttons, white text
--good         light-dark(#1a7f37, #3fb950)      done, marked, correct, ✓
--warn         light-dark(#9a6700, #d29922)      check before you act
--bad          light-dark(#cf222e, #f85149)      failed, wrong, destructive
--agent        light-dark(#8250df, #ab7df8)      the agent's judgement (= --conclusion)
tints          accent 10% selected · reveal purple 14% / good 14% · status fill 8%, border 45% · table cells 10–12%
meter          good 100% (yours) · good 40% (Jev, not relevant) · hatched accent (this question) · white 8% track
code (Catppuccin) Latte on light / Mocha on dark, as in citations.css
```

Type: `system-ui, -apple-system, "Segoe UI", "Noto Sans", sans-serif`; mono `ui-monospace, SFMono-Regular, Menlo, monospace`.
Scale: masthead 15px/700 · headline (question, thesis, conclusion) 22px/1.35/650 (19px on phones) · part thesis 18px/1.45/600 · lead 17px · body 16px/1.55 · small 15px · hints 14px muted · rail/legend 13px · eyebrow 12px/650 uppercase, letter-spacing .06em, muted · tags/chips 12px/650 · code 14px/1.6.
Space: gutter 32px (16px phone) · column gap 40px · panel padding 16px, gap 14px · parts 44px apart · stage blocks 28–32px apart · card padding 10px 12px · choice gap 8px.
Radius: panel 12 · cards, code, figures, buttons, textareas 8 · status card 10 · chips/tags 999 · discs 50%.
Buttons: primary 44px (48px phone) `accent-fill` white 16px/600; secondary 40px, 1px `line-strong`, ink; accent-outline (chat Send, Ask) 1px accent, accent text; quiet = underlined 14px muted text; danger = `bad` fill, only Confirm reset.
Shadows: popovers `0 14px 36px rgba(0,0,0,.55)`; chat `0 18px 48px rgba(0,0,0,.55)` (light: `rgba(0,0,0,.18)`).

## Assets

None: no images or icon fonts. The chat bubble is a CSS shape; status marks and glyphs are text (✓ ✗ ! i … ◆ ▸ ▾ ⋯). Diagrams come from the page's Mermaid pipeline.

## Screenshots

`screenshots/` holds a 1× capture of each reference card, named by its option id. Read them alongside the sections above; they are the visual truth where a measurement here is ambiguous.

- Stages: `10a-design.png`, `10b-question.png`, `10c-waiting.png`, `10d-quiz.png`, `10e-conclusion.png`
- Masthead details: `3b-rail-map-open.png` (Design ▾ popover), `5a-menu.png` (⋯ menu), `8b-meter-hover.png` (meter grown, window open), `9a-meter-live.png`
- Chat: `15a-chat.png` (over the answer column, as first designed; the desktop place now departs from it, see "Agent chat"), `15b-phone-chat.png`
- Phone: `11a-phone-design.png`, `11b-phone-question.png`, `11c-phone-conclusion.png`, `12a-phone-swipe.png`
- States: `13a-start-cover.png`, `13b-status-cards.png`, `13c-implementation.png`
- Light theme: `16a-question-light.png`, `16c-design-light.png`, `16b-status-light.png`

Diagrams appear as striped placeholders in every capture; see Fidelity.

## Files

- `Explore Variations.dc.html` — the whole exploration; the reference set is sections `#t10` (stages), `#t12` (swipe), `#t15` (chat), `#t16` (light); `#t9` (live meter), `#t11` (phones), `#t13` (start cover, status cards, implementation).
- `HairlineMeter.dc.html` — meter at rest / hover / answer sent, with the window.
- `BlindPickPanel.dc.html` — pick → Send reveals → Confirm.
- `SwipeDemo.dc.html` — phone swipe with threshold and chip feedback.
- `ChatDrawer.dc.html` — chat over the answer column, Add-to-chat quote.
- `support.js` — the prototype runtime; not part of the design.
- `design-review.md` — the review this redesign follows (findings 1–31); authoritative for anything not restated here.
