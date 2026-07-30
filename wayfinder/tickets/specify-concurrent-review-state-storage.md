---
title: Specify concurrent review-state storage
status: done
labels:
  - wayfinder:grilling
parent: ../herdr-progressive-reviewer-mvp.md
assignee:
blocked_by: []
---

## Question

What exact XDG state layout, repository identity, path encoding, file schema, atomic replacement protocol, and stale-state policy provide persistent local review progress with one independently writable file per repository, stable jj change ID, and reviewed path?

## Answer

The complete layout and protocol are in
[the concurrent state specification](../../research/concurrent-review-state-storage.md).
