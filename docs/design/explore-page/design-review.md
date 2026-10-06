# Explore page: design review

Reviewed from the 156 screenshots in `gallery/current/` (all 39 states at 1280 dark and 390
light, and the dense ones at 1280 light and 390 dark), with the stylesheets and templates of
`crates/review-explore-page`. File names in brackets are gallery screenshots; `style.css:7`
means line 7 of `crates/review-explore-page/assets/style.css`, and templates are in
`crates/review-explore-page/templates/`.

Mock-ups are in `design-review/`: `design.html` (the round's first screen: the design of the
change, then question 1), `question.html`, `states.html` and `conclusion.html`. They use the
page's own stylesheets copied next to them, plus one `proposal.css` that holds every proposed
rule, with comments naming the finding each block belongs to. Screenshots sit next to them:
each mock-up at 1280 in both themes and at 390 in light; the design and the question also at
390 in dark (`design-1280-dark.png` and so on).

Findings 28 to 31, on the design of the change, were added after a first round of feedback;
they revise findings 1, 8, 14 and 17, as each of those says.

## 1. Verdict

The page is honest, legible and calm. The base is sound: system type at 16px, a 72ch measure,
light/dark through `light-dark()`, status marks in tables, restrained callouts, highlighted diff
citations, a sticky answer column, plain forms that work without a framework. What it lacks is
a hierarchy that matches the reviewer's job. On almost every screen, the most prominent things
are a generic "Explore" heading and stacks of equal bordered boxes. The decision to make, the
agent's recommendation and the next step are visually no louder than the help text around
them.

The five changes that would help most:

1. **Put the current decision in the first screen and make it the headline.** Merge "Your
   last answer" and "Reply from the agent" into one compact strip that shows your answer next
   to the agent's recap of it. Today they are two boxes split by the design block, and on a
   phone they fill the whole first screen. Then make the question text the largest text on the
   page, with "Question 2" as a small label above it.
2. **Make selection, recommendation and first pick look different.** Today a recommended
   choice and a selected choice both get a blue border, and they look almost the same in dark
   mode. The blind-pick reveal is the product's best idea, but it happens below the fold with
   no visible payoff. Give the agent's pick its own tag and colour, put its reason inside its
   card, add a one-line "you agree / the agent recommends another choice" reveal, and scroll to
   it after Pick.
3. **One meaning per colour, one status component, one button hierarchy.** Red now means
   error, "someone else got there first", destructive and wrong answer. Grey means both
   "check before you act" and "success". Every state that is not a question uses the same box.
   Replace that with one status card in five kinds (progress, info, warn, danger, ok), with a
   title, the reason, the next step and its action. Make Cancel answer (which throws away the
   agent's turn in one click) and Reset quiet, confirmed actions.
4. **A frame that orients.** A slim masthead with the review's title and a round rail
   (Design ✓ · Q1 ✓ · Q2 · Quiz · Conclusion), plus a browser tab title that says whose turn it
   is ("Q3 · your turn" / "Agent working…"). The reviewer can then leave the tab while the agent
   works and still notice when the turn comes back. That is the cheapest attention feature
   available.
5. **Make the design of the change a whiteboard, not a wall.** It is what the whiteboard test
   is about, and today it is 2,550px of evenly weighted prose. Give the change and each of its
   four parts a one-sentence thesis, and lead each part with its figure: a component map, a
   numbered sequence diagram, a cost table, a comparison table. On a wide window, the panel
   column holds a map of the four parts and the question they prepare. On a phone, sticky part
   headers say where you are. Once the round moves on, fold the design to one line that recaps
   the change and reopens on a summary (findings 28 to 31, `design-review/design.html`).

**Direction: a reading desk and an answer panel.** The left column is a document you read. It
has no boxes, a clear type scale, lead paragraphs that give a skim path, and figures (tables,
diagrams, code) styled as one family. The right column is a tinted panel that always holds the
one thing to do in this stage, in the same place: choices and Send, the list and Implement.
States that are not a question get a status card in the reading column. Colour carries state.
Accent blue means "you act here". Purple marks the agent's judgement, which already colours the
conclusion callout. This idea fits a tool used for long, attentive reading: it adds no
decoration, makes the existing sticky column the organising principle of every stage, and
gives the eye one fixed place to return to. `design-review/design-1280-light.png` shows it on
the round's first screen, where the panel holds the map of the design;
`design-review/question-1280-light.png` on question 2 after the blind pick.

## 2. Findings, most valuable first

Each finding: **[kind · effort]**, where it shows, what is wrong and why it matters for this
reviewer, and the change.

### 1. The current question starts too far down; the previous turn is split and repeated

**[layout, component · medium]**

Seen in: [question-2-blind-1280-dark], [question-2-blind-picked-1280-dark],
[question-3-diagram-error-1280-dark], [quiz-1280-dark], [conclusion-1280-dark], every
`implement-*`, and above all [question-2-blind-390-light], whose first 844px screen holds "Your
last answer", "Design of the change" and "Reply from the agent", with no question at all (the
question heading starts at y≈884).

Before every stage after question 1, the page stacks three bordered boxes:

- "Your last answer": the choice in bold, a two-line hint and an outlined Cancel answer button,
  about 190px.
- The folded Design box.
- "Reply from the agent": "Recorded: …", follow-ups and the reply, about 250px.

The agent's "Recorded:" line repeats your answer in other words, yet it sits two boxes away
from it. In [question-3-diagram-error-1280-dark], the open design (≈2,550px) separates them
entirely. That recap is exactly what the reviewer should check ("the short recap shows the
agent's interpretation", `.agents/wiki/explore-round.md`). Keeping it apart from the answer makes the check harder,
and the stack pushes the current task down on every screen.

Change:

- Replace `cancel-answer.html` and `response.html` with one `turn.html` strip, placed right
  above the question, quiz or conclusion.
- From 56rem up it has two columns. Left: "You answered question 1", then the choice (600
  weight) and the comment in italics. Right: "The agent recorded", then the recap and the
  reply.
- Follow-ups become small chips under both columns. Cancel answer becomes a quiet link (finding
  5).
- Style: a 3px left rule in `--line-strong`, a 3% ink tint, 14px 16px padding, 0.9375rem text,
  and no border box.
- Below 40rem, clamp each part to two lines (`-webkit-line-clamp: 2`) and hide the follow-ups
  behind the clamp.
- `LatestAnswer` (`review-explore-page/src/round.rs:384`) needs the question's number, and
  ideally its text, so the strip can say "question 1".
- Make the folded Design a one-line row without its own box. *Revised by finding 31:* that
  row shows the change's thesis and reopens on a summary of the four parts; the rail's "Design
  ✓" opens it.

Mock-up: `design-review/question.html`. The phone version puts the question in the first screen
(`question-390-dark.png`).

### 2. The question is not the headline

**[typography · small]**

Seen in: every question state, e.g. [question-1-1280-dark] and [question-3-diagram-error-390-light].
Also the quiz ([quiz-1280-dark]: "Question 1 of 3" is bigger than the quiz question).

`<h2>Question 1</h2>` renders at 1.5em bold. The question itself, the thing to decide, is 16px
at weight 600 (`style.css:8`), 40px further down. The label outranks the decision. On a page
where the reviewer must stay focused, the first thing the eye lands on in a stage should be
what to decide.

Change in `page.html:38-39`:

- Wrap the label and the question in a `header.question-head`.
- The label becomes an eyebrow, "QUESTION 2": 0.75rem, weight 650, letter-spacing 0.06em,
  uppercase, `--muted`. Put the door chip from finding 11 on the same line.
- The question text becomes the `h2`: 1.375rem/1.35, weight 650, 6px under the eyebrow, at
  most `--measure` wide. Use 1.1875rem below 40rem.
- Do the same in `quiz.html:10,16,24`: "QUESTION 1 OF 3" as the eyebrow, and the item as the
  headline.

### 3. Selection, recommendation and first pick look alike; the reveal goes unnoticed

**[component, behaviour · medium]**

Seen in: [question-2-blind-picked-1280-dark] (the picked card and the recommended card both
have a blue border, at 100% and 50% accent, and are hard to tell apart),
[question-1-1280-light], [question-2-answer-cancelled-1280-dark], [question-2-blind-picked-390-light].

- `.choice:has(input:checked)` and `.choice.recommended` (`style.css:12,51`) use the same
  colour for two opposite meanings: "you chose this" and "the agent would choose this". That
  pushes: the recommended card looks pre-selected.
- The reason sits outside its card, as a paragraph between two cards (`choices.html:11`,
  `.recommendation` margin -4px). It reads as if it belonged to the gap.
- Radios use the default 13px size, centred vertically on three-line labels.
- After Pick, the only sign of the reveal is a dim hint at the top of the column ("You picked
  '…' first"). The redirect lands at the top of the page (`page.rs:677`, `to_page` redirects
  to `/`). In the 1280×900 screenshot, the recommended card is at y≈940-1095, below the fold.
  On a phone it is about 2,000px down.

The blind pick exists to let the reviewer test their own judgement, and its payoff (did I see
what the agent saw?) is invisible.

Change:

- **Selected:** a 1px accent border plus a 1px inset accent shadow (2px in all), a background
  of accent 8% over the canvas, and `accent-color` on an 18px radio (`width/height: 1.1em`)
  aligned to the first line (`align-items: flex-start`, radio `margin-top: 0.22em`).
- **Recommended:** a neutral border, plus a pill tag "◆ Agent recommends" in `--conclusion`
  purple (0.75rem, weight 650, purple 14% background). Its reason moves inside the label as a
  `<span class="recommendation">` under a dashed hairline, at 0.875rem. A `p` is not allowed
  inside a `label`; `aria-describedby` keeps working.
- **After the pick:** a pill tag "Your first pick" (accent) on the picked card. Replace the
  hint in `answer.html:12` with a reveal line at the top of the panel:
  - when the first pick equals the recommended choice: "You and the agent picked the same
    choice." on a `--good` 14% tint;
  - otherwise: "**The agent recommends another choice.** Read its reason, then keep your pick
    or change it." on a `--conclusion` 12% tint. Not warn or red: disagreeing is information,
    not an error.
- **Scroll:** redirect a successful `/pick` to `/#answer` and give the answer form
  `id="answer"`, so the reveal lands in view. Strip the fragment with `history.replaceState`
  once the page has loaded, so the polling `location.reload()` does not jump there again.
- Two-way questions keep the agent's order and show the purple tag at once. The recommended
  card no longer looks selected.

Mock-up: `design-review/question.html`, panel on the right.

### 4. Nothing says which review this is or where the round stands; the tab title never changes

**[layout, behaviour · medium; the tab title alone is small]**

Seen in: all states. The 2em "Explore" `h1` (`page.html:15`) is the largest text on every page
and says nothing. The review's title appears only on the start screen, although
`PageContext.review` is available in every stage. Nothing shows how many questions were
answered or what comes after. The `<title>` is always "Explore" (`page.html:6`).

For keeping attention, two things matter. First, a sense of progress: "two decided, this is
the third, then the quiz". Second, being able to switch away while the agent works (30 to 120
seconds) and notice when it is your turn again.

Change:

- Replace the `h1` with `header.masthead`: "Explore" (0.9375rem, weight 700), the review title
  and revision in `--muted` (ellipsis), a round rail, and the quiet "Reset…" from finding 5,
  all in one 12px-padded row with a hairline under it.
- Rail pills: done steps with a green ✓, the current step with an accent border, later steps
  at 60% opacity. From the data at hand: Design ✓ once the design exists; Q1…Qn up to
  `question.number`; Quiz with its score once known; Conclusion.
- Below 40rem, the rail gets its own row and scrolls sideways; better, show only the current
  step there.
- Set `<title>` by stage:
  - a question: "Q3 · your turn — <review title>";
  - working: "Agent working… — <review title>";
  - interrupted: "Retry needed — <review title>";
  - the conclusion: "Conclusion — <review title>".
- Optionally, a favicon dot.

Mock-up: the masthead in all three mock-ups.

### 5. Cancel answer throws away the agent's turn in one click; Reset looks like any button

**[behaviour, component · small]**

Seen in: every state with "Your last answer" ([working-last-answer-1280-dark],
[question-2-blind-390-light], [quiz-1280-dark], [conclusion-1280-dark], all `implement-*`), and
Reset on every page ([working-1280-dark], [interrupted-1280-dark]).

Cancel answer is styled as an accent-outlined button, the same as Stop waiting and Skip the
quiz. It is the first button on the screen in most phone states. One click discards the
agent's whole next turn and the marks it made, with no confirmation, while Reset has one.
Reset itself is a grey outlined "button" (`style.css:98`) placed right under Stop waiting or
Retry, at the same size, in the main flow.

Change:

- **Cancel answer:** a quiet text button in the turn strip, "Cancel this answer…" (0.875rem,
  `--muted`, underlined), using the same `<details>` pattern as `reset.html`. Inside: the hint
  that is now always visible (`cancel-answer.html:10`), then a secondary button "Confirm:
  cancel my answer".
- **Reset:** move it to the masthead's right as a quiet "Reset…" (or to a footer row under a
  hairline), out of the action flow. Keep the red Confirm reset.
- Keep the action names the pane uses: Cancel answer, Reset, Confirm reset.

### 6. Every state that is not a question uses the same box; give them one status card

**[component, wording · medium]**

Seen in: [start-failed-1280-dark] (a red box, then a grey "No Explore round is running." box),
[start-failed-nothing-to-review-1280-dark] (a red "The round could not be started." with no
reason; the reason is the dimmest line on the page, under the disabled buttons),
[start-refused-1280-dark], [retry-refused-1280-dark], [question-1-answer-refused-1280-dark],
[question-2-pick-refused-1280-dark] (red for "someone else got there first"),
[interrupted-uncertain-1280-dark] (a grey box; the instruction that matters, "Check the agent's
conversation before you retry", ends a three-line sentence), [earlier-round-1280-dark] (an
irrelevant second box: "The agent is not working on its next turn, and the round has none to
send again"), [delivery-failed-1280-dark] (a raw reason as the headline, no next step),
[implement-sent-1280-dark] (success shown as a neutral grey box),
[implement-unknown-1280-dark].

These twenty states are where the reviewer has to understand fast and act right. Today the
eye cannot tell a success from a warning, a stale notice from a failure, or the reason from
what to do.

Change: a `status.html` macro `status(kind, title, reason, next, actions)`:

- **Layout:** a 1.6em round glyph in the kind's colour, then the title (weight 650), the reason
  (0.9375rem, `--muted`, verbatim error strings in `code`), the next step (0.9375rem, ink), and
  an action row: buttons left-aligned, primary first, hint inline.
- **Frame:** 1px border at the kind's colour 45%, background at the kind's colour 8%, radius
  10px, padding 14px 16px, at most `--measure` + 34px wide.
- Use it in `start.html`, `page.html:22-27` (starting, working), `interrupted.html`,
  `storage-failed.html`, `earlier.html`, `notice.html` and the state lines of `implement.html`.

Kinds:

| State | Kind | Title (sketch) | Action |
| --- | --- | --- | --- |
| starting, working, implement-sending | progress (accent), with an indeterminate 3px bar | "Preparing the round", "The agent is working on your answer to question 2" | Stop waiting (secondary) |
| *-refused notices, earlier-round, interrupted (stopped), implement-cancelled | info (accent) | "Question 1 was already answered", "This is an earlier round", "The turn is paused" | Retry, or none |
| not-started, interrupted-uncertain, implement-not-started, implement-paused, implement-unknown | warn | "The agent did not start on your answer", "The agent may or may not have your answer" | Retry; secondary when retrying could duplicate |
| delivery-failed, start-failed, storage-failed, implement-failed | danger | "Your answer did not reach the agent", "The round could not start" | Retry, Start, Implement |
| start-nothing-to-review, implement-sent | ok | "Every changed line is reviewed", "The agent received the implementation request" | none |

Also:

- In `start.html:11`, drop "No Explore round is running." when a failure card shows.
- In `earlier.html` with `interrupted.html`, show only the earlier-round card.
- The nothing-to-review state is good news. Show it as ok with the unmark hint, and keep the
  disabled buttons under it.
- The start screen becomes a cover: the review title at 1.625rem, the revision and repository
  as a muted line, then Start and Start with Challenger, each with a one-line description
  under it. The current hint only explains the Challenger and sits above both buttons.

Mock-up: `design-review/states.html` (`states-1280-light.png`, `states-390-light.png`).

### 7. Colour means too many things, and the page has two palettes

**[colour · small]**

Seen in: everywhere.

- **Red (`#c5221f`) means four things:** errors, stale notices (`style.css:28`), the
  destructive Confirm reset (`style.css:101`), and wrong quiz answers (`style.css:67,69`). The
  diagram failure uses `--bad`.
- **Grey means both "check before you act" and "success":** [interrupted-uncertain],
  [implement-unknown], [implement-paused] and [implement-sent].
- **Accent blue means five things:** primary buttons, secondary borders, the selected choice,
  the recommended choice, and the working tint.
- **Two palettes.** `style.css` hard-codes `#3367d6`, `#c5221f` and `#1e8e3e` for both themes,
  while `markdown.css:3-9` has proper `light-dark()` tokens (`--good`, `--bad`, `--warn`,
  `--tip`, `--conclusion`). In dark mode the hard-coded red is duller than `--bad`, and the
  quiz green is not the table green.
- "Next question" uses the browser's default link colour: `#0000EE` in light, lavender in dark
  ([quiz-correct-390-light], [quiz-wrong-1280-dark]).

When colour means everything, it means nothing. The reviewer can no longer read a state at a
glance.

Change:

- Move the tokens to one `:root` block in `style.css`:
  - `--accent = light-dark(#0969da, #4493f8)`, which is `--tip`; for text, borders, selection
    and links;
  - `--accent-fill = light-dark(#0969da, #1f6feb)`, for white button text at 4.5:1 or more;
  - `--ink`, `--muted = light-dark(#59636e, #a3a9b1)`, `--line`, `--line-strong` (ink 30%);
  - `--panel = light-dark(#f5f7f9, #1b1d21)`;
  - `--canvas`, set explicitly on `body`.
- Rule for each colour:
  - accent: you act here, or this is selected;
  - `--good`: success or correct;
  - `--warn`: check something before you act;
  - `--bad`: something failed, wrong, or destructive (the Confirm reset fill);
  - `--conclusion` (purple): the agent's judgement, meaning the recommendation tag and the
    conclusion callout;
  - neutral or accent: information.
- Replace the hard-coded colours in `style.css:3,26,28,66-70,101` and use the tokens for the
  diff tints in `citations.css:17-18,39-40`.
- Add `a { color: var(--accent) }`.

Mock-up: `design-review/proposal.css`, first block.

### 8. Desk and panel: make the two-column split the organising principle

**[layout · medium]**

Seen in: [question-1-1280-dark], [question-2-blind-picked-1280-light], [conclusion-1280-dark],
[implement-paused-1280-dark].

The split serves reading and deciding, and the sticky column is right. Keep both. Four
weaknesses:

- **The panel is a bare column.** Nothing marks it as the zone where you act.
- **24rem is narrow for long choices.** Every choice wraps to three lines, while the reading
  column has about 140px of slack beyond the measure.
- **The sticky panel can outgrow the window** (`max-height: calc(100vh - 32px)`, `overflow-y:
  auto`, `layout.css:54-55`). In [question-1-1280-dark] it is about 820px against the 868px
  available. Open the 29-line marks list or write a longer comment, and Send scrolls out of
  sight inside an inner scrollbar.
- **The design explanation above question 1 is full width.** Its paragraphs stop at 72ch,
  leaving 40% of the width empty to their right, while its diagram and table run 1,180px
  wide. The panel is absent while it is read.

Change, in `layout.css`:

- `--actions: 26rem` for the question; 30rem for the conclusion (finding 10).
- The panel gets `background: var(--panel)`, radius 12px and padding 16px.
- A `.panel-actions` wrapper around Send with `position: sticky; bottom: 0; background:
  inherit`, so the primary action is always visible inside the panel. Send becomes full width,
  44px tall.
- The design section uses the same grid: its text in the reading column, and the right column
  holding an outline of its four parts plus "Go to question 1 ↓". *Revised by finding 29:* the
  outline becomes a design map, with each part's thesis, the part in view marked, and a preview
  of question 1's text.

Mock-up: `design-review/question.html`; the design's panel in `design-review/design.html`.

### 9. Phone: much to scroll before the question, narrow text, small actions

**[layout · medium; mostly follows from findings 1, 6, 16 and 19]**

Seen in:

- [question-1-390-light]: the question text starts at y≈3,870, the choices at ≈5,800 and Send
  at ≈6,700 of 7,458px. The design alone is about 3,700px, 4.4 screens.
- [question-2-blind-390-light]: no question in the first screen.
- Every 390 shot: the body gutter of 16px, the section border and the section's 16px padding
  leave a 325px text column on a 390px phone.
- Send and Pick are 79×40 buttons, right-aligned. The comment box does not grow
  ([question-2-blind-picked-390-light]: "first." is cut off).

Change:

- Remove the section boxes (finding 19), so text runs 358px wide.
- The panel goes full-bleed under 40rem (`margin: 0 -16px; border-radius: 0`).
- Send, Pick, Check and Implement become full width, 44px or taller.
- Add a "Choices ↓" link in the question eyebrow, shown only under 40rem, pointing to
  `#answer`.
- Clamp the turn strip (finding 1).
- On phones, fold design sections 2 to 4 under their headings (finding 17).
- Give `.comment` and `.tasks` `field-sizing: content` with min and max heights.
- Keep the reading order question → context → choices → citations. The reviewer asked for the
  form above the code, and the order reads naturally once the top is compressed.

Mock-up: `design-review/question-390-light.png` / `-dark.png`.

### 10. Conclusion and implementation: the list is the payoff, but it is hidden or doubled

**[layout, component, wording · medium]**

Seen in:

- [conclusion-1280-dark]: the 6-row textarea shows 3 of 10 items.
- [implement-failed-1280-dark] and [implement-cancelled-1280-dark]: the same.
- [implement-paused-1280-dark]: the list appears twice, rendered and in the textarea. "Send the
  saved request" is secondary and left-aligned; "Send a new request" is primary and
  right-aligned.
- [implement-unknown-1280-dark]: the primary action is the risky "Send a new request", and the
  textarea touches the list with no gap.
- [implement-sent-1280-dark]: success as a grey box.
- [conclusion-quiz-results-1280-dark]: the quiz score is a fold at the very end, after Future
  work.
- [conclusion-quiz-results-390-light]: on a phone, Future work comes after the Reply form.

The list is what the reviewer authorises. It is the most consequential text of the round, and
it is squeezed into a 24rem column and cut to three items. A quiz score of 1 of 3 is a signal
that matters right at the Implement decision, and it is the last thing on the page.

Change:

- `.tasks` gets `field-sizing: content; min-height: 6lh`; as a fallback for Firefox, the
  server sets `rows` to the line count + 1 (`implement.html:30`). The conclusion panel is 30rem
  wide.
- The panel opens with the quiz score as a small banner: "Quiz 1 of 3 · You missed what a
  crash loses · See the answers", which links to the results fold. Use a warn tint below
  two-thirds, ok above.
- **Paused:** show the saved list once, with "Send the saved request" as primary. "Edit before
  sending" is a disclosure that reveals the textarea and a secondary "Send a new request".
- **Unknown:** a warn card (finding 6). The only action is a secondary "Send a new request
  anyway".
- **Sent:** an ok card, "The agent received the implementation request".
- Reply moves under a disclosure, "Not ready? Reply to the agent instead", so the panel has one
  primary action.
- Optional: label the button "Implement 10 items", counting non-empty lines.
- In `conclusion.html`, put Future work before the `to-be-implemented` section in the DOM. The
  desktop grid is unchanged; on a phone, reading then comes before acting.

Mock-up: `design-review/conclusion.html`. Its "Your decisions" list is finding 24.

### 11. Door and Blast radius are folded and say nothing until opened

**[component, content · medium]**

Seen in: [question-1-1280-dark] and [question-2-blind-1280-dark] (two bordered rows reading
"▸ Door" and "▸ Blast radius"); [question-1-unfolded-1280-dark] and
[question-3-diagram-error-1280-dark] (the verdict "Two-way —" is buried at the start of the
body).

A fold with no hint of its content rarely gets opened, so these two sections do not earn their
place as they stand. They would if their verdict showed: the door also explains why a
question is blind.

Change:

- The question eyebrow gets a chip from `Assessments.door`: "Two-way door" in `--good`,
  "One-way door" in `--warn`, "Mixed door" and "Door unknown" neutral. Blind questions add a
  "Blind pick" chip in purple.
- Each summary row shows the lens's summary after the label, on one line with an ellipsis,
  wrapping when open: "**Door** One-way — a dropped notification cannot be sent later…".
- This needs the door kind and the two summaries as data, not baked into the body. Today
  `Assessments::sections` (`review-explore/src/sections.rs:29-42`) prefixes the body with the
  label. Add a `lead: String` to `QuestionSection`, or pass the door and leads to
  `QuestionContext`, and render them in `explanation.html`.
- Replace the hint in `pick.html:7`: "This decision is hard to reverse, so the agent's
  recommendation stays hidden until you pick." It now says why.

Mock-up: `design-review/question.html`.

### 12. After a post, the page lands at the top, not on what changed

**[behaviour · small]**

Seen in: [question-2-blind-picked-1280-dark] (the reveal at y≈940, below a 900px fold),
[quiz-correct-1280-dark] (the verdict at y≈950), [quiz-wrong-1280-dark] (y≈925), and every
phone state.

Every post redirects to `/` (`page.rs:677`), and the page shows the turn strip, design and
reply first. Each check therefore costs a scroll to see its own result, which breaks the "act,
see the result" rhythm that keeps attention.

Change:

- Redirect `/pick` to `/#answer`, `/quiz` to `/#quiz`, and failed `/implement` to
  `/#to-be-implemented`.
- Add those ids to `answer.html`, `pick.html`, `quiz.html` and `implement.html`.
- Strip the fragment after the jump (finding 3).

### 13. Waiting is a dead screen

**[behaviour, component · medium; small without the timestamp]**

Seen in: [starting-1280-dark], [working-1280-dark], [working-after-answer-1280-dark],
[implement-sending-1280-dark].

A static tinted box, "The agent is working…", gives no sense of time and does not say which
turn is running. The reviewer cannot tell a normal two-minute turn from a stuck one, and Stop
waiting exists precisely for stuck ones.

Change:

- Show the progress card of finding 6 with a 3px indeterminate bar (CSS animation, a static
  bar under `prefers-reduced-motion`).
- The title names the turn: "The agent is working on your answer to question 2", or "…on the
  design and its first question".
- Add a line "Sent 0:42 ago". The best source is a `sent_at` timestamp in the working stage;
  the time since the page loaded is a weaker fallback.
- Keep the turn strip above, so the reviewer sees what was sent.
- Use the tab title from finding 4.

Mock-up: `design-review/states.html`, first card.

### 14. Diagrams look borrowed and take too much room

**[component · medium]**

Seen in:

- [question-1-1280-dark] and [question-1-1280-light]: the sequence diagram is about 1,050px
  tall, with the actor boxes repeated at the bottom (Mermaid's `mirrorActors`). It uses
  Mermaid's default palette (lavender boxes and purple lifelines in light, which clash with the
  blue accent and the purple conclusion colour; flat grey in dark), its own font (Trebuchet or
  Arial), and grey label boxes on the flowchart edges.
- [question-1-390-light]: on a phone the same diagram shows one lifeline per screen for 900px.

The reviewer asked for visual explanations. Diagrams are the strongest tool for that, and
today they look pasted in from another product.

Change, in `assets/diagrams.js:31-35`:

- `theme: 'base'`, with `themeVariables` read from the page's computed tokens:
  - `fontFamily: 'system-ui, sans-serif'`, `fontSize: '14px'`;
  - `primaryColor` and `actorBkg` = `--panel`; `primaryBorderColor` and `actorBorder` =
    `--line-strong`;
  - `primaryTextColor` and `signalColor` = `--ink`; `lineColor` = `--muted`;
  - `edgeLabelBackground` = `--canvas`; `noteBkgColor` = warn 12%.
- `sequence: { mirrorActors: false, actorMargin: 32, messageMargin: 24, boxMargin: 8 }` and
  `flowchart: { curve: 'basis', padding: 10 }`.
- On desktop, let a diagram shrink to the column when its natural width is at most 1.4 times
  the column (14px text stays at least 10px). Wider ones keep their size and scroll.
- Frame every figure the same way: diagrams, wide tables and the ASCII timeline blocks get a
  1px `--line` border, radius 8px, 12px padding and a `--panel` background.
- On phones, add a scroll cue to the frame: an edge shadow using `background-attachment:
  local` gradients.

*Revised by finding 30,* which gives the exact configuration as drawn in `design.html`, a
stricter size rule (shrink only down to 0.85, so text stays at 12px or more, instead of 1.4×),
top-down redrawing of wide flowcharts on phones, and the `new` / `changed` node classes. Wide
tables keep their own grid rather than a figure frame.

### 15. Citations: a detached header, solid green blocks, and almost no code visible on a phone

**[component · medium]**

Seen in:

- [question-1-unfolded-1280-dark] and [question-2-blind-1280-dark]:
  - the location `src/notify/queue.rs new 22–38` is bold mono floating above the note, apart
    from its code;
  - when every row is added, the whole block is a solid green block, and the tint then says
    nothing;
  - lines are cut at the column edge with no sign that they continue.
- [question-1-unfolded-390-light] and [question-1-390-light]: two number columns, the sign and
  four spaces of shared indentation leave about 22 characters of code per line on a phone.

Change:

- `citation.html:5-6`: the note stays above as the "why". The location moves into a header bar
  inside the code frame: the path in mono at weight 600, the side and range muted ("new
  22–38"), on a 4% ink background with a hairline under it.
- `citations.css:17-18`: row tints drop to 9%, plus a 3px gutter bar in `--good` or `--bad` on
  the sign cell. When every row is added, drop the row tint and keep the gutter.
- Fade the right edge when a row overflows.
- On the server (`crates/review-explore-citations`), strip the block's common leading
  indentation.
- Under 40rem, show a single number column: the new number, or the old one for removed rows.

Mock-up: the citation in `design-review/question.html`.

### 16. Tables fall apart on a phone

**[component · small]**

Seen in: [question-1-390-light] (the design's table wraps "The / replies / the / agent / has /
not / heard…" one word per line, over 280px for one row), [question-2-blind-390-light].

`.markdown th, td { min-width: 4em }` (`markdown.css:27`) lets the browser squeeze columns
instead of scrolling the frame. The tables with status marks are the best visual device on the
page, so they are worth keeping intact on a phone.

Change, under 40rem:

- `th, td { min-width: 8em }`, 7em for the first column.
- The first column is `position: sticky; left: 0`, with a canvas background, so row labels
  stay visible while the table scrolls.
- The same edge-shadow scroll cue as finding 14.

### 17. Long explanations have no skim path

**[typography, content · small; medium for the phone folding]**

Seen in: [question-1-1280-dark] (the design is about 2,550px at 1280, about 2.8 screens),
[question-1-390-light], [conclusion-1280-dark] (run-in bold labels inside long paragraphs).

The design's section headings are 16px bold, the same size as body text (`style.css:37`), and
every paragraph has the same weight and spacing. The reviewer said reading all that text is
"inefficient and boring". A skim path lets them get the gist in 30 seconds and choose where to
go deep.

Change:

- Design `h3`: 1.0625rem, weight 650, 32px above, with a hairline rule.
- The first paragraph of each design section and of Context becomes a lead: `.design .markdown
  > p:first-child, .explanation > p:first-child { font-size: 1.0625rem }`. The prompts already
  ask for "a short summary paragraph first".
- Below 40rem, render each design section as its own `<details>`, the first one open, in
  `design.html`.
- Watch whether the quiz scores drop with this folding. If they do, keep the sections open.

*Revised by findings 28 and 29* for the design: each part gets a thesis and a numbered header
instead of a styled lead paragraph. On phones, nothing folds; sticky part headers and the design
map give orientation, so every thesis and figure stays in view. Lead paragraphs remain the rule
for Context and the conclusion.

### 18. Quiz: a weak next step, and marks that rely on colour

**[component · small]**

Seen in: [quiz-1280-dark], [quiz-correct-1280-dark], [quiz-wrong-1280-dark],
[quiz-correct-390-light].

- The tags run together as dim text: "your pick correct answer" (`quiz-item.html:12-13`).
- Right and wrong differ only by a 1px green or red border.
- "Next question" is an unstyled default link (`quiz.html:26`), weaker than the outlined "Skip
  the quiz" under it.
- Check is right-aligned and Skip is left-aligned on the next row.

Change:

- Each answer starts with a status mark, reusing `.status` from `markdown.css:33` (✓ on the
  correct one, ✗ on a wrong pick), plus pill tags "Your pick" (accent) and "Correct" (good).
- The verdict line gets the ok or danger glyph.
- "Next question →" becomes a `.send` button, a GET form or a link styled as a button, followed
  by a quiet "Skip the quiz" on the same row.
- Show the item number as an eyebrow, "QUESTION 2 OF 3", with three small dots.
- Land on `#quiz` after Check (finding 12).

### 19. Boxes inside boxes: let space do the grouping

**[layout · small]**

Seen in: everywhere.

- Every section is a bordered 8px box (`style.css:7,33,92`).
- Inside are tables, Door and Blast radius rows, choice cards, code frames, text areas and
  state boxes, each with its own border.
- [question-1-1280-dark] nests text inside Door rows inside the question box inside the page.

The borders add noise and eat 33px of width on each side of a phone.

Change:

- Remove the borders and padding from `.question`, `.conclusion`, `.quiz`, `.response`,
  `.last-answer` and `.design`. Separate stages with 32-48px of space.
- Keep borders only on interactive or framed objects: choices, inputs, code, tables and
  figures.
- The panel gets a surface (finding 8) instead of a border.

### 20. No type scale; labels compete with content

**[typography · small]**

Seen in: everywhere. Headings use browser defaults:

- "Explore" at 2em;
- "Question 1", "Quiz" and "Conclusion" at 1.5em;
- "Design of the change" and "Reply from the agent" at 1.15em;
- "To be implemented" and "Future work" at 1.17em;
- "Your last answer" and "Citations" at 1em bold;
- the "Choices" legend in plain weight.

Hints use `opacity: .75` (`style.css:13`), which also dims any code or link inside them.

Change: a small scale.

- Masthead: 0.9375rem.
- Stage headline (question, quiz item, conclusion title): 1.375rem, weight 650.
- Section heading: 1.0625rem, weight 650.
- Eyebrow labels: 0.75rem, uppercase, weight 650, 0.06em letter-spacing, `--muted`. Use them
  for "Choices", "Comment · optional", "Citations", "To be implemented", "Proof", "You answered
  question 1" and "The agent recorded". Labels then stop competing with content.
- Body: 1rem/1.55.
- Small text: 0.875rem in `--muted`, through colour, not opacity.

### 21. Buttons: one hierarchy, one placement rule

**[component · small]**

Seen in: [interrupted-1280-dark] (a 40px Retry next to a 42px Reset), [implement-not-started-1280-dark]
(Retry left-aligned, while Implement in the same column is right-aligned), [quiz-1280-dark]
(Check right-aligned, Skip left-aligned), [start-1280-dark].

- `.send` is right-aligned (`style.css:26`, `justify-self: end`). Stop waiting, Retry, Cancel
  answer, Send the reply and Send the saved request are left-aligned (`style.css:91`).
- Primary buttons have no border and secondary ones have 1px, so their heights differ.
- Secondary buttons have an accent outline, which makes them look half-primary.

Change:

- Four tiers with the same box: a 1px border, transparent on filled buttons; min-height 40px,
  44px under 40rem; radius 8px.
  - **Primary:** filled `--accent-fill`.
  - **Secondary:** a `--line-strong` border and ink text, not accent.
  - **Quiet:** text with an underline, for Cancel answer, Skip the quiz, Reset, and "Not
    ready? Reply".
  - **Danger:** red fill, only for Confirm reset.
- Placement: in the panel, the primary action is full width at the end. In status cards and in
  the flow, a left-aligned row with the primary first. One primary per view.

### 22. A diagram that does not parse is the loudest thing on its page

**[component, wording · small]**

Seen in: [question-3-diagram-error-1280-dark], [question-3-diagram-error-390-light]. A red bar,
a red title, then Mermaid's parse dump ("Expecting 'SQE', 'DOUBLECIRCLEEND', 'PE', …") in the
middle of the explanation. The reviewer cannot fix it, and the source reads fine as text.

Change, in `diagrams.css:6-9` and `diagrams.js:61-72`:

- The neutral figure frame of finding 14.
- A muted caption: "This diagram could not be drawn; here is its source."
- The source as code.
- Mermaid's message inside a closed `<details>` titled "Mermaid's message".
- Keep reporting the error.

### 23. Wording: say the state first, then the next step

**[wording · small]**

Seen in: the states of finding 6, plus the hints around the answer form. Keep every action
name the pane and the docs share (Start, Stop waiting, Retry, Cancel answer, Reset, Confirm
reset, Implement, Send). Change only the titles and hints:

| Now | Proposed |
| --- | --- |
| "The agent is not working on its next turn: you stopped waiting, or the review pane was reopened before it sent the turn's prompt." | Title "The turn is paused"; reason "You stopped waiting, or the review pane was reopened before the prompt went out."; next "Retry sends it again, with your answer." |
| "The agent is not working on its next turn as far as the review tool knows: …" | "The agent may or may not have your answer" · "The review pane was reopened while it sent the prompt." · **"Check the agent's conversation before you retry."** |
| "The agent is not working on its next turn." + "The selected agent is no longer available" | "Your answer did not reach the agent" · reason verbatim · "Select an agent in the review pane, then Retry." |
| Cancel answer hint, always visible, two lines | Only inside its confirmation (finding 5) |
| "Reply from the agent" / "Recorded:" | "The agent recorded" (turn strip) |
| "Pick your answer before you see the agent's recommendation: it shows once you have picked." | "This decision is hard to reverse, so the agent's recommendation stays hidden until you pick." |
| "You picked '…' first. Keep your pick or change it, then send." | The reveal line of finding 3 |
| "The implementation request could not be sent: The selected agent…" | Lower-case the reason, or put it on its own line |
| "With the Challenger, a second agent with fresh context reviews the change beside the agent." | One line under each start button (mock-up) |

### 24. Give the reviewer a decision log

**[content, behaviour · large]**

Seen in: nowhere, and that is the point. The page forgets each answer once the next one is
given. The conclusion's "Decisions." paragraph is the agent's prose
([conclusion-1280-dark]).

For the whiteboard test, the best study sheet is the round's own decisions: for each question,
its short text, what the reviewer kept, and a tag ("as recommended", "changed after your first
pick", "differs from the agent").

Change: publish the round's answered questions to the page (question number and text, the
answer's choice, first pick and recommendation) and render them:

- as a "Your decisions" list in the conclusion's reading column;
- as tooltips or links on the rail's done steps (finding 4).

Mock-up: `design-review/conclusion.html`.

### 25. Make the comparison table the default explanation

**[content · small; a prompt change, outside the page]**

Seen in: [question-2-blind-1280-dark] and [question-1-1280-dark]. The "choice × consequence"
table with status marks is the single most effective explanation in the gallery: one look
tells you what each choice costs.

Ask the agent, in the question prompt, for one such table whenever a question has two or more
substantive choices: rows in the order of the choices, columns for the consequences the
reviewer cares about. The page needs no change. Following AGENTS.md, test that the prompt
carries the request, not its wording.

*Extended by finding 28:* tables also lead two of the design's four parts (the cost table and
the comparison of rejected alternatives).

### 26. No focus or hover styles

**[component · small; read from the CSS, not visible in stills]**

There is no `:hover` and no `:focus-visible` rule anywhere in the stylesheets. Choice cards
rely on the native 13px radio ring, and the code frames are focusable (`tabindex=0`) with the
default outline.

Change:

- `.choice:hover { border-color: var(--line-strong) }`.
- `.choice:has(:focus-visible), button:focus-visible, summary:focus-visible, .code:focus-visible
  { outline: 2px solid var(--accent); outline-offset: 2px }`.

### 27. Small accidents

**[various · small]**

- The last answer's comment looks like an input box ([delivery-failed-last-answer-1280-dark],
  [working-after-answer-1280-dark]). `cancel-answer.html:7` uses `p.comment`, which picks up
  the textarea style at `style.css:23`. Rename it `.answer-comment`.
- "Your last answer" is empty in [retry-refused-1280-dark]: a heading, a hint and a button,
  with no choice and no comment. Show "(no choice, no comment)", or hide the strip.
- In [implement-unknown-1280-dark], the textarea touches the rendered list
  (`implement.html:21-23`). Add a 12px gap, or drop the duplicate (finding 10).
- "Next question" uses the browser's default link colour (finding 18).
- Radios are centred on multi-line labels (finding 3).
- In [start-failed-nothing-to-review], the red box has no reason and the reason is the
  faintest line on the page (finding 6).

## 2b. The design of the change

The design that opens the round is the part of the page the whiteboard test is about: what
the change adds and where, its types and data flow, its algorithm and cost, and the
alternatives it rejected. After it, the reviewer should be able to say the architecture in a
few sentences and draw it. Today it is 2,550px of evenly weighted prose above question 1, and a
mute "Design of the change" box in every later stage.

What I propose, in one line per idea:

- **Say it before showing it.** One sentence for the whole change, then one sentence per part,
  the line you would say at a whiteboard (finding 28).
- **Show it before explaining it.** Each part opens with its figure: a component map, a
  numbered sequence diagram, a cost table, a comparison table. Prose follows, as commentary on
  the figure (finding 28).
- **Know where you are and what comes next.** On a wide window, the panel column holds a map of
  the four parts and the question they prepare; on a phone, part headers stay at the top of the
  screen. The rail's first step is the design (finding 29).
- **Diagrams that belong to the page.** The page's theme and type, a compact layout, natural
  size when it fits, top-down on phones, and a shared "new / changed" vocabulary (finding 30).
- **Folded, but worth reopening.** In later stages, one line that recaps the change; opening it
  starts with a summary of the four parts, with thumbnails of their diagrams (finding 31).

The design and its first screen are in `design-review/design.html`, with synthetic content of
the gallery's shape: the same change, two Mermaid diagrams drawn by the vendored Mermaid
11.17.2 with the configuration of finding 30, four tables with status marks, and a callout.

### 28. The design reads as a wall: give the change and each part a thesis, and lead each part with its figure

**[content, typography · medium; small with the first-paragraph fallback]**

Seen in:

- [question-1-1280-dark] and [question-1-1280-light]: the design is ≈2,550px. Its four headings
  are 16px bold, the size of body text (`style.css:37`). "Types and data flow" opens with prose
  before its diagram. "Algorithm and cost" is three paragraphs with the costs inside
  sentences. "Rejected alternatives" is three paragraphs with bold run-in names.
- [question-1-390-light]: ≈3,700px, with nothing to tell how far through you are.

To pass the whiteboard test, the reviewer needs the claims and the pictures. Today they must dig
the claims out of paragraphs, and two of the four parts have no picture at all. Five sentences
and four figures are the whiteboard; everything else is commentary.

Change:

- **Schema** (`review-explore/src/design.rs`): `Design` gains `thesis`, one sentence for the
  whole change. Each part becomes `DesignPart { thesis, body }`, the thesis being one concrete
  sentence that names the parts it talks about, under about 160 characters. Validate that each
  is non-empty and a single sentence. The pane shows each thesis in bold as the part's first
  line.
- **Prompt** (the design part of the kickoff): each part's body opens with its figure.
  - What it adds and where: a component map (`flowchart LR`, new components marked `:::new`,
    changed ones `:::changed`, finding 30), then a where-table: file, what it holds, lines.
  - Types and data flow: a sequence diagram with `autonumber`, then the types table (type,
    holds, lives, after a crash, with status marks). The prose can then say "steps 3 to 8 are
    the change".
  - Algorithm and cost: a cost table (operation, runs, cost now, before, with status marks),
    then at most one worked example and one callout.
  - Rejected alternatives: a comparison table (alternative, the cases that matter, why not,
    with status marks). The change is the first row, and an inferred alternative says so in
    its row.
  - Following AGENTS.md, test that the prompt asks for the theses and the figures, not its
    wording.
- **Page** (`design.html` template, `proposal.css` "finding 28" blocks):
  - The header: the eyebrow "Design of the change · read before question 1", the change's
    thesis as the `h2` (1.375rem/1.35, weight 650, at most `--measure`), and a meta line "4
    parts · about 4 minutes · 4 files". Reading time is words / 230; the file count comes from
    the diff.
  - Each part: a header row (a 1.6rem number badge, the part's name as an eyebrow, "2 / 4" at
    the right), then the thesis (1.125rem/1.45, weight 600), then the body.
  - No box around the design (finding 19), and 40px between parts. Drop the 16px side padding
    that `style.css:37-38` gives the design's headings and Markdown.
- **Fallback without the schema change:** style each part's first paragraph as its thesis
  (`.part .markdown > p:first-child`), and ask the prompt to open each part with one sentence.

Mock-up: `design-review/design.html`. In `design-1280-light.png`, the first 900px hold the
change's thesis, the four part theses with the question they prepare, and part 1's thesis and
component map. The mock-up's design is not shorter than today's: question 1 starts at about
3,300px at 1280, because two parts gain a figure. It is faster to take in: the first screen
carries the whole argument, and each part can be read from its thesis and figure alone.

### 29. Navigate the four parts: a design map in the panel column, sticky part headers on a phone

**[layout, behaviour · medium]**

Seen in: [question-1-1280-dark] (the design spans the full width, its paragraphs stop at 72ch
with 40% of the width empty beside them, and nothing outlines it or says what comes next);
[question-1-390-light] (four and a half screens with no sign of progress).

Change:

- **Wide window:** the design section uses the desk-and-panel grid of finding 8:
  `grid-template-columns: minmax(0, 1fr) var(--actions)`, with rows `repeat(5, auto) 1fr`. The
  right column holds the design map, sticky at `top: 16px`, on the `--panel` surface:
  - the four parts as links, each with its number, its name (0.9375rem/650) and its thesis
    (0.8125rem, `--muted`);
  - the part in view is marked (accent 10% background, filled number badge), and parts already
    read get a green ✓ badge. A 15-line `IntersectionObserver` in `page.js` does this; without
    it, the links still work;
  - under the parts, "Then · question 1" with the question's text and a secondary "Go to
    question 1 ↓".
- **Why a preview of the question and not its choices:** reading with the coming question in
  mind gives the reading a purpose. Showing the choices would invite a pick before the reviewer
  has read the question's own context. The answer panel appears with question 1, in the same
  column, so that column always says where you are and what comes next.
- **Phone:** the same `nav` sits under the headline, full-bleed. The DOM order is header, map,
  parts, and the grid moves the map to the right column on wide windows. Each part header is
  `position: sticky; top: 0` with "2 / 4", a canvas background and a hairline, so the reader
  always knows which part they are in. Nothing is folded: every thesis and figure stays in
  view.
- **Rail** (finding 4): "Design" is the first step. On the first screen it is current and Q1 is
  next (ink, outlined); the observer hands "current" to Q1 when the question scrolls into view.
  In later stages, "Design ✓" links to the folded design (finding 31).

Mock-up: `design-review/design.html` (`design-1280-*.png`, `design-390-*.png`).

### 30. Diagrams: the page's theme, natural size when it fits, top-down on a phone, a shared vocabulary

**[component · medium]** · revises finding 14

Seen in: as finding 14. [question-1-1280-dark] and [question-1-1280-light] draw a sequence
diagram of ≈1,080×1,050px with the actors repeated at the bottom, in Mermaid's default palette
and font.

Change, in `assets/diagrams.js`. The mock-up's drawings use exactly this configuration, with the
vendored Mermaid 11.17.2:

```js
mermaid.initialize({
  startOnLoad: false, securityLevel: 'strict', theme: 'base',
  fontFamily: 'system-ui, -apple-system, "Segoe UI", "Noto Sans", sans-serif',
  themeVariables: {            // literal colours, read from the page's tokens at draw time
    fontSize: '14px', background: canvas,
    primaryColor: panel, mainBkg: panel, actorBkg: panel,
    primaryBorderColor: lineStrong, nodeBorder: lineStrong, actorBorder: lineStrong,
    primaryTextColor: ink, textColor: ink, signalColor: ink, actorTextColor: ink,
    lineColor: muted, actorLineColor: line, loopTextColor: muted,
    edgeLabelBackground: panel, labelBoxBkgColor: canvas, labelBoxBorderColor: lineStrong,
    noteBkgColor: warnTint, noteBorderColor: warn, sequenceNumberColor: canvas,
  },
  themeCSS: '.node.new rect { fill: goodTint; stroke: good; stroke-width: 2px } ' +
            '.node.changed rect { fill: warnTint; stroke: warn; stroke-width: 2px }',
  sequence: { mirrorActors: false, actorMargin: 22, width: 112, height: 40, messageMargin: 30,
              diagramMarginX: 8, diagramMarginY: 18, useMaxWidth: false },
  flowchart: { curve: 'basis', nodeSpacing: 30, rankSpacing: 34, padding: 10, useMaxWidth: false },
});
```

- **Colours:** Mermaid needs literal colours. Read the tokens with `getComputedStyle` before
  each draw; `diagrams.js` already redraws when the theme changes.
- **Edge labels** sit on `--panel`, the figure's own background, so they do not punch dark or
  white holes in it.
- **Vocabulary:** `new` and `changed` node classes, styled by the page (`--good`, the green of
  added lines in citations; `--warn`). The page adds a "new · changed · unchanged" legend under
  a diagram that uses them, and the kickoff names the two classes.
- **Size rule** (replaces finding 14's 1.4×): a diagram keeps its natural size when it fits.
  When it is wider than its frame, it shrinks only while the scale stays at 0.85 or more, so
  14px text stays at 12px or more. Otherwise it keeps its size in the frame, which scrolls
  sideways with edge shadows and a "Scroll sideways · 5 participants" caption.
- **Direction:** a `flowchart LR` that does not fit is drawn again as `TD` (swap the keyword
  before rendering). In the mock-up, the component map goes from 808px to 374px wide on a
  phone.
- **Frame:** a figure with a `--panel` background, a 1px `--line` border, radius 8px and 12px
  padding; full-bleed on a phone. Tables keep their own grid.
- **Prompt guidance:**
  - `autonumber` in sequence diagrams.
  - At most five participants: beyond that, a sequence diagram no longer fits a 768px column.
  - Rounded `("…")` nodes rather than stadium `([…])` ones. Mermaid 11 draws a stadium as a
    hand-drawn path of about 30 KB; in the mock-up, this change took the component map from
    108 KB to 21 KB.

Measured in the mock-up: the gallery's flow, drawn this way, is 755×726px (against ≈1,080×1,050
today) and fits the 768px reading column at 1280 at its natural size.

### 31. The folded design: one line that recaps, a reopen that starts with a summary

**[component · small]** · refines finding 1

Seen in: [working-last-answer-1280-dark], [question-2-blind-1280-dark], [quiz-1280-dark],
[conclusion-1280-dark]: a 52px bordered box that says "▸ Design of the change" and nothing
about the design. Reopened, it is the same 2,550px wall.

Before each later question, and before the quiz, the reviewer benefits from a ten-second
refresher of the architecture. Today the fold gives them no reason to open it.

Change:

- In every stage after question 1, a `details.design-fold` directly under the masthead, with no
  box and a hairline under it. Its summary: ▸, the eyebrow "Design", the change's thesis on one
  line with an ellipsis (0.9375rem, weight 600), and "4 parts · reopen" in `--accent`.
- Opened, it starts with a recap: the four parts as cards (`grid-template-columns:
  repeat(auto-fit, minmax(15rem, 1fr))`, 1px `--line`, radius 8px, padding 12px). Each card
  shows the number, the name and the thesis, and links to its part. Cards of parts with a
  diagram add a 92px thumbnail of it, a memory cue rather than a copy to read
  (`preserveAspectRatio="xMidYMin slice"` for a sequence diagram). The full parts follow, as on
  question 1.
- The rail's "Design ✓" links to an anchor inside the fold. Chrome opens a closed `<details>`
  when a fragment points inside it; other browsers need a three-line script.

Mock-up: `design-review/design.html` (the note at the end), and `question.html` under the
masthead.

### Earlier findings these revise

- **Finding 1:** the folded design row is finding 31's row.
- **Finding 8:** the design's right column holds the design map and the preview of question 1
  (finding 29), not a static outline.
- **Finding 9:** on phones, the design map sits under the headline, and part headers stay at
  the top of the screen.
- **Finding 14:** finding 30 replaces its configuration and its size rule (0.85 instead of
  1.4×), and adds the top-down redraw and the node classes.
- **Finding 17:** for the design, a thesis per part replaces the lead paragraph, and nothing
  folds on phones. Lead paragraphs remain for Context and the conclusion.
- **Finding 25:** tables also lead two of the design's parts.

## 3. What to keep

- **The type base:** system UI at 16px, the 72ch measure, light and dark through `color-scheme`
  and the `light-dark()` tokens in `markdown.css`. The page never looks broken in either theme.
- **Status marks in table cells:** a round ✓, ✗ or ! with a tint of the same colour. This is
  the page's best visual device, and it reads in both themes.
- **Callouts:** a 4px bar, an 8% tint, a coloured title with its glyph. Restrained and
  consistent with the status marks; keep them as the model for the status card.
- **Diff citations:** Catppuccin syntax colours (Latte and Mocha), two line numbers on desktop,
  the most decisive citation first with the others folded, and the dashed "limitation" box for
  files that cannot be shown.
- **The answer form above the citations and the sticky action column,** both asked for by the
  reviewer and both right.
- **The recommendation comes with its reason,** not as a bare badge: it explains instead of
  pushing. Keep it, inside the card.
- **The blind first pick,** with the comment kept across the pick. It is the most distinctive
  idea in the product; finding 3 is about making its payoff visible, not changing it.
- **"Will mark 29 lines reviewed · 5 lines not relevant when you answer",** expandable to the
  lines. It makes the consequence of an answer visible before Send.
- **The quiz with its proof lines.** Checking understanding against code is exactly the
  whiteboard test.
- **The design open above question 1 and folded after it,** and its four parts in that order
  (what it adds and where, types and data flow, algorithm and cost, rejected alternatives).
  That order is the whiteboard outline; findings 28 to 31 change how the parts are shown, not
  what they are.
- **Reset behind a confirmation, with red used only on Confirm reset.** Extend the pattern to
  Cancel answer.
- **Plain server-rendered forms:** drafts kept in `sessionStorage` across reloads, the
  double-submit guard, refusals that say what happened.
- **Frames that scroll sideways on phones** for tables, code and diagrams, instead of the
  page; add the scroll cue.
- **Precise, honest wording** with no false reassurance, and action names shared with the pane
  and the docs.

## 4. What still images could not show

Questions for someone who uses the page:

1. At 1280×800 or 1280×900, with real choices, does the sticky panel ever scroll internally
   and hide Send, for instance with the marks list open or a long comment? (Finding 8 assumes
   it can.)
2. When the round changes while you read (the polling `location.reload()`), does the page keep
   your scroll position, or jump? Does a reload ever take focus from the comment box mid-word?
3. Keyboard: the pane answers with j/k, number keys and Ctrl-Enter, and the page has no
   shortcuts. Do you miss them? Is the tab order (Context, then Door and Blast radius, then the
   choices, then the citations) the one you expect?
4. Focus visibility on the choice cards and the code frames, and hover feedback on the cards
   (none is defined).
5. How long does a normal agent turn take, and when do you reach for Stop waiting? This
   calibrates the elapsed time and progress bar of finding 13.
6. Do you read the design explanation before question 1, or skip to the question? Would you
   start from the design map (finding 29) and jump, or read top to bottom?
7. On a real phone: is a full-width Send in thumb reach after a long scroll? Does sideways
   scrolling in a code or table frame fight the browser's back-swipe gesture?
8. Is there a flicker when Mermaid redraws a diagram after a theme change?
9. Does the blind-pick reveal change your mind in practice? How often do you and the agent
   differ? This is worth knowing before tuning the reveal line's tone.
10. In a long round, would you rather see the earlier decisions on the page (finding 24) or
    keep the page about the current question only?
11. Can the agent write good one-sentence theses reliably (finding 28)? A vague thesis ("This
    part describes the data flow") is worse than none; a few real rounds will tell whether the
    prompt needs examples.
12. Does the scrollspy of the design map feel helpful or jumpy while scrolling, and does the
    hand-over from the design map to the answer panel read as one column?
13. Do the diagram thumbnails in the reopened design (finding 31) help you recall the design, or
    are they decoration at that size?
14. Does the top-down redraw (finding 30) keep larger component maps readable, or do they get
    too tall on a phone? And does "about 4 minutes" match your real reading time?

## 5. Proposed changes, in build order

Each change can be built on its own and checked with `make explore-gallery` against the
previous run.

1. **Tokens, type, buttons, space** (findings 7, 19, 20, 21, 26 and the CSS parts of 27).
   Almost all CSS: `style.css`, `markdown.css`, `citations.css`, and `cancel-answer.html` for
   the `.comment` collision. Check every state in both themes for contrast, and that no state
   lost its grouping when the boxes went.
2. **Status cards and wording** (findings 6, 23, 5, and finding 13 without the timestamp).
   `status.html` macro; `start.html`, `page.html`, `stop.html`, `interrupted.html`,
   `storage-failed.html`, `earlier.html`, `notice*.html`, `cancel-answer.html`, `reset.html`.
   Check: the twenty short states at 390 and 1280.
3. **The frame: masthead, tab title and previous-turn strip** (findings 4, 1, and the top half
   of 9). `page.html`, a new `turn.html` replacing `cancel-answer.html` and `response.html`,
   and `LatestAnswer` with the question number and text. Check: [question-2-blind-390-light]
   shows the question in its first screen; quiz, conclusion and implement states lose the
   two-box stack.
4. **The question and its panel** (findings 2, 3, 8, 11, 12, and the rest of 9). `page.html`,
   `choices.html`, `answer.html`, `pick.html`, `explanation.html`, `layout.css`, the `/pick`
   redirect in `page.rs`, and the door and lead in `QuestionSection` or `QuestionContext`.
   Check: question-1, question-2-blind, -blind-picked and -answer-cancelled, and question-3,
   in all four combinations. The reveal must be visible without scrolling after Pick.
5. **The design of the change** (findings 28, 29, 30, 31, with finding 14 as finding 30
   revises it). `Design` and `DesignPart` in `review-explore/src/design.rs`, the kickoff
   prompt, the pane's design view, `design.html`, `diagrams.js`, `diagrams.css`, `page.js`
   (the scrollspy), and the "finding 28" to "finding 31" blocks of `proposal.css`. It needs
   the tokens (1), the rail (3) and the panel grid (4). It can move up to fourth place if the
   panel styles of finding 8 come with it. Check: question-1 and question-1-unfolded in all
   four combinations; the first 900px at 1280 must hold the change's thesis, the design map
   and part 1's figure; the sequence diagram must fit the reading column at 1280 at its
   natural size. Then check the folded design in question-2-blind, quiz and conclusion.
6. **Tables and citations** (findings 16, 15, 22). `markdown.css`, `citations.css`,
   `citation.html`, `diagrams.css` for the failed diagram, and the dedent in
   `review-explore-citations`. Check: question-1-unfolded, question-3 and the quiz proofs at 390
   and 1280.
7. **Conclusion, implementation and quiz** (findings 10, 18, and the quiz redirect of 12).
   `conclusion.html`, `implement.html`, `quiz.html`, `quiz-item.html`, `quiz-results.html`.
   Check: the eight `implement-*` states, both conclusions, and the three quiz states.
8. **Reading rhythm** (finding 17 for Context and the conclusion; plus 25 in the prompts).
   `explanation.html`, `markdown.css`, and the question prompt. Check that the leads read as a
   summary on their own.
9. **The decision log** (finding 24). It needs new round data, so it comes last. Check:
   conclusion and conclusion-quiz-results, and the rail on every question state.
