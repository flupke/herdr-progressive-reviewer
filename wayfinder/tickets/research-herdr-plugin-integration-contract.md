---
title: Research the Herdr plugin integration contract
status: done
labels:
  - wayfinder:research
parent: ../herdr-progressive-reviewer-mvp.md
assignee: research-herdr-contract
blocked_by: []
---

## Question

Which Herdr manifest, lifecycle actions, pane APIs, and chat-input APIs must the MVP use to open and close its Ratatui pane and insert text, without submission, into the most recently focused agent chat in the same workspace? Define the exact behavior when no agent chat exists and confirm the constraints for pure jj and colocated workspaces.

## Expected asset

`research/herdr-plugin-integration-contract.md`

## Answer

The resolved contract is in
[the Herdr integration research](../../research/herdr-plugin-integration-contract.md).
Herdr v1 has no chat-draft API. The MVP must use checked `pane.send_text`
insertion and must never use the submitting `agent.prompt` operation.
