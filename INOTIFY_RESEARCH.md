# Filesystem notifications for repository refresh

## Decision

Use filesystem notifications as a refresh trigger, but do not use them as the
source of repository state. Keep `Repository::poll()` unchanged. It already asks
`jj` to snapshot the working copy, reads the identity, files, and statistics,
and rejects a result if the commit changes during the read
([implementation](crates/pr-core/src/repository.rs#L416-L428)).

This change is useful. The current idle loop runs every two seconds
([runtime](crates/pr-app/src/runtime.rs#L34-L35)), and one refresh starts five
`jj` processes: `status`, identity, diff, statistics, and identity verification.
Notifications can remove almost all of this idle work and make common edits
appear sooner.

Do not replace polling completely. On Linux, an inotify queue can overflow and
lose events. The API reports `IN_Q_OVERFLOW`, and its manual tells robust
applications to rebuild their state after event loss
([inotify(7)](https://man7.org/linux/man-pages/man7/inotify.7.html)). On macOS,
FSEvents can report dropped kernel or user events, and Apple says that event
delivery has non-deterministic latency
([Apple FSEvents guide](https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/UsingtheFSEventsFramework/UsingtheFSEventsFramework.html)).

## Minimal design

Add the stable `notify` crate and use `recommended_watcher`. It uses inotify on
Linux and FSEvents on macOS
([official `notify` source](https://github.com/notify-rs/notify/blob/a1d7c2d8f80786679d58ec6d5986a1d4278bc8cf/notify/src/lib.rs#L468-L472)).
Do not add a separate debounce crate.

1. Keep the initial refresh.
2. Watch the workspace root recursively.
3. Treat any successful event as only a hint that state can be stale. Coalesce
   a burst with the existing standard-library channel and one short timer, then
   send one `WorkerCommand::Poll`.
4. On a watcher error or overflow, refresh immediately and recreate the watch.
5. Keep a slow safety refresh, such as every 30 seconds. `notify` states that
   network filesystems can emit no native events and recommends its polling
   backend as a fallback
   ([official crate documentation](https://docs.rs/notify/8.2.0/notify/#known-problems)).

Do not depend on exact event kinds or reconstruct changes from event paths.
The inotify rename pair is not inserted atomically and can be separated or
incomplete ([inotify(7)](https://man7.org/linux/man-pages/man7/inotify.7.html)).
Editors also use different save methods, including truncate and replace
([official `notify` documentation](https://docs.rs/notify/8.2.0/notify/#editor-behaviour)).
Calling the existing `jj` refresh after the burst handles these cases with less
code.

## Paths that matter

For a normal pure-jj workspace, a recursive workspace-root watch includes the
working files and the internal Git backend at `.jj/repo/store/git`. For a
colocated workspace, it also includes `.git`; Jujutsu uses the same working copy
for its colocated Git backend
([Jujutsu workspace source](https://github.com/jj-vcs/jj/blob/32bfcf3ba041be2d37d11e2265c98dea69505d06/lib/src/workspace.rs#L204-L245)).
The `.jj` events are required because a `jj describe` can change the header
without changing a working file. Jujutsu operation heads are files that make a
new repository operation visible
([Jujutsu concurrency design](https://jj-vcs.github.io/jj/latest/technical/concurrency/#storage)).

An added Jujutsu workspace is the exception. Its `.jj/repo` is a file that
points to the repository directory in another workspace
([Jujutsu workspace source](https://github.com/jj-vcs/jj/blob/32bfcf3ba041be2d37d11e2265c98dea69505d06/lib/src/workspace.rs#L370-L390),
[loader](https://github.com/jj-vcs/jj/blob/32bfcf3ba041be2d37d11e2265c98dea69505d06/lib/src/workspace.rs#L568-L593)).
Resolve and watch that repository directory too when it is outside the
workspace root. Otherwise, a repository-only operation from another workspace
can be missed until the safety refresh.

## Recommendation

Implement the hybrid trigger only if the repeated idle `jj` work is a measured
or visible problem. The smallest correct change is one `notify` dependency, one
watch thread, a short coalescing timer, and the existing 30-second safety timer.
Do not change `Repository::poll()`, interpret individual file events, or add
Linux-only inotify code.
