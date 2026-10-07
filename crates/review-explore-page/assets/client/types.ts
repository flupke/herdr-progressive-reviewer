// The types of the messages of the Explore page's socket. Generated from the Rust types by `make explore-types`: do not edit.
export type AnswerParams = { 
/**
 * The identity of the round that showed the question, when it has one.
 */
round: string | null, question: string, version: number, 
/**
 * The picked choice's ID; `None` when the reviewer picked none.
 */
choice: string | null, comment: string, 
/**
 * The question's number on the rail, as the page showed it, which a refusal names.
 */
number: number | null, };
/**
 * Where in its round the reviewer wrote a message: under a question, by its identity and
 * version, or at a stage that shows no question.
 */
export type AskedUnder = { "stage": "question", question: string, version: number, number?: number, } | { "stage": "design" } | { "stage": "conclusion", conclusion: string, };
/**
 * The author role of a posted thread message.
 */
export type Author = "reviewer" | "agent";
/**
 * The tier of an action's button (assets/buttons.css).
 */
export type ButtonTier = "primary" | "secondary";
/**
 * What the page asks, with the identities of what it acts on: a `method` and its `params`,
 * on the round, on its conclusion, or in its conversation with the agent.
 */
export type Call = RoundCall | ConclusionCall | ConversationCall;
export type CancelAnswerParams = { answer: string, };
export type CancelImplementationParams = { delivery: string, };
/**
 * A time a card's reason tells: "Sent at 14:36.", in the reader's clock, or "Sent 0:42 ago",
 * counted in the page.
 */
export type CardTime = { 
/**
 * What happened at that time: "Sent at", or "Sent" before the time since then.
 */
words: string, 
/**
 * Milliseconds since the epoch.
 */
ms: number, };
/**
 * One message of the conversation.
 */
export type ChatMessageView = { id: string, author: Author, 
/**
 * The text, as Markdown rendered.
 */
html: string, 
/**
 * When the review received it, in milliseconds since the Unix epoch.
 */
posted_at_ms: number | null, 
/**
 * Where in the round the reviewer wrote it, as the rail names it: "Q2", "Design",
 * "Conclusion"; `None` when it names no step, or one the round no longer shows.
 */
place: string | null, 
/**
 * The passage of the round the reviewer quoted.
 */
quote: string | null, 
/**
 * What became of a message of the reviewer's; an agent's reply has none.
 */
delivery: Delivery | null, 
/**
 * Whether the reviewer has not read this reply.
 */
unread: boolean, };
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
location: string, 
/**
 * The path alone: `src/main.rs`.
 */
path: string, 
/**
 * The side and lines alone: `new 7–9`, or `whole file`.
 */
span: string, notes: string, rows: Array<RowView>, 
/**
 * Why the citation shows no rows.
 */
limitation: string | null, };
/**
 * What the page asks of the round's conclusion.
 */
export type ConclusionCall = { "method": "implement", "params": ImplementParams } | { "method": "quiz", "params": QuizParams } | { "method": "quiz-skip", "params": QuizSkipParams } | { "method": "cancel-implementation", "params": CancelImplementationParams } | { "method": "resend-implementation", "params": ResendImplementationParams };
export type ConclusionView = { 
/**
 * The request of the agent's turn that posted the conclusion.
 */
request: string, 
/**
 * The summary's first paragraph, which leads the conclusion; `None` when the summary
 * opens with something else.
 */
lead_html: string | null, 
/**
 * The rest of the summary, under the reviewer's decisions: its limitations and remaining
 * uncertainty.
 */
summary_html: string, 
/**
 * The reviewer's decisions in the round, in the order of the round rail.
 */
decisions: Array<DecisionView>, future_work_html: string | null, 
/**
 * The list to be implemented that the form starts from, as raw text: the agent's, or the
 * reviewer's own list of a request that was not sent.
 */
draft: string, 
/**
 * How the panel shows the list, with the actions around it.
 */
list: ListView, 
/**
 * The latest implementation request of the conclusion, with its list rendered.
 */
implementation: ImplementationView | null, 
/**
 * What became of it, as a status card with the action that recovers it.
 */
implementation_card: StatusCard | null, 
/**
 * The words that open the reply to the conclusion; `None` when the page offers no actions
 * on the conclusion, as in an earlier round.
 */
reply_label: string | null, 
/**
 * The conclusion's quiz, when it has one.
 */
quiz: QuizView | null, };
/**
 * What the page asks in the round's conversation with the agent, a review thread: the
 * reviewer's messages do not answer the round's questions.
 */
export type ConversationCall = { "method": "send-message", "params": MessageParams } | { "method": "read-messages", "params": ReadParams } | { "method": "retry-messages", "params": RetryMessagesParams };
/**
 * The conversation of the round the page shows.
 */
export type ConversationView = { 
/**
 * The round, by its instance, which a message the reviewer sends names.
 */
round: string, 
/**
 * The messages in posting order.
 */
messages: Array<ChatMessageView>, 
/**
 * How many of the agent's replies the reviewer has not read.
 */
unread: number, 
/**
 * What the page marks read through once the chat showed the replies.
 */
read_through: number, 
/**
 * That the reviewer's waiting messages did not reach the agent, with Retry.
 */
card: StatusCard | null, 
/**
 * Whether the reviewer can write: not in an earlier round, which offers Reset only.
 */
writable: boolean, };
/**
 * How the kept choice relates to the reviewer's first pick and to the agent's recommendation.
 */
export type DecisionTag = "changed_after_first_pick" | "as_recommended";
/**
 * A question the reviewer answered, and the answer kept: a line of "Your decisions".
 */
export type DecisionView = { 
/**
 * The question's number in the round rail.
 */
number: number, 
/**
 * The question's text, the agent's Markdown rendered.
 */
question_html: string, 
/**
 * The text of the kept choice; `None` for a comment-only answer.
 */
choice: string | null, 
/**
 * The reviewer's comment; empty when there is none.
 */
comment: string, 
/**
 * How the kept choice relates to the reviewer's first pick and to the agent's
 * recommendation.
 */
tags: Array<"changed_after_first_pick" | "as_recommended">, };
/**
 * What became of a message the reviewer wrote.
 */
export type Delivery = { "state": "waiting" } | { "state": "not_delivered", error: string, } | { "state": "answered" };
/**
 * One part of the design, under its heading: its thesis, then the rest of its text.
 */
export type DesignPartView = { title: string, 
/**
 * The part's thesis, as inline HTML for one line.
 */
thesis_html: string, body_html: string, };
/**
 * The design of the change, which the design screen shows: it opens the round, and the
 * reviewer can open it again at any later stage.
 */
export type DesignView = { 
/**
 * The change's thesis, as inline HTML for a heading.
 */
thesis_html: string, 
/**
 * The four parts, in reading order.
 */
parts: Array<DesignPartView>, 
/**
 * About how many minutes the design takes to read.
 */
minutes: number, 
/**
 * How many files the change touches.
 */
changed_files: number, };
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
/**
 * Added and removed text lines in one diff.
 */
export type DiffStatistics = { 
/**
 * Number of added text lines.
 */
lines_added: number, 
/**
 * Number of removed text lines.
 */
lines_removed: number, };
export type Door = "one_way" | "two_way" | "mixed" | "unknown";
/**
 * A question the reviewer answered earlier in the round, as it was answered: read only.
 */
export type EarlierQuestionView = { 
/**
 * The question's step on the round rail, from 1.
 */
number: number, 
/**
 * The question's latest version, as the reviewer answered it.
 */
text_html: string, 
/**
 * How hard the decision is to reverse, as the agent assessed it; `None` when it did not.
 */
door: Door | null, 
/**
 * The Context section; `None` when the question has none.
 */
context_html: string | null, 
/**
 * The Door and Blast radius sections, folded to their leads.
 */
sections: Array<SectionView>, 
/**
 * The question's choices, the one the reviewer kept selected, read only.
 */
choices: Array<ChoiceView>, 
/**
 * The answer the reviewer kept; `None` for a question the round left unanswered.
 */
answer: KeptAnswer | null, 
/**
 * What the agent recorded of the answer, and its reply.
 */
recorded: ResponseView, 
/**
 * What the answers to the question marked, one summary for each turn that marked lines:
 * "Marked", then "15 lines reviewed", the lines marked not relevant counted as reviewed.
 */
marks: Array<MarkPhrase>, 
/**
 * The question's citations, most decisive first.
 */
citations: Array<CitationView>, 
/**
 * The question's identity and latest version, which a message of the chat written beside
 * it names.
 */
id: string, version: number, };
export type Field = { name: string, value: string, };
/**
 * One changed file's review marks.
 */
export type FileTally = { 
/**
 * The repository-relative path.
 */
path: string, 
/**
 * The file's changed lines. A file without lines to mark one by one has none.
 */
tally: Tally, 
/**
 * A file without lines to mark one by one (a binary or another non-text change), which
 * is marked or left whole; `None` for a file of text lines.
 */
whole: WholeFile | null, 
/**
 * Whether the question the round waits for cites the file ("cited here").
 */
cited: boolean, };
/**
 * What answering the question the round waits for does to the whole change: "Answering
 * marks 12 lines reviewed · 3 not relevant", "38% → 49%".
 */
export type Gain = { 
/**
 * Open lines it marks reviewed and not relevant.
 */
pending: PendingLines, 
/**
 * Marked lines it reopens.
 */
reopened: number, 
/**
 * The marked share now.
 */
before: Share, 
/**
 * The marked share once the answer's marks apply.
 */
after: Share, };
/**
 * The reviewed share of the change before and after the answer, in whole percent, as the
 * reviewer's file list rounds it: "39% → 50%".
 */
export type GainView = { before: number, after: number, };
export type ImplementParams = { conclusion: string, 
/**
 * The delivery of the conclusion's request that the page showed as not sent; `None` when
 * it showed none.
 */
replaces: string | null, text: string, };
/**
 * An implementation request of the conclusion.
 */
export type ImplementationView = { 
/**
 * The request's delivery identity.
 */
delivery: string, text_html: string, };
/**
 * The agent's recap of the answer, and the follow-ups it recorded.
 */
export type InterpretationView = { recap_html: string | null, follow_ups: Array<string>, };
/**
 * The answer the reviewer kept on a question: the latest answer to its latest version.
 *
 * "None of the above" is a choice: `choice` names it, and it is never "as recommended". A
 * comment-only answer has no `choice`, only its `comment`, and no tag unless the reviewer
 * had made a first pick.
 */
export type KeptAnswer = { 
/**
 * The text of the choice the reviewer sent; `None` for a comment-only answer.
 */
choice: string | null, 
/**
 * The reviewer's comment; empty when there is none.
 */
comment: string, 
/**
 * Each tag that applies, in the order of [`DecisionTag`]: none, one, or both when the
 * reviewer moved to the recommendation after a first pick of another choice.
 */
tags: Array<DecisionTag>, };
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
 * How the panel shows the list to be implemented, and the actions around it besides the card's.
 */
export type ListView = { "kind": "editable" } | { "kind": "draft", html: string, } | { "kind": "request", 
/**
 * What the list is: "Saved request", "The request", "Sent".
 */
label: string, 
/**
 * Whether the request is out of the reviewer's hands, which mutes its list.
 */
settled: boolean, 
/**
 * Whether "Edit before sending" offers to edit the draft and send it in place of the
 * request.
 */
edit: boolean, 
/**
 * Whether "Start a new round…" offers Reset.
 */
new_round: boolean, };
/**
 * A summary of marks in two parts: its verb, and what it marks, each amount on its own, so
 * that a page can set the first amount apart. `verb` names the first part's action: "Marked"
 * when it marks lines, "Reopened" when it only reopens them.
 */
export type MarkPhrase = { verb: string, 
/**
 * "4 lines reviewed", "30 lines not relevant", "reopened 1 line": the parts after the
 * first carry their own verb when it differs from `verb`. Empty when nothing changes.
 */
parts: Array<string>, };
/**
 * The review marks of the change under review: the change as a whole, each changed file, and
 * what answering the question the round waits for adds.
 */
export type MarkTally = { 
/**
 * The whole change: "+125 −10 · 4 files", "38% reviewed · 52 of 135 changed lines".
 */
change: Tally, 
/**
 * Each changed file, in the order of the change.
 */
files: Array<FileTally>, 
/**
 * What answering the question the round waits for does; `None` when no question waits
 * for an answer, or when the code changed since the round started and an answer marks
 * nothing.
 */
gain: Gain | null, };
/**
 * Marked lines by who marked them, in the meter's three groups: `answers`; `jev` and
 * `not_relevant`; `by_hand` and `other_rounds`.
 */
export type MarkedLines = { 
/**
 * Lines an answer of the round settled.
 */
answers: number, 
/**
 * Lines Jev dismissed as too insignificant to need the reviewer's attention.
 */
jev: number, 
/**
 * Lines the round's agent read and found to hold no decision for the reviewer.
 */
not_relevant: number, 
/**
 * Lines the reviewer marked by hand, and lines whose mark names no author.
 */
by_hand: number, 
/**
 * Lines an answer or a turn of another round of the review marked.
 */
other_rounds: number, };
/**
 * Who marked lines, in the groups the meter shows.
 */
export type Marker = "answer" | "jev" | "not_relevant" | "by_hand" | "other_round";
/**
 * What an answer to the question marks: a summary, and the lines on request.
 */
export type MarksView = { 
/**
 * "Answering marks", then "15 lines reviewed": the lines marked not relevant count as
 * reviewed, as in the meter; the lists below keep them apart.
 */
summary: MarkPhrase, reviewed: Array<string>, 
/**
 * Each with why it is not relevant.
 */
not_relevant: Array<string>, reopened: Array<string>, };
export type MessageParams = { 
/**
 * The round the page showed, whose conversation the message joins.
 */
round: string, 
/**
 * The message's identity, a UUID the page chose, so that sending it again posts it once.
 */
id: string, text: string, 
/**
 * Where in the round the page showed the chat: the question and its version, the design,
 * or the conclusion; `None` elsewhere.
 */
asked_under: AskedUnder | null, 
/**
 * The passage of the round the reviewer quoted.
 */
quote: string | null, };
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
 * The number on the rail of the question the latest answer answered, when known.
 */
answered: number | null, 
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
title: TabTitle | null, 
/**
 * How much of the change the review marks cover, for the meter on the masthead's hairline
 * and the start cover's size of the change; `None` until the owner counted them.
 */
tally: MarkTally | null, 
/**
 * The questions the reviewer answered before, each a done step of the rail, in its order.
 */
earlier_questions: Array<EarlierQuestionView>, 
/**
 * The reviewer's answer that the agent's turn carries, while the agent works on the turn
 * or the turn waits for Retry, with the turn's status card.
 */
sent: SentView | null, 
/**
 * The round's conversation with the agent; `None` when no round is running, and when the
 * page offers no conversation.
 */
conversation: ConversationView | null, };
/**
 * The open lines that answering the question the round waits for marks.
 */
export type PendingLines = { 
/**
 * Marked reviewed: the lines the answer settles.
 */
reviewed: number, 
/**
 * Marked not relevant: the lines the agent found to hold no decision.
 */
not_relevant: number, };
export type PickParams = { round: string | null, question: string, version: number, choice: string, 
/**
 * The question's number on the rail, as the page showed it, which a refusal names.
 */
number: number | null, };
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
 * How hard the decision is to reverse, as the agent assessed it; `None` when it did not.
 */
door: Door | null, 
/**
 * The Context section; `None` when the question has none.
 */
context_html: string | null, 
/**
 * The Door and Blast radius sections, folded away until the reviewer opens them, each
 * showing its lead.
 */
sections: Array<SectionView>, 
/**
 * When the page shows the agent's recommendation.
 */
recommendation: Recommendation, 
/**
 * The agent's alternatives, then None of the above; once the recommendation shows after
 * the first pick, the choice picked first is the checked one.
 */
choices: Array<ChoiceView>, 
/**
 * The question's citations, most decisive first.
 */
citations: Array<CitationView>, 
/**
 * The lines an answer marks, `None` when it marks none.
 */
marks: MarksView | null, 
/**
 * The reviewed share of the change before and after the answer; `None` when the tool
 * cannot tell, as when the code changed since the round started.
 */
gain: GainView | null, 
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
export type ReadParams = { round: string, 
/**
 * The position the view said to mark read through.
 */
through: number, };
/**
 * When the page shows the agent's recommendation for a question.
 */
export type Recommendation = "shown" | "hidden_until_pick" | "shown_after_pick";
/**
 * The reply to one request.
 */
export type Reply = { id: number, result: Outcome, } | { id: number, error: RpcError, };
/**
 * A request of the page: the reviewer's action, or a check that the tool is there.
 */
export type Request = { 
/**
 * Matches the reply to the request, on this socket only.
 */
id: number, } & (RoundCall | ConclusionCall | ConversationCall);
export type ResendImplementationParams = { conclusion: string, delivery: string, 
/**
 * The request's latest attempt, as the page showed it.
 */
attempt: string, };
export type ResetParams = { round: string, };
/**
 * What the agent's turn said back to the reviewer's previous answer, and, when run-ahead
 * watched the question, one quiet line on the turn: prepared while the reviewer was thinking,
 * or why not.
 */
export type ResponseView = { interpretations: Array<InterpretationView>, reply_html: string | null, path_line: string | null, };
export type RetryMessagesParams = { round: string, };
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
 * The abbreviated revision identifier, with the prefix jj highlights.
 */
revision: ShortRevision, 
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
 * the page could not have sent, and for one sent after the page's token stopped opening the
 * round, which the socket closes once the replies it waits for are sent.
 */
data: StatusCard | null, };
/**
 * One titled part of the agent's Markdown, rendered: its lead, which shows while the part is
 * folded, then the rest.
 */
export type SectionView = { title: string, lead_html: string, 
/**
 * `None` when the part is its lead alone.
 */
details_html: string | null, };
export type SentView = { 
/**
 * The status card of the turn, whose actions the panel holds.
 */
card: StatusCard, 
/**
 * The number on the rail of the question the answer answers, when known.
 */
number: number | null, 
/**
 * The question the answer answers, as the reviewer answered it; `None` for a reply to the
 * conclusion.
 */
question: QuestionView | null, 
/**
 * The choice and the comment the reviewer sent, and how the choice relates to the first
 * pick and to the agent's recommendation.
 */
answer: KeptAnswer, 
/**
 * What the answer marked: "Marked", then "15 lines reviewed", the lines marked not
 * relevant counted as reviewed; `None` when it marked nothing.
 */
marked: MarkPhrase | null, };
/**
 * The number of a view: the revision of the round's published stage, then how many first
 * picks this page kept for the round, then the revision of the review threads, which hold the
 * round's conversation: the last two change the view without changing the first. Each part
 * only goes up.
 */
export type Seq = { revision: number, picks: number, threads: number, };
/**
 * A share of changed lines that review marks cover: "38% reviewed · 52 of 135 changed lines".
 */
export type Share = { marked: number, changed: number, 
/**
 * `marked` of `changed` in whole percent, as the reviewer shows a file's reviewed share
 * ([`LineCount::percent`]): rounded down, but never 0% once a line is marked nor 100%
 * while one is not; 0 when nothing changed.
 */
percent: number, };
/**
 * An abbreviated revision identifier (a snapshot's `display_id`) as jj highlights it: the
 * shortest prefix that names the revision, then the rest of the abbreviation. An identifier
 * with no prefix coloured apart, such as a Git abbreviation, is all rest.
 */
export type ShortRevision = { 
/**
 * The shortest prefix that names the revision; empty when none is coloured apart.
 */
prefix: string, 
/**
 * The rest of the abbreviation, after the prefix.
 */
rest: string, };
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
id: string, role: StatusRole, title: string, 
/**
 * When the state began, which the reason opens with, in the reader's clock.
 */
time: CardTime | null, reason: string | null, 
/**
 * When what the card waits for began, which the reason ends with as the time since then,
 * counted in the page: "Sent 0:42 ago".
 */
since: CardTime | null, 
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
/**
 * How the changed lines of a file, or of the whole change, stand.
 */
export type Tally = { 
/**
 * The changed lines, added and removed: "+125 −10".
 */
changed: DiffStatistics, 
/**
 * The changed lines a review mark covers, by who marked them.
 */
marked: MarkedLines, 
/**
 * The open lines that answering the question the round waits for marks.
 */
pending: PendingLines, 
/**
 * The marked lines that answering the question the round waits for reopens.
 */
reopened: number, 
/**
 * The open lines the question does not mark: changed, less marked, less pending ("Left
 * to explore"). Unlike the unreviewed lines, they leave out the lines the question marks.
 */
left: number, 
/**
 * The marked share of the changed lines.
 */
share: Share, };
export type TokenView = { text: string, 
/**
 * The palette role of the token's color, or `None` for the page's text color.
 */
role: string | null, };
/**
 * What answering a question does to a file it marks or reopens as a whole.
 */
export type WholeChange = "reviewed" | "not_relevant" | "reopened";
/**
 * A file without lines to mark one by one, marked or left as a whole.
 */
export type WholeFile = { 
/**
 * Who marked the file, in the meter's groups; `None` while it is left.
 */
marked_by: Marker | null, 
/**
 * What answering the question the round waits for does to the file.
 */
answering: WholeChange | null, };
