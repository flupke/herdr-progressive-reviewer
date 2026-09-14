# Host MCP in the reviewer

Thread access is needed only while the review pane is open. Host Streamable HTTP
directly in the reviewer on localhost, avoiding an additional adapter process and
private forwarding protocol. Accept client connection recovery as a behavior to
validate when the reviewer closes or reopens, and use stable endpoint addresses
to avoid repeated registration.
