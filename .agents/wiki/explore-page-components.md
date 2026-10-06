# Explore page: components

The catalogue of the page's building blocks: for each, its stylesheet, its client module, its markup and its rules. Read it before changing the page's markup or styles.

Each component is plain markup with a few classes, styled in one stylesheet of
`crates/review-explore-page/assets`, which the client's modules build with the same
classes. Colours, type, radii and shadows come from the tokens in `tokens.css`
(one meaning per colour: accent where the reviewer acts or what is selected, good for done,
warn for "check before you act", bad for failed or destructive, agent for the agent's
judgement); `h1` to `h4` follow the type scale, `.eyebrow` is the small uppercase label above
what it names, `.hint` the muted help line. Every control has a focus ring (`:focus-visible`).

- **Buttons** (`buttons.css`): `<button class="button primary">`, with one tier among
  `primary` (the one thing to do in the view), `secondary`, `outline`, `quiet` (underlined
  text) and `danger` (only Confirm reset); add `block` for the full width of a panel. A link
  may carry the same classes.
- **Disclosure** (`client/disclosure.js`, styled in `buttons.css`): a button with
  `aria-expanded` that shows or hides an action behind a fold, a muted line with its ▸ by
  default (`<button class="disclosure-toggle" aria-expanded="false">`), or a button of any
  tier (the conclusion's "Start a new round…"). A button rather than `<details>`, so that
  readers and the e2e agent see a control.
- **Status card** (`status.css`), for every state that is not a question:

  ```html
  <div class="status-card warn" id="interruption" role="alert" aria-labelledby="interruption-title">
    <span class="status-glyph" aria-hidden="true"></span>
    <p class="status-title" id="interruption-title">The agent may or may not have your answer</p>
    <p class="status-reason">The review pane was reopened while it sent the prompt.</p>
    <div class="status-bar" aria-hidden="true"></div>   <!-- progress only -->
    <p class="status-next"><strong>Check the agent's pane before you retry.</strong></p>
    <div class="status-actions">
      <form class="retry" data-method="retry">
        <input type="hidden" name="request" value="…">
        <button class="button secondary" type="submit">Retry</button>
        <p class="hint">Sends the same turn again, which could duplicate it.</p>
      </form>
    </div>
  </div>
  ```

  The kind is one class among `progress`, `info`, `warn`, `danger` and `ok`; the glyph comes
  from the stylesheet. A verbatim error in the reason is a `<code>`. Which state shows which
  card, with its words and actions, is data: `StatusCard` in
  [`src/status.rs`](../../crates/review-explore-page/src/status.rs) maps each stage, each
  implementation request and each refused action to its kind, title, reason, next step and
  actions; `assets/client/status.js` only draws it. In a panel, where the primary action
  comes last at its full width, `panelCard` draws the card without its actions and
  `panelActions` draws them as the panel's buttons (the conclusion's implementation request,
  the answer the agent's turn carries). A card that waits says the time since what it waits for
  began (`since`: "Sent 0:42 ago"), which the page counts on every second, in tabular numerals
  and quietly for a screen reader; the tool says when (the start, or the turn's latest attempt
  going out), the page counts.
- **The answer the agent's turn carries** (`sent.css`, `client/sent.js`, from the view's
  `sent`, `SentView` in `src/view/sent.rs`): while the agent works on a turn that carries the
  reviewer's answer, and when that turn did not go through, the turn's status card sits on a
  desk of its own (not above the stage), then the answered question, read again as it read
  before the answer, at full contrast (`questionReading`, "Question 2 · answered"; its
  citations folded or open as the reviewer left them on the question, kept by `citations.js`;
  the project owner's request, where the handoff's capture mutes it); the panel holds the
  question's choices, read only with the one sent selected (`choiceCards(..., { answered })`,
  `QuestionView::keeping`), the comment and tags and what the answer covered (`answer-card.js`,
  `answer-card.css`, as an earlier question's panel shows them), the card's one
  action (Stop waiting, or Retry) and Cancel this answer. The panel comes right after the card
  in the markup, so a phone shows the answer and its action before the question. A turn that
  carries no answer (the kickoff) shows its card above the stage, as before:

  ```html
  <div class="sent desk">
    <div class="status-card progress" id="waiting" role="status">…Sent <span class="status-elapsed">0:42</span> ago…</div>
    <section class="sent-answer panel" aria-labelledby="sent-answer-title">
      <p class="eyebrow" id="sent-answer-title">Your answer to question 2</p>
      <fieldset class="choices answered" disabled><legend class="sr-only">Choices</legend><label class="choice">…<input type="radio" checked>…</label>…</fieldset>
      <div class="answer-card"><p class="answer-comment">“…”</p><p class="answer-tags"><span class="tag accent">changed after your first pick</span></p></div>
      <p class="answer-marked"><span class="check">✓</span> Marked 15 lines reviewed</p>
      <form class="stop" data-method="stop">…<button class="button secondary block">Stop waiting</button><p class="hint">…</p></form>
      <div class="disclosure">…Cancel this answer…</div>
    </section>
    <section class="answered-question" aria-labelledby="question-2-label">…</section>
    <section class="citations" aria-labelledby="question-2-citations">…▸ 1 more citation…</section>
  </div>
  ```
- **Panel and desk** (`layout.css`): the page is capped at 90rem with a 32-pixel gutter (16 on
  a phone). From 70rem, a container with the class `desk` reads in two columns: its children
  in the reading column, each in its own grid row, and its child with the class `panel` (416
  pixels; 480 with `desk wide`) beside them from the first row to the last, sticking in view.
  The panel is a tinted surface with no border; on a phone it runs from edge to edge. From
  89rem (1424 pixels), the open chat stands as a third column at the window's left gutter, and
  the page starts after it (`--chat-room`): the reading column narrows, down to 448 pixels
  (384 beside the conclusion's wider panel), and the panel keeps its width and its place;
  between 70 and 89rem the chat lies over the left of the page and nothing moves. Because
  the grid places the parts, a template keeps one order, which a phone shows as is: a reading
  part after the panel (the question's citations) comes after the reviewer's actions there.

  ```html
  <section class="question desk" aria-labelledby="question-1-title">
    <h2 id="question-1-title">Question 1</h2>
    <div class="text markdown">…</div>
    <form class="answer panel" data-method="answer">…</form>
    <section class="citations">…</section>
  </section>
  ```
- **Start cover** (`style.css`, "start cover"): the status cards come first when a start
  failed or nothing is left to review, then

  ```html
  <section class="start-cover">
    <p class="eyebrow" role="status">No round is running</p>   <!-- only without a card -->
    <h1>The review's title</h1>
    <p class="meta"><code class="revision"><span class="revision-prefix">wq</span><span class="revision-rest">zlpnrt</span></code> in <code>repository</code> · +125 −10 in 4 files</p>
    <form class="start" data-method="start">
      <div class="start-choice">
        <button class="button primary block" type="submit">Start</button>
        <p class="hint">The agent explains the design, then asks one question at a time.</p>
      </div>
      <div class="start-choice">…Start with Challenger…</div>
    </form>
  </section>
  ```
- **Masthead** (`masthead.css`, drawn by `assets/client/masthead.js`), above `main`: the
  chat's place, the product's name and the review's title, the round rail and the ⋯ menu, with
  its hairline across the window as an element of its own, which the meter draws on. The rail
  and the tab title come from the round's overview (`review_explore::RoundOverview`, which the
  session derives in `publish_page` and the view carries as `rail` and `title`): take every
  question number from it, never from a count of the question's versions. "Design ▾" opens the
  design map in place; the menu copies the page's address, opens the agent's conversation, and
  holds Reset, which the page offers nowhere else. Its map links to the design screen
  (`#design`, `#design-part-N`), and while that screen shows, the rail shows Design as current
  (`aria-current="page"`) and the round's own step as the next one (class `next`), which keeps
  `aria-current="step"`. Each done
  question is a link to its earlier question (`#question-N`), which shows the same way; the
  round's own step is then a link back to the stage (`#round`), and a step that is a link
  carries its `aria-current` on the link. Each step names the screen it leads to in
  `data-screen` (`design`, `question-N`, `round`), for the swipe. On a
  phone the rail shows the design and the current step as chips, with the screen in view and
  the screens beside it, which a swipe turns to (class `near`), and the chip a swipe heads to
  fills (class `target`); the review's title moves into the menu. The chat's place is first in
  the row from 70rem, where the bubble stands at the window's left gutter, above the chat it
  opens, and the product's name begins after it (or, with the chat open beside the page, where
  the reading column begins); below 70rem `masthead.js` moves it beside the menu, so that the
  keyboard meets the bubble where the row shows it.

  ```html
  <header class="masthead">
    <div class="masthead-chat" id="masthead-chat">          <!-- the chat's bubble; beside the menu below 70rem -->
      <button class="chat-bubble" aria-label="Talk to the agent" aria-controls="chat"><span class="bubble-shape"></span><span class="chat-badge">1</span></button></div>
    <div class="identity"><p class="product">Explore</p><p class="review">…</p></div>
    <nav class="rail" aria-label="Round"><ol>
      <li class="step done design-step"><button class="design-toggle" aria-expanded="false">…</button>
        <div class="design-map" id="design-map" hidden>…</div></li>
      <li class="step current" aria-current="step">Q2 · working</li>
      <li class="step later">Quiz</li>
    </ol></nav>
    <div class="menu"><button class="menu-toggle" aria-label="Round menu">⋯</button>
      <div class="menu-popover" id="round-menu" hidden>…</div></div>
    <div class="masthead-line" id="masthead-line"></div>   <!-- the meter draws here -->
  </header>
  ```
- **Chat** (`chat.css`, `client/chat.js`, `client/chat-quote.js`): the round's conversation
  with the agent, after `main`. On a desktop (from 70rem) it stands at the window's left gutter,
  under its bubble, as wide as the panel and drawn as the panel is; it starts under the
  masthead, rises with it to the panel's height as the page scrolls (a scroll timeline, or
  `rise()` in a browser without one), and
  reaches the bottom of the window, its messages scrolling on their own above the composer.
  From 89rem the page makes room for it (`layout.css`, "Panel and desk"); between 70 and 89rem
  it lies over the left of the page, with a shadow and no scrim; it never covers the panel. It
  sits under the masthead's layer (`--layer-chat-beside`), so the design map, the ⋯ menu and
  the meter's window open over it, and an Escape that closes the map, the menu, the meter's
  pinned window or a diagram opened large leaves the chat open (the chat hears Escape on the
  window, after every listener of the document). Below the desk's two columns it is a bottom sheet over the
  dimmed page, whose grabber drags it to the full height or closes it. It opens from the bubble, from the menu,
  from "Not ready? Reply to the agent instead" (`requestChat()`), and from "Add to chat", which
  a passage selected in the reading offers and which it quotes. The composer is a form of
  `actions.js` (`send-message`); its draft and quote are kept for the tab. A reply the reviewer
  has not seen counts on the bubble and in the tab's title until the chat shows it, which marks
  it read (`read-messages`):

  ```html
  <aside class="chat open" id="chat" aria-label="Conversation with the agent">
    <header class="chat-head"><h2 class="chat-title">Agent</h2><span class="chat-meta">this round · 3 messages</span><button class="chat-close" aria-label="Close the conversation">×</button></header>
    <div class="chat-log" role="log" aria-label="Messages">
      <article class="chat-message mine" aria-label="Your message"><p class="eyebrow">You · Q2 · 14:21</p><blockquote class="chat-quote-shown">…</blockquote><div class="markdown chat-text-shown">…</div></article>
      <article class="chat-message agent" aria-label="Reply from the agent"><p class="eyebrow">Agent · 14:22</p><div class="markdown chat-text-shown">…</div></article>
      <p class="chat-pending" role="status"><span class="chat-pending-glyph">…</span><span>The agent is answering · <span class="chat-pending-time">0:31</span></span></p>
      <div class="status-card danger" id="conversation-delivery" role="alert">…Retry…</div>
    </div>
    <form class="chat-composer" data-method="send-message" data-requires="text">…<div class="chat-quote">…</div><textarea class="chat-text" aria-label="Message to the agent"></textarea><div class="chat-send"><button class="button outline" type="submit">Send</button><span class="hint">The question stays open. ⌘↵</span></div></form>
  </aside>
  ```
- **Design screen** (`design.css`): the design of the change on the desk, its map in the panel.
  The part in view carries the class `current` (and its map link `aria-current="location"`),
  the parts above it `read`, and the section says its number in `data-part`. On a phone the map
  sits under the thesis, part headers stick to the top, and `.design-bar` is pinned to the
  bottom.

  ```html
  <section class="design-screen desk" aria-label="Design of the change" data-part="1">
    <header class="design-head"><p class="eyebrow">…</p><h2>The change's thesis</h2><p class="meta">4 parts · about 4 minutes · 3 files</p></header>
    <nav class="design-nav panel" aria-label="Design map">… <a class="button primary block" href="#round">Go to question 1</a></nav>
    <section class="design-part" id="design-part-1" aria-labelledby="design-part-1-title">
      <header class="part-head"><span class="n">1</span><h3 class="eyebrow" id="design-part-1-title">What it adds and where</h3><span class="part-of">1 / 4</span></header>
      <p class="thesis">…</p>
      <div class="markdown">…</div>
    </section>
    <div class="design-bar"><p>Current · Question 1</p><a class="button primary block" href="#round">Go to question 1</a></div>
  </section>
  ```
- **Earlier question** (`earlier.css`, `client/earlier.js`, `earlierScreen(question,
  current)`): a question the reviewer answered before, from `PageView.earlier_questions`
  (the overview's `earlier` records, with their citations resolved by the session): on the desk
  the question head ("Question 1 · answered" and its Door chip), Context, the Door and Blast
  radius rows and the citations, as the question screen draws them; in the panel the kept
  answer in a card with its decision tags and what the answers marked (`answer-card.js`), what the agent recorded
  (the previous turn's `recorded` block) and the way back to the round's step (`goTo` of
  `design.js`). It holds no form: nothing on it can change the round.

  ```html
  <section class="earlier-question desk" aria-labelledby="earlier-1-label">
    <header class="question-head"><p class="eyebrow question-eyebrow"><span id="earlier-1-label">Question 1 · answered</span> <span class="chip good">Two-way door</span></p><h2 class="question-text">…</h2></header>
    <section class="earlier-panel panel" aria-labelledby="earlier-1-answer">
      <p class="eyebrow" id="earlier-1-answer">Your answer to question 1</p>
      <div class="answer-card"><p class="answer-choice">…</p><p class="answer-comment">“…”</p><p class="answer-tags"><span class="tag agent">as recommended</span></p></div>
      <p class="answer-marked"><span class="check">✓</span> Marked 12 lines reviewed · 3 lines not relevant</p>
      <div class="earlier-record"><section class="turn-record">…</section><p class="earlier-follow-ups">Follow-ups · …</p></div>
      <a class="button primary block" href="#round">Go to question 3 →</a>
    </section>
    <div class="markdown explanation">…</div>
    <div class="assessments">…</div>
    <section class="citations">…</section>
  </section>
  ```
- **Chips and tags** (`tags.css`): a pill of 12 pixels. A chip names what a thing is, outlined
  in its colour; a tag marks one item of a list, on a tint of its colour. One colour among
  `good`, `warn`, `bad`, `agent`, `accent` and `neutral`. The chip of a question's Door comes
  from `doorChip(door)` in `client/chips.js`, on the question screen and in the design
  screen's panel:

  ```html
  <span class="chip warn">One-way door</span>   <span class="chip agent">Blind pick</span>
  <span class="tag agent">◆ Agent recommends</span>   <span class="tag accent">Your first pick</span>
  ```
- **Previous turn** (`turn.css`, `client/turn.js`, `turnStrip({ answer, response, number })`,
  with `number` the view's `answered`, the rail's number of the question the answer answered):
  one hairlined block, with no frame and no fill, above the stage the turn led to (the first
  row of the question's desk and of the conclusion's). While the agent works on the next turn,
  or that turn waits for Retry, the panel of the answer the turn carries shows the answer and
  Cancel this answer instead; the block sits above the stage only for a turn that carries no
  answer. What the reviewer answered beside what the agent recorded and
  replied, each a region named by its eyebrow; then the follow-ups and Cancel this answer, a
  disclosure (`disclosure.js`) with the quiet button's tier that opens its hint and Confirm:
  cancel my answer. On a phone the columns stack, each `turn-text` clamps to two lines, and the
  follow-ups hide:

  ```html
  <div class="turn">
    <section class="turn-answer" aria-labelledby="turn-answer-title">
      <p class="eyebrow" id="turn-answer-title">You answered Q1</p>
      <div class="turn-text"><p class="turn-choice">…</p><p class="turn-comment">“…”</p></div>
    </section>
    <section class="turn-record" aria-labelledby="turn-record-title">
      <p class="eyebrow" id="turn-record-title">The agent recorded</p>
      <div class="turn-text"><div class="markdown turn-recap">…</div><div class="markdown turn-reply">…</div></div>
    </section>
    <div class="turn-foot">
      <p class="turn-follow-ups">Follow-ups · … · …</p>
      <div class="disclosure">
        <button class="button quiet" type="button" aria-expanded="false" aria-controls="disclosure-1">Cancel this answer…</button>
        <div class="disclosure-body" id="disclosure-1" hidden>
          <form class="cancel-answer" data-method="cancel-answer">…hint…<button class="button secondary" type="submit">Confirm: cancel my answer</button></form>
        </div>
      </div>
    </div>
  </div>
  ```
- **Question head** (`question.css`): the eyebrow with the question's number, which names the
  question's region, its chips, and on a phone the link to the answer; then the question as the
  headline (`h2`, the agent's Markdown):

  ```html
  <header class="question-head">
    <p class="eyebrow question-eyebrow">
      <span id="question-2-label">Question 2</span>
      <span class="chip warn">One-way door</span> <span class="chip agent">Blind pick</span>
      <a class="choices-link" href="#answer">Choices ↓</a>
    </p>
    <h2 class="question-text">…</h2>
  </header>
  ```

  The door's chip: "One-way door" `warn`, "Two-way door" `good`, "Mixed door" and "Door
  unknown" `neutral`; "Blind pick" while the question is blind.
- **Door and Blast radius rows** (`question.css`, each a disclosure of `disclosure.js` whose
  button is a `disclosure-row`, a whole row after its ▸, in `buttons.css`): hairlined rows that
  open; folded, the lead
  (`SectionView::lead_html`, the section's decisive reason) follows the label on one line, cut
  with an ellipsis; open, it wraps and the rest (`details_html`) follows:

  ```html
  <div class="assessments">
    <div class="disclosure assessment">
      <button class="disclosure-row assessment-toggle" type="button" aria-expanded="false" aria-controls="disclosure-2">
        <span class="assessment-title">Door</span><span class="assessment-lead"><p>…</p></span>
      </button>
      <div class="disclosure-body" id="disclosure-2" hidden><div class="markdown">…</div></div>
    </div>
  </div>
  ```
- **Code frame** (`citations.css`, `client/citations.js`, `citation(view, id)`): the note, then
  a `--panel` frame whose head (the citation's heading, which names its region) gives the path
  and the lines; added and removed rows carry a bar in the gutter, and their tint unless every
  row is added (`all-added`). A phone shows one number column, the new line's or, on a removed
  row, the old one's. Citations after the first wait behind a disclosure ("▸ 1 more citation").
  The quiz's proofs use the same frame:

  ```html
  <section class="citation" aria-labelledby="question-2-citation-1">
    <p class="notes">…</p>
    <div class="code-frame">
      <h4 class="code-head" id="question-2-citation-1"><code>src/threads/reply.rs</code> <span>new 20-27</span></h4>
      <div class="code" tabindex="0" role="group" aria-label="Lines of …">
        <table><tbody><tr class="added"><td class="number old"></td><td class="number new">20</td><td class="sign">+</td><td class="line">…</td></tr></tbody></table>
      </div>
    </div>
  </section>
  ```
- **Choice card** (`choices.css`, `choiceCards(choices, { name, legend, picking, revealed })`
  in `client/choices.js`, for a question's choices and a quiz item's answers): a card on the page's surface with an 18-pixel radio on its
  first line. Hover strengthens its frame (`--line-strong`); the selected card takes the
  accent frame, 2 pixels, and an accent tint; the keyboard's focus rings it with a 2-pixel
  outline. The radio is named by `choice-text` alone. A recommended card keeps a neutral frame
  and adds the **recommendation tag**, a `choice-tags` row with the purple tag, and the
  agent's reason under a dashed hairline, which describes the radio; the reviewer's first pick
  of a blind question keeps the accent tag "Your first pick" once the recommendation shows:

  ```html
  <fieldset class="choices">
    <legend class="eyebrow">Choices</legend>
    <label class="choice recommended">
      <input type="radio" name="choice" value="…" aria-labelledby="choice-3" aria-describedby="recommendation-3">
      <span class="choice-text" id="choice-3">…</span>
      <span class="choice-tags"><span class="tag agent">◆ Agent recommends</span></span>
      <span class="choice-reason" id="recommendation-3">…</span>
    </label>
  </fieldset>
  ```
- **Reveal line** (`question.css`, `revealLine(choices)` in `client/question.js`): at the top of the answer panel once the first Send of a
  blind question showed the recommendation; purple when the agent recommends another choice,
  green when both picked the same. Never warn or red: disagreeing is information.

  ```html
  <p class="reveal other" tabindex="-1" data-shows="pick"><strong>The agent recommends another choice.</strong> Read its reason, then keep yours or change it.</p>
  <p class="reveal same" tabindex="-1" data-shows="pick"><strong>You and the agent picked the same choice.</strong></p>
  ```
- **Gain line** (`question.css`, `gainLine(marks, gain)` in `client/question.js`): what answering marks (`MarksView::summary`, a
  `MarkPhrase`: its verb, then each amount, the first in bold) and the reviewed share of the
  change before and after (`GainView`, from the mark tally's `Gain`, rounded as the reviewer's
  file list rounds it), with a bar: reviewed in `--good`, what the answer adds hatched in
  accent. The lines open on request: the whole block is a disclosure's button. Without a share
  (the tool cannot tell), the line shows the amounts alone:

  ```html
  <div class="disclosure gain">
    <button class="disclosure-row gain-toggle" type="button" aria-expanded="false" aria-controls="disclosure-3">
      <span class="gain-line">
        <span class="gain-text">Answering marks <strong>12 lines reviewed</strong> · 3 lines not relevant</span>
        <span class="gain-share">38% → <strong>49%</strong></span>
      </span>
      <span class="gain-bar" aria-hidden="true"><span class="gain-done" style="width: 38%"></span><span class="gain-added" style="width: 11%"></span></span>
    </button>
    <div class="disclosure-body" id="disclosure-3" hidden>
      <ul class="gain-lines"><li>src/threads/reply.rs new 16-27 (reviewed)</li></ul>
    </div>
  </div>
  ```

  The answer panel holds, in order: the blind hint or the reveal line, the choices, the
  comment (`COMMENT · optional`, a box that grows with its text), the gain line, and Send
  answer or Confirm answer, which stays in view at the panel's bottom when a short window makes
  the panel scroll. A form may say what it needs before it can be sent with `data-requires`
  (`choice`, `choice-or-comment`, `answer`; `REQUIRES` in `client/actions.js`): its button stays dimmed
  until then. After an action, the page brings the part it changed into view when the reviewer
  cannot see it (`CHANGED` in `client/actions.js`): the element its module marks with
  `data-shows="<method>"` (the reveal line after a first pick, the verdict on a quiz answer,
  the question after Cancel answer), or the status card after an answer.
- **Quiz** (`quiz.css`, `QuizScreen` in `client/quiz.js`): one item at a time on a desk. The
  head is "QUIZ · QUESTION 2 OF 3" with a dot for each item (named in words for a screen
  reader) and the item as the headline; before Check the panel holds the answers as choice
  cards (`choiceCards`, `data-requires="answer"`) and Check, with a quiet Skip the quiz. After Check the panel
  opens with the verdict (`role="status"`, brought into view), then the answers with their
  marks, each a glyph from the stylesheet and a tag in words (✓ `Correct`, ✗ `Your pick`), then
  Next question (Show the conclusion after the last item); an answer after Check is a choice
  card with its mark in the radio's place. The proof joins the reading column.
  While the quiz shows an item, the page gives the masthead a rail whose quiz step is current
  at that item (`railShowing`), even right after the last one is checked. The results beside
  the conclusion (`quizResults`) reuse the verdict and the marked answers behind a fold, which
  the panel's "See the answers" opens (`openQuizResults`, through `openDisclosure` of
  `disclosure.js`):

  ```html
  <section class="quiz-panel panel" aria-label="Your answer to quiz question 2">
    <p class="verdict bad" role="status" tabindex="-1" data-shows="quiz"><span><strong>Not quite.</strong> …</span></p>
    <p class="eyebrow" id="quiz-answers-title">Answers</p>
    <ol class="quiz-answers" aria-labelledby="quiz-answers-title">
      <li class="choice quiz-answer wrong"><span class="mark" aria-hidden="true"></span><span class="choice-text">…</span>
        <span class="choice-tags"><span class="tag accent">Your pick</span></span></li>
    </ol>
    <button class="button primary block" type="button">Next question <span aria-hidden="true">→</span></button>
  </section>
  ```
- **Meter** (`meter.css`, drawn by `assets/client/meter.js` on the masthead's hairline): how
  much of the change the review marks cover, from `PageView.tally`
  (`review_explore_tally::MarkTally`). The session publishes the tally with each stage, in the
  same change (`RoundPublisher::publish_counted` in `publish_page`), and again from
  `marks_changed`, so a mark by hand or a run of Jev during a round reaches the page at once
  (`RoundPublisher::tally`). At rest the bar is the marked share
  in green; hovered, focused or open it grows and splits into the reviewer's answers, marks by
  hand or from earlier rounds, Jev and not relevant, and what the waiting question marks, and a
  window gives the totals, a legend and a row for each file. The window is as wide as its
  longest path needs, from the handoff's 520px up to 880px, within the viewport's gutters and
  never over the panel beside the reading column; it keeps the width it opened with while it
  stays open. A path that does not fit loses its start to an ellipsis, so that the file's name
  stays (the path sits in a left-to-right `<bdi>`, so that a leading `.` stays in place); each
  row is a list item named by its full path, which is also its title. Past the viewport's
  height the list of files scrolls under the totals and the legend. The strip is a button whose name
  carries the share for a screen reader; Enter or a click pins the window, Escape closes it.
  The start cover takes the change's size from the same tally (`change-size.js`).

  ```html
  <div class="meter open grown">
    <div class="meter-bar" aria-hidden="true"><span class="meter-segment answers"></span>…</div>
    <button class="meter-strip" aria-label="Lines reviewed: 38%, 52 of 135 changed lines"
            aria-expanded="true" aria-controls="meter-window"></button>
    <div class="meter-window" id="meter-window" role="group" aria-label="Review marks of the change">
      <div class="meter-totals">…</div>
      <div class="meter-legend">…</div>
      <div class="meter-files" role="list" aria-label="Files of the change">
        <div class="meter-file" role="listitem" title="src/notify/queue.rs" aria-label="src/notify/queue.rs">
          <span class="meter-path"><bdi dir="ltr"><span class="meter-dir">src/notify/</span><span class="meter-name">queue.rs</span></bdi></span>
          <span class="meter-file-bar">…</span>
          <span class="meter-file-state">18 left · <span class="cited">cited here</span></span>
        </div>
      </div>
    </div>
  </div>
  ```
