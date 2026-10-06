# Reviewer runtime

What the reviewer does while idle, how its views follow the filesystem, and how to record its UI timings.

## An idle reviewer

An idle reviewer keeps its frame. Input and background updates repaint it; timers also
process deferred file loads and toast deadlines. The conversation worker sleeps until an
event arrives when no comment needs a wakeup.

## Filesystem refresh

The Files view, and the peek at a thread's current file, follow filesystem events (inotify
on Linux), with no periodic repository scan. The displayed source is watched even when
ignore rules exclude its directory; other ignored output stays excluded. A watcher failure
is reported, and reopening the reviewer restores live updates.

## Diagnose UI stalls

Set `HERDR_REVIEWER_TIMINGS` to a JSONL file path when starting the reviewer.
It records event queue delays, handler times and frame render times.
