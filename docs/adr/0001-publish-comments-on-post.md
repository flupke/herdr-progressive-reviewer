# Publish review comments when posted

Saving comments privately and sending them later can include older, out-of-view
feedback in a batch. Posting a comment will make it immediately available through
MCP; only unposted editor text remains unpublished. This removes the separate
publication step. Posting wakes an idle selected agent through Herdr; an active
agent retrieves new comments through MCP without an additional notification or a
separate submission queue. Unread comments still trigger a wakeup when the agent
becomes idle, but comments already retrieved do not automatically restart an
agent that stopped before replying.
