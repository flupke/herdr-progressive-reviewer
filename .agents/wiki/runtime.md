# Reviewer runtime

How to record the reviewer's UI timings to diagnose a stall.

## Diagnose UI stalls

Set `HERDR_REVIEWER_TIMINGS` to a JSONL file path when starting the reviewer.
It records event queue delays, handler times and frame render times.
