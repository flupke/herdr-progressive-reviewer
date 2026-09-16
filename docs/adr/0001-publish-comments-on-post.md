# Publish review comments when posted

Saving comments privately and sending them later can include older, out-of-view
feedback in a batch. Posting a comment will make it immediately available through
MCP; only unposted editor text remains unpublished. This removes the separate
publication step. Posting wakes an idle selected agent through Herdr; an active
agent retrieves new comments through MCP without an additional notification or a
separate submission queue. Pending comments still trigger a wakeup when the agent
becomes idle. Reading never acknowledges them: each successful reply acknowledges
comments through its fetched `in_reply_to` message ID, in the same durable update
that appends the reply. Later comments remain pending, and retries reuse the
same message ID, text and snapshot boundary.

Returning to idle does not repeat an already sent wakeup, even if the agent stops
before answering. The reviewer can use **Retry agent** without posting another
comment. Reopening the reviewer also restores pending work. This keeps delivery
recoverable without creating an automatic notification loop.
