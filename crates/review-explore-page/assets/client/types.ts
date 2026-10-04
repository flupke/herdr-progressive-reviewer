// The types of the messages of the Explore page's socket. Generated from the Rust types by `make explore-types`: do not edit.
export type AnswerParams = { 
/**
 * The identity of the round that showed the question, when it has one.
 */
round: string | null, question: string, version: number, 
/**
 * The picked choice's ID; `None` when the reviewer picked none.
 */
choice: string | null, comment: string, };
/**
 * The tier of an action's button (assets/buttons.css).
 */
export type ButtonTier = "primary" | "secondary";
/**
 * What the page asks, with the identities of what it acts on: a `method` and its `params`,
 * on the round or on its conclusion.
 */
export type Call = RoundCall | ConclusionCall;
export type CancelAnswerParams = { answer: string, };
export type CancelImplementationParams = { delivery: string, };
/**
 * One choice of a question.
 */
export type ChoiceView = { id: string, text: string, 
/**
 * Why the agent recommends the choice, when it does and the page shows it.
 */
recommendation: string | null, 
/**
 * Whether the choice is selected: the reviewer's first pick.
 */
checked: boolean, };
/**
 * One citation: its location, its note, and the cited lines of the change, colored on the
 * server.
 */
export type CitationView = { 
/**
 * The path, side and lines: `src/main.rs new 7-9`.
 */
location: string, notes: string, rows: Array<RowView>, 
/**
 * Why the citation shows no rows.
 */
limitation: string | null, };
/**
 * What the page asks of the round's conclusion.
 */
export type ConclusionCall = { "method": "implement", "params": ImplementParams } | { "method": "quiz", "params": QuizParams } | { "method": "quiz-skip", "params": QuizSkipParams } | { "method": "reply", "params": ReplyParams } | { "method": "cancel-implementation", "params": CancelImplementationParams } | { "method": "resend-implementation", "params": ResendImplementationParams };
export type ConclusionView = { 
/**
 * The request of the agent's turn that posted the conclusion.
 */
request: string, summary_html: string, future_work_html: string | null, 
/**
 * The list to be implemented that the form starts from, as raw text: the agent's, or the
 * reviewer's own list of a request that was not sent.
 */
draft: string, 
/**
 * The same list rendered, shown when the page does not offer to edit it.
 */
draft_html: string, 
/**
 * Whether the page offers Implement.
 */
offers_implement: boolean, 
/**
 * The latest implementation request of the conclusion, with its list rendered.
 */
implementation: ImplementationView | null, 
/**
 * What became of it, as a status card with the action that recovers it.
 */
implementation_card: StatusCard | null, 
/**
 * Whether the page offers actions on the conclusion: Implement and its recoveries, and
 * Reply. An earlier round offers none.
 */
offers_actions: boolean, 
/**
 * The conclusion's quiz, when it has one.
 */
quiz: QuizView | null, };
/**
 * One part of the design, under its heading: its thesis, then the rest of its text.
 */
export type DesignPartView = { title: string, thesis_html: string, body_html: string, };
/**
 * The design of the change: open above the round's first question, folded away in every later
 * stage of the round.
 */
export type DesignView = { open: boolean, 
/**
 * The change's thesis.
 */
thesis_html: string, 
/**
 * The four parts, in reading order.
 */
parts: Array<DesignPartView>, };
/**
 * A diagram of the question the page showed that Mermaid could not parse.
 */
export type DiagramParams = { question: string, version: number, 
/**
 * The diagram's Mermaid source.
 */
source: string, 
/**
 * Mermaid's message.
 */
message: string, };
export type Field = { name: string, value: string, };
export type ImplementParams = { conclusion: string, 
/**
 * The delivery of the conclusion's request that the page showed as not sent; `None` when
 * it showed none.
 */
replaces: string | null, text: string, };
/**
 * What became of an implementation request, as the page shows it.
 */
export type ImplementationState = { "kind": "sending" } | { "kind": "sent" } | { "kind": "paused" } | { "kind": "unknown" } | { "kind": "not_started" } | { "kind": "not_sent", "reason": string } | { "kind": "cancelled" };
/**
 * An implementation request of the conclusion.
 */
export type ImplementationView = { 
/**
 * The request's delivery identity.
 */
delivery: string, text_html: string, state: ImplementationState, };
/**
 * The agent's recap of the answer, and the follow-ups it recorded.
 */
export type InterpretationView = { recap_html: string | null, follow_ups: Array<string>, };
/**
 * The reviewer's latest answer of a round, which the page offers to cancel as the pane does.
 */
export type LatestAnswer = { 
/**
 * The answer's identity.
 */
id: string, 
/**
 * The text of the choice the reviewer picked, if any.
 */
choice: string | null, 
/**
 * The reviewer's comment; empty when there is none.
 */
comment: string, };
/**
 * What an answer to the question marks: a summary, and the lines on request.
 */
export type MarksView = { 
/**
 * "Will mark 4 lines reviewed · 20 lines not relevant".
 */
summary: string, reviewed: Array<string>, 
/**
 * Each with why it is not relevant.
 */
not_relevant: Array<string>, reopened: Array<string>, };
/**
 * A message the tool sends the page on its own.
 */
export type Notification = { "method": "state", "params": StateParams } | { "method": "ping" };
/**
 * What became of an action the tool carried out.
 */
export type Outcome = { 
/**
 * Whether this request changed the round; false for a repeat of an action applied
 * already, which changes nothing.
 */
applied: boolean, 
/**
 * After a Reset on the network: the token of the start screen, which the page opens with
 * from now on, since the reset round's token opens nothing any more.
 */
reopen: string | null, };
/**
 * The round as the page shows it.
 */
export type PageView = { 
/**
 * The review the page belongs to, once its owner named it.
 */
review: ReviewName | null, 
/**
 * The start cover, when no round is running.
 */
start: StartView | null, 
/**
 * The status cards above the round's stage: that the round is an earlier one, and the
 * stage when it is not a question. A refused action's notice travels in its reply.
 */
cards: Array<StatusCard>, 
/**
 * The reviewer's latest answer, when the page offers to cancel it.
 */
cancellable: LatestAnswer | null, 
/**
 * The design of the change, as the round's first turn explained it.
 */
design: DesignView | null, 
/**
 * What the agent's turn said back to the reviewer's previous answer, above its question or
 * conclusion; `None` when it said nothing.
 */
response: ResponseView | null, question: QuestionView | null, conclusion: ConclusionView | null, 
/**
 * The round Reset closes, when the page offers it.
 */
reset: string | null, 
/**
 * Whether the round is an earlier one, which the reviewer can only Reset.
 */
earlier: boolean, 
/**
 * The address of Mermaid's script, which draws the diagrams.
 */
mermaid: string, 
/**
 * The steps of the round rail, in order; empty when no round is running.
 */
rail: Array<RailStep>, 
/**
 * What the browser tab's title says before the review's name; `None` when no round is
 * running, and when the review tool cannot save the round, which then waits for nothing.
 */
title: TabTitle | null, };
export type PickParams = { round: string | null, question: string, version: number, choice: string, };
export type QuestionView = { 
/**
 * The identity of the round that asks the question, which the answer carries back: a
 * question of the same ID in a later round is another question.
 */
round: string | null, 
/**
 * The question's step on the round rail, from 1: a clarified question keeps its number.
 */
number: number, id: string, version: number, text_html: string, 
/**
 * The Context section; `None` when the question has none.
 */
context_html: string | null, 
/**
 * The Door and Blast radius sections, folded away until the reviewer opens them.
 */
sections: Array<SectionView>, 
/**
 * When the page shows the agent's recommendation.
 */
recommendation: Recommendation, 
/**
 * The agent's alternatives, then None of the above.
 */
choices: Array<ChoiceView>, 
/**
 * The text of the choice the reviewer picked first, once the recommendation shows.
 */
first_pick: string | null, 
/**
 * The question's citations, most decisive first.
 */
citations: Array<CitationView>, 
/**
 * The lines an answer marks, `None` when it marks none.
 */
marks: MarksView | null, 
/**
 * Whether the page offers to answer: not in an earlier round.
 */
answerable: boolean, };
export type QuizAnswerView = { 
/**
 * The option's position, from 0, as a pick sends it.
 */
index: number, text: string, correct: boolean, picked: boolean, };
/**
 * One quiz item, and the reviewer's pick once there is one.
 */
export type QuizItemView = { 
/**
 * The item's position, from 1.
 */
number: number, question: string, answers: Array<QuizAnswerView>, 
/**
 * Whether the reviewer picked the correct option, once the reviewer picked one.
 */
picked_correct: boolean | null, 
/**
 * The correct option's text.
 */
correct_answer: string, why: string, proof: Array<CitationView>, };
export type QuizParams = { conclusion: string, 
/**
 * The item, from 0.
 */
item: number, 
/**
 * The option picked, from 0.
 */
answer: number, };
export type QuizSkipParams = { conclusion: string, };
/**
 * How far the reviewer is in the quiz.
 */
export type QuizStage = { "kind": "later" } | { "kind": "running", item: number, items: number, } | { "kind": "scored", correct: number, answered: number, items: number, };
/**
 * A conclusion's quiz: an item at a time before the conclusion, until the reviewer answered or
 * skipped every item, then the results beside the conclusion.
 */
export type QuizView = { items: Array<QuizItemView>, 
/**
 * The item the reviewer answers next, from 0; `None` once the quiz is done, or when the
 * round cannot save answers.
 */
next: number | null, 
/**
 * How many picks were correct, and how many items have a pick.
 */
correct_picks: number, picked: number, 
/**
 * Whether the reviewer skipped the items that have no pick.
 */
skipped: boolean, };
/**
 * One step of the round rail and its state.
 */
export type RailStep = { step: Step, state: StepState, };
/**
 * When the page shows the agent's recommendation for a question.
 */
export type Recommendation = "shown" | "hidden_until_pick" | "shown_after_pick";
/**
 * The reply to one request.
 */
export type Reply = { id: number, result: Outcome, } | { id: number, error: RpcError, };
export type ReplyParams = { conclusion: string, text: string, };
/**
 * A request of the page: the reviewer's action, or a check that the tool is there.
 */
export type Request = { 
/**
 * Matches the reply to the request, on this socket only.
 */
id: number, } & (RoundCall | ConclusionCall);
export type ResendImplementationParams = { conclusion: string, delivery: string, 
/**
 * The request's latest attempt, as the page showed it.
 */
attempt: string, };
export type ResetParams = { round: string, };
/**
 * What the agent's turn said back to the reviewer's previous answer.
 */
export type ResponseView = { interpretations: Array<InterpretationView>, reply_html: string | null, };
export type RetryParams = { request: string, 
/**
 * The turn's latest attempt, as the page showed it.
 */
attempt: string, };
/**
 * The review a page belongs to, as the pane's header names it, with the repository: two
 * reviewers' pages can be told apart.
 */
export type ReviewName = { 
/**
 * The name of the repository's directory.
 */
repository: string, 
/**
 * The abbreviated revision identifier, without colors.
 */
revision: string, 
/**
 * The first line of the change's description; empty when it has none.
 */
title: string, };
/**
 * What the page asks of the round.
 */
export type RoundCall = { "method": "answer", "params": AnswerParams } | { "method": "pick", "params": PickParams } | { "method": "start", "params": StartParams } | { "method": "stop", "params": StopParams } | { "method": "retry", "params": RetryParams } | { "method": "cancel-answer", "params": CancelAnswerParams } | { "method": "reset", "params": ResetParams } | { "method": "diagram-failed", "params": DiagramParams } | { "method": "ping" };
export type RowKind = "context" | "added" | "removed";
/**
 * One row of the cited lines.
 */
export type RowView = { kind: RowKind, old_line: number | null, new_line: number | null, tokens: Array<TokenView>, };
/**
 * Why the tool did not carry out a request.
 */
export type RpcError = { code: number, message: string, 
/**
 * The notice the page shows, worded for the action, as a status card; `None` for a request
 * the page could not have sent.
 */
data: StatusCard | null, };
/**
 * One titled part of the agent's Markdown, rendered.
 */
export type SectionView = { title: string, body_html: string, };
/**
 * The number of a view: the revision of the round's published stage, then how many first
 * picks this page kept for the round, which change the view without changing the revision.
 */
export type Seq = { revision: number, picks: number, };
export type StartParams = { 
/**
 * Whether the Challenger reviews the change beside the agent.
 */
challenger: boolean, 
/**
 * The identity of the start the page offered.
 */
start: string, };
/**
 * The start cover, when no round is running.
 */
export type StartView = { 
/**
 * The identity of the start the cover offers, which a Start carries back.
 */
start: string, 
/**
 * Whether no status card says the cover's state, so that the cover says that no round is
 * running.
 */
idle: boolean, 
/**
 * The ID of the status card that says why the reviewer cannot start a round: the start
 * buttons are inactive, and it describes them.
 */
block: string | null, };
/**
 * The whole view, numbered: `seq`, compared as a pair, goes up with each change the page shows,
 * and starts again with a new `epoch` when the tool restarts.
 */
export type StateParams = { epoch: string, seq: Seq, view: PageView, };
/**
 * An action of a card: a request the page sends.
 */
export type StatusAction = { 
/**
 * The method of the page's request, which also names its form: `retry` sends a Retry.
 */
method: string, 
/**
 * The identities the request carries.
 */
fields: Array<Field>, label: string, tier: ButtonTier, 
/**
 * A short line beside the button.
 */
hint: string | null, };
/**
 * One state that is not a question: the state first, then the next step.
 */
export type StatusCard = { kind: StatusKind, 
/**
 * Names the card, once on the page.
 */
id: string, role: StatusRole, title: string, reason: string | null, 
/**
 * Whether the reason is a verbatim error, shown as code.
 */
code: boolean, 
/**
 * A next step the reviewer must not miss, in bold before `next`.
 */
imperative: string | null, next: string | null, 
/**
 * In a row under the text, primary first.
 */
actions: Array<StatusAction>, };
/**
 * What a card tells, by its colour and glyph.
 */
export type StatusKind = "progress" | "info" | "warn" | "danger" | "ok";
/**
 * How a reader is told of a card.
 */
export type StatusRole = "status" | "alert" | "note";
/**
 * A step of the round: the design, a question, the quiz or the conclusion.
 */
export type Step = { "kind": "design" } | { "kind": "question", number: number, } | { "kind": "quiz", stage: QuizStage, } | { "kind": "conclusion" };
/**
 * Where a step stands in the round. The rail tells the round's state, not the screen the
 * reviewer looks at: while the round waits for an answer to question 1, Design is done and
 * question 1 current, and the page shows that the reviewer reads the design.
 */
export type StepState = { "kind": "done" } | { "kind": "current", 
/**
 * The agent works on the turn after the step ("Q2 · working"). False while the round
 * waits for the reviewer, and while a turn waits for Retry.
 */
working: boolean, } | { "kind": "later" };
export type StopParams = { 
/**
 * The start the page showed under way, by its identity.
 */
start: string | null, 
/**
 * The agent's turn the page showed the agent working on.
 */
request: string | null, };
/**
 * What the tab title says before " — <review>", so that a reviewer who left the tab sees whose
 * turn it is. The quiz belongs to the conclusion's stage.
 */
export type TabTitle = { "kind": "your_turn", question: number, } | { "kind": "agent_working" } | { "kind": "retry_needed" } | { "kind": "conclusion" };
export type TokenView = { text: string, 
/**
 * The palette role of the token's color, or `None` for the page's text color.
 */
role: string | null, };
