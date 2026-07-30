---
title: Assemble the MVP implementation specification
status: done
labels:
  - wayfinder:grilling
parent: ../herdr-progressive-reviewer-mvp.md
assignee:
blocked_by:
  - research-herdr-plugin-integration-contract.md
  - validate-the-jj-review-engine.md
  - specify-concurrent-review-state-storage.md
  - prototype-the-ratatui-review-flow.md
---

## Question

How must the resolved Herdr contract, jj review engine, concurrent state protocol, and Ratatui flow fit into one approved implementation specification with acceptance checks and an executable implementation-ticket sequence?

## Answer

The assembled specification and ordered implementation tickets are in
[the MVP implementation specification](../implementation-specification.md).
The integration decision is approved: use `pane.send_text` to insert the
excerpt, then let the user add a comment before submission.
