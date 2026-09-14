# Release notes

## 0.2.0

This version requires Herdr 0.7.5 or later and jj 0.43.0 or later.

- Add immediate review-comment posting and chronological agent replies through
  a reviewer-hosted MCP server for Codex and Claude Code. Opening the reviewer
  registers the project's MCP configuration automatically; restart/resume the
  agent and check `/mcp`. Resuming the same agent session retains review access;
  reopening the reviewer sends fresh access for unread comments. This replaces the
  Codex queue integration and starts a fresh conversation store.
- Add AI-generated inline review guides. Use `rf` for one file, `ra` for all
  visible unreviewed files, and `]r` or `[r` to move between guide comments.

## 0.1.0

This version requires Herdr 0.7.5 or later and jj 0.43.0 or later.
