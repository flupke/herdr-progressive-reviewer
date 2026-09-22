# Publish review comments when posted

Saving comments privately and sending them later can include older, out-of-view
feedback in a batch. Posting a comment will make it immediately available through
MCP; only unposted editor text remains unpublished. This removes the separate
publication step. Posting notifies the selected agent through Herdr's `agent.prompt`,
including while it is working. Thread history and exact reply acknowledgements
track pending work, so notifications do not need terminal parsing, focus checks,
or agent-turn tracking. Herdr owns submission; a delivery error leaves the comments
available for an explicit retry. Reading never acknowledges them: each successful
reply acknowledges comments through its fetched `in_reply_to` message ID, in the
same durable update that appends the reply. Later comments remain pending, and
retries reuse the same message ID, text and snapshot boundary.

Agent status changes do not repeat an already attempted notification, even if the
agent stops before answering. The reviewer can use **Retry agent** without posting
another comment. Reopening the reviewer also restores pending work. This keeps
delivery recoverable without creating an automatic notification loop.

Filename and diff-selection insertion shortcuts are removed. Reviewer feedback is
published through review threads, and automated prompts use `agent.prompt`.
