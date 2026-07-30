---
title: Validate the jj review engine
status: done
labels:
  - wayfinder:prototype
parent: ../herdr-progressive-reviewer-mvp.md
assignee:
blocked_by: []
---

## Question

What is the smallest command protocol and parsed data model that correctly polls and snapshots `@`, detects stable change-ID switches, lists the current change's files, renders full diffs and per-file interdiffs, and handles rebases, missing baselines, conflicts, binary files, deletion, and rename output in both pure and colocated jj workspaces?

## Answer

The command transaction, data model, failure behavior, and fixture matrix are
in [the jj review engine protocol](../../research/jj-review-engine.md).
