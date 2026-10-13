# Review someone else's change in a round of its own

An Explore round assumed that its reviewer wrote the change: the agent explained the design
to someone who knew why the change exists, every answer was the reviewer's decision, and
Implement made the agreed changes. Reviews of other people's pull requests did not fit. A
teammate who reviewed the user's change was lost from the first question, met an inferred
motive presented as the change's purpose, and could leave nothing to the author; the user's
own reviews of others' changes rewrote Implement into "post comments to the PR".

The reviewer now says at Start whose change it is (`ChangeAuthor`, kept by the round and each
turn's request). A round on someone else's change gives the agent its own rules (story first,
guesses about intent become questions for the author, an unclear description is a finding,
the reviewer's chat questions set the order), requires the choice `ask-author` on every
question, and ends in the review to post: Post review, in place of Implement, asks the agent
to post the edited list on the change's pull request, and nothing else. The reviewer may give
a name, which the kickoff carries.

Considered:
- Detecting the author from version control: the author of a commit the reviewer checked out
  says nothing about who reviews it, and the user preferred to say it.
- Prompt rules alone: Implement would still ask for code changes, and nothing would make every
  question leave room for the author.
- A separate tool to post review comments: the reviewer's edited list already reaches the
  agent through Implement's durable delivery; a second path would duplicate its recovery.
