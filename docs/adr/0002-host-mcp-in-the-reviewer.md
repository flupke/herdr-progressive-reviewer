# Host MCP in the reviewer

Thread access is needed only while the review pane is open. Host Streamable HTTP
directly in the reviewer on localhost. An agent-owned stdio bridge advertises
the tools independently of the reviewer's lifetime and forwards tool calls over
standard MCP HTTP. This lets clients discover tools while the reviewer is closed
and reconnect on the next call after reopening. Stable endpoint addresses avoid
repeated registration. The bridge stores no conversations and does not send
wakeup prompts; Herdr handles those separately.
