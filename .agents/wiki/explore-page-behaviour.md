# Explore page: behaviour

What the reviewer can do on the Explore page and what the page guarantees: how it opens, the blind first pick, the actions and their refusals, the chat, the quiz, the conclusion, what survives a reload or a lost connection, and what the pane shows instead. The terms (Explore page, round rail, meter, first pick, earlier question, decision) are defined in [CONTEXT.md](../../CONTEXT.md).

Related pages: the round's own rules are in [Explore round: behaviour](explore-round.md); how the page is built in [Explore page: architecture](explore-page.md), which maps every state of the pane to the page's actions; its markup in [Explore page: components](explore-page-components.md); the network and the tunnel in [Explore page: network and tunnel](explore-page-sharing.md).

## Opening the page

- The Herdr action `herdr.progressive-reviewer.explore-page` ("Open the Explore page of the
  progressive reviewer"), run from the workspace of an open reviewer, opens the page in the
  default browser of the machine that runs the reviewer. `BROWSER` in Herdr's environment
  chooses the program; otherwise the action uses `xdg-open` on Linux and `open` on macOS.
- Start and Start with Challenger in the pane open the same page once the reviewer has
  captured the change, and so does Open the Explore page during a round on the page. Retry,
  Reset, a round started on the page and the pane's other buttons open nothing.
- The action opens the page whatever the setting "Open the page on Start" says.
- On this machine the page is at `127.0.0.1`, on a port chosen when the reviewer starts.
  Its address carries a token: the page refuses a request without it, and a request from
  another site. Closing the reviewer stops the page.
- The reviewer can run the whole round on the page, recoveries included, without the pane.
  Every action saves the same result and sends the agent the same prompt as the pane's.

## What a stage shows

- The page shows the round's current stage: the design, a question with its explanation
  and choices, that the agent is working, that it is no longer working on its turn and
  why, the quiz, the conclusion, or that no round is running.
- Above a question, the quiz or the conclusion, it shows the previous answer beside what
  the agent said back to it: its recap, its reply and the follow-ups it recorded.
- While no round runs, the page names the review as the pane's header does (the change's
  title and revision), its repository and the size of its change, so the pages of two
  reviewers can be told apart.
- Wherever the page shows the revision, it sets apart the prefix that names it, as jj does in
  the pane's header, and dims the rest of the abbreviation; the masthead puts it before the
  title, as the pane's header does. A revision whose prefix jj does not colour apart, such as
  a Git abbreviation, shows plain.

## Starting on the page

- With no round running, the page offers Start and Start with Challenger. The round starts
  as from the pane: the reviewer captures the change, Jev marks first when enabled, and
  the agent gets the kickoff. The page says that it prepares the round, then that the
  agent is working, then opens the round on the design.
- When the reviewer cannot capture the change, the page says why and offers Start again.
- A round started in the reviewer or in another tab after the page loaded wins: the page
  refuses to start a second one, says so, and shows the round as it is.
- When nothing is left to review, both buttons are inactive and a card says why. They
  become active as soon as a line is unmarked, with no reload.

## Answering, and the blind first pick

- An answer is a choice or None of the above, with an optional comment; a comment with no
  choice is an answer too.
- The choice the agent recommends carries a tag and the agent's reason.
- A question whose Door is one-way, mixed or unknown is blind: the page hides the
  recommendation, lists the agent's choices in a mixed order that is the same each time the
  question shows, with None of the above last and none selected.
- The first Send of a blind question sends nothing to the agent. It shows the
  recommendation, a line that says whether the reviewer and the agent picked the same
  choice, and the first pick tagged and selected. The reviewer then keeps or changes the
  choice and confirms.
- A comment typed before the first Send stays in its box, and is part of the answer only
  once the reviewer confirms.
- The reviewer saves the first pick with the answer, for the statistics. The agent
  receives only the answer that was sent.
- A two-way question, or one with no Door, shows the recommendation at once, in the
  agent's order. So does a question asked again after a Cancel answer, and its answer
  keeps no new first pick.
- Above the Send button, the page says how many lines the answer marks reviewed (lines
  found not relevant count with them, as in the meter) and the reviewed share of the
  change before and after; the line opens to list the lines, each with how it is marked.

## After an answer

- The page says that the agent works on the answer, with the time since the answer went
  out, and shows the answered question read only, as it read before the answer, its
  citations open or folded as the reviewer left them.
- The panel shows the question's choices with the sent one selected, the comment, what the
  answer marked, Stop waiting and Cancel this answer. When the turn does not go through,
  it offers Retry in place of Stop waiting.
- The agent's next question or conclusion shows as soon as the agent posts it.

## The actions

- **Stop waiting**, while a round starts or the agent works. A round that is starting is
  dropped. For the agent's turn, the page then says that the agent is not working on it
  and offers Retry.
- **Retry**, when the agent is not working on the turn the round waits for: the prompt
  could not be delivered, the agent did not start on it, the reviewer stopped waiting, or
  the reviewer was reopened during the turn (the page says when the prompt may have reached
  the agent already). It sends the same turn again, the kickoff included.
- **Cancel this answer**, behind a confirmation, under the previous answer and in the panel
  of the answer the turn carries. It is offered until an implementation request is made.
- **Reset**, in the masthead's menu, behind a confirmation. It closes the round for good,
  and the page offers Start again. Once the agent received the implementation request,
  "Start a new round" in the conclusion's panel does the same. The page offers Reset
  nowhere else.
- An action on a state that changed meanwhile, in the reviewer or in another tab, is
  refused: the page says which action did nothing and why, and shows the round as it is.
- An answer is recorded once: when the question already has an answer, the page refuses to
  send another.
- The pane shows the result of each action taken on the page.
- A round that can no longer change (an earlier round, or one whose saved history had to
  be repaired) says so and offers only Reset. When the reviewer cannot save Explore
  rounds, the page says why, and that nothing can be done until the problem is fixed and
  the pane reopened.

## The chat

- The chat is the round conversation, offered on every screen of a round. It opens from
  its bubble, from the menu, from "Not ready? Reply to the agent instead" under the
  conclusion's list ("Reply to the agent" once the agent received the request), and from
  "Add to chat" on a passage selected in the reading, which quotes it.
- A message names where it was written (the design, a question, an earlier question, the
  conclusion) and wakes the agent as a thread comment does. It answers nothing: the
  question stays open and the round does not move.
- The chat says that the agent is answering, with the time since. The reply lands in the
  chat, not above the question.
- A reply the reviewer has not seen counts on the bubble and in the tab's title until the
  chat shows it.
- When a message did not reach the agent, the chat says why and offers Retry, which wakes
  the agent again for the waiting messages.

## The rail, the tab's title and the meter

- The masthead names the review, and its rail shows where the round stands: the design, each question
  asked so far, the quiz (the item it shows, then its score) and the conclusion. The
  current step says when the agent works on it. Each done question opens its earlier
  question, read only: nothing on that screen can change the round.
- The tab's title says whose turn it is ("Q3 · your turn", "Agent working…", "Retry
  needed", "Conclusion"), after the number of chat replies not yet seen.
- The meter's window gives the share reviewed, the lines marked out of the changed lines,
  the size of the change, the lines the answers marked (with their questions), those
  marked by hand or in earlier rounds, by Jev and as not relevant, those the waiting
  question marks once answered, and those left; then a row for each file with its lines
  left, "cited here" beside the files the current question cites.

## What survives

- The page follows the round in every state with no reload: the reviewer sends each
  change, and the page changes in place only what changed, so the text being typed, the
  scroll position, the focus and what the reviewer opened stay.
- Text typed in a box (a comment, the list to be implemented, a chat message) is kept for
  as long as the tab stays open, across a reload too: the box shows it again whenever the
  page shows the same version of that question, that conclusion or the chat. The next
  question starts with an empty box.
- While the page cannot reach the reviewer, a line says that it reconnects, and the
  actions wait with their buttons inactive; nothing is queued. Once the reviewer is back,
  the page shows the round as it is.
- When the reviewer comes back to the page (a phone that wakes, a tab shown again, the
  network back), the page checks its connection at once and opens a new one when needed.

## The design screen

- The round opens on the design: the change's thesis as the headline, with the number of
  parts, about how long they take to read and how many files the change touches, then the
  four parts, each led by its thesis. The design map links to each part, marks the one in
  view, and names the question the round waits for, with the button that goes on to it.
- The round opens on its design once per tab: a reload after the reviewer went on shows
  the question, and when the round moves on in the pane while the page shows the design it
  opened, the page shows the round's new stage.
- Later, the rail's design step opens the map and the design screen, whose button returns
  to where the round stands; the browser's Back returns to the screen before.
- On a phone, a sideways swipe turns between the design, the earlier questions and the
  round's current stage, in the rail's order. A drag that starts in a table, code or a
  diagram that scrolls sideways scrolls it instead.

## The agent's text, citations and diagrams

- A question's explanation is its Context, in the agent's Markdown: paragraphs, lists,
  code, tables whose cells can carry a status mark, and callouts. Door and Blast radius
  are folded to one line, their decisive reason. Raw HTML in the agent's text shows as
  text.
- Citations follow the explanation (and the choices on a phone), in the agent's order,
  each with the agent's note and the cited lines as rows of the diff, with line numbers
  and syntax colours. Citations after the first are folded.
- A citation of a whole file, or of a file that is not text, says so instead of showing
  lines. So does a citation of a file that is neither part of the change nor tracked by
  the repository, such as an ignored `.env`: the page can be open from the network, so it
  shows no other file of the working copy. The pane shows such a file's lines.
- Reading a citation marks no line reviewed.
- The reviewer serves Mermaid itself, so the page needs no internet access. When Mermaid
  cannot read a diagram, the page says so and shows its source, and the reviewer saves the
  error with the question.

## The quiz

- When the conclusion carries a quiz, the page asks it first, one item at a time. After
  Check, the page says at once whether the pick is correct and why, marks the correct
  answer and the pick, and shows the lines that prove it.
- Skip the quiz goes straight to the conclusion; the items left count as skipped.
- The quiz marks no lines. The picks are saved with the round.
- The conclusion's panel then opens with the score, and gives access to the results, item
  by item.

## The conclusion

- The conclusion shows the agent's summary, then the reviewer's decisions: each answered
  question with the kept choice, tagged "as recommended" or "changed after your first
  pick" when it was; then the rest of the summary and the future work.
- The panel holds the list to be implemented, whole, as plain text the reviewer can edit.
  Implement counts the lines that are not blank, authorizes the agent to implement that
  list and nothing else, and sends the same request as the pane's. An empty list is
  refused.
- The page says that the request is being sent, then that the agent received it and when,
  with the list that was sent.
- A conclusion gets one request: once one was sent, from the reviewer or from another tab,
  the page refuses another and shows the one that was sent.
- When the request could not be sent, the page says why and offers Implement again with
  the list. While it is being sent, it can be cancelled if it has not reached the agent.
- A request saved before a reopen and never sent can be sent as saved, or edited and sent
  as a new request. For a request whose delivery is unknown, the page asks the reviewer to
  check the agent's pane before sending a new one. When the agent did not start on the
  request, its list may still wait in the agent's prompt box, and the page offers only
  Retry.

## Layout

- The page lays itself out for the browser's width. On a phone it is one column, and a
  wide table, line of code or diagram scrolls sideways in its own frame. A wider window
  gives tables, cited code and diagrams more room, while paragraphs keep a readable line
  length.
- From about 1120 pixels, a question reads in two columns: the explanation and citations
  on the left; the choices, the comment and Send on the right, in view while the reading
  scrolls. The conclusion with its list, the design with its map, an earlier question with
  its answer and the quiz with its answers do the same.
- On a window of 1424 pixels and more, the open chat is a third column and the reading
  moves over; on a narrower desktop window it lies over the left of the page and leaves
  the choices in view; on a phone it is a sheet over the page.

## What the pane shows instead

- The pane asks no quiz, and a first pick exists only on the page.
- The pane shows the design before question 1, the change's thesis first and each part's
  thesis before its text.
- The pane shows a callout with its title, a marked cell with its mark (✓, ✗ or !), and a
  diagram as its source.
