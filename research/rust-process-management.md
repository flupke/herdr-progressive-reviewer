# Rust child-process management

Date: 2026-07-30

## Decision

Use real temporary jj repositories for tests of jj behavior. Run the installed
`jj` binary against pure and colocated fixtures. Do not use a fake command
runner to prove command arguments or repository behavior. Small pure tests for
parsers, such as version-string parsing, do not need a repository.

The normal reviewer commands do not justify a custom process-group runner. The
reviewer passes `--no-pager`, and it requests built-in diff output with
`--git`. Jujutsu documents `--no-pager` as the option that disables the pager.
It documents that an external diff program runs only when an external
formatter is selected. Thus, `status`, template-only `log`, built-in `diff`,
and built-in `interdiff` do not normally start descendants for their main
work. Jujutsu can still contact Watchman when the user enables the Watchman
filesystem monitor. That optional configuration is not sufficient reason to
maintain a large custom process supervisor.

Sources:

- [Jujutsu CLI reference: `--no-pager`](https://docs.jj-vcs.dev/latest/cli-reference/#jj)
- [Jujutsu configuration: external diff programs](https://docs.jj-vcs.dev/latest/config/#generating-diffs-by-external-command)
- [Jujutsu configuration: Watchman filesystem monitor](https://docs.jj-vcs.dev/prerelease/config/#watchman)
- [Jujutsu repository: the Git backend uses gitoxide](https://github.com/jj-vcs/jj#compatible-with-git)

If the project keeps a timeout, a capture limit, and Unix process-group
termination, use `subprocess` 1.2 instead of custom threads, polling, and
signals. It supplies timeout capture, allocation limits, `setpgid()`, and a
group-signal operation. It is the closest single-crate match for the current
synchronous and Unix-only implementation. The caller must still send a signal
to the group after a timeout. This crate does not give the same whole-tree
guarantee on Windows.

If the project does not need these safeguards, use
`std::process::Command::output()` directly. This is the smallest design. It
waits for one process and collects stdout and stderr. It has no timeout and no
output-size limit.

Sources:

- [`subprocess` 1.2 crate documentation](https://docs.rs/crate/subprocess/latest)
- [`subprocess` Unix process-group source and API](https://docs.rs/subprocess/latest/src/subprocess/exec.rs.html#827-955)
- [`std::process::Command::output`](https://doc.rust-lang.org/stable/std/process/struct.Command.html#method.output)

## What process-group cleanup means

On Unix, the parent can put `jj` in a new process group. If a time limit
expires, it can send a signal to that group instead of only to the direct `jj`
process. This also stops descendants that stayed in the group. The current
runner does this because a descendant can keep a captured stdout or stderr pipe
open after `jj` exits. In that case, a reader can continue to wait for end of
file.

This protection is useful for commands that intentionally start other
programs, such as a shell, pager, diff tool, editor, formatter, or Watchman
client. It is defensive for the reviewer's fixed built-in jj commands. It is
not evidence that `jj status`, `jj log`, `jj diff --git`, or
`jj interdiff --git` normally create command trees.

Sources:

- [Duct documents the open-pipe problem with grandchildren](https://docs.rs/duct/latest/duct/struct.ReaderHandle.html#method.kill)
- [Jujutsu documents external diff invocation](https://docs.jj-vcs.dev/latest/config/#generating-diffs-by-external-command)

## Crate comparison

| Crate | Result |
| --- | --- |
| `subprocess` | Best fit if all three safeguards stay. It has timeouts, bounded capture, Unix `setpgid()`, and group signaling. Version 1.2.0 was released on 2026-06-23. |
| `process-wrap` | Best dedicated containment library. It is the maintained successor to `command-group` and supports Unix process groups and Windows Job Objects. It does not supply the timeout or capture bound. Current version 9.1 requires Rust 1.87, but this workspace specifies Rust 1.85. |
| `command-group` | Do not add it to new code. Its successor is `process-wrap`; its latest release is 5.0.1 from 2023-11-18. It supplies groups, not bounded capture. |
| `duct` | It has timeout waits and a convenient command API. Its own documentation says that kill does not kill grandchildren. It is not a process-tree solution. |
| `wait-timeout` | It adds only `wait_timeout()` to `std::process::Child`. Its example kills and waits for the direct child. Its Unix implementation installs a `SIGCHLD` handler, which can conflict with another handler. |
| Tokio process | It has asynchronous capture, `kill_on_drop`, and Unix process-group setup. `kill_on_drop` applies to the child, not a complete process tree. Adding Tokio only for these short synchronous jj calls is not justified. |

Sources:

- [`subprocess` features and release metadata](https://docs.rs/crate/subprocess/latest)
- [`process-wrap` overview, platform wrappers, and MSRV](https://docs.rs/crate/process-wrap/latest)
- [`command-group` release metadata](https://docs.rs/crate/command-group/latest)
- [Duct timeout and grandchild behavior](https://docs.rs/duct/latest/duct/struct.Handle.html#method.wait_timeout)
- [`wait-timeout` API and Unix caveat](https://docs.rs/wait-timeout/latest/wait_timeout/)
- [Tokio process command and `kill_on_drop`](https://docs.rs/tokio/latest/tokio/process/struct.Command.html)

## What a combined output bound protects

A combined stdout and stderr bound limits how much child output the reviewer
keeps in memory. For example, a very large diff or a faulty command that writes
without stopping cannot make the reviewer allocate memory without a limit. A
combined 256 MiB bound means that the retained bytes from stdout plus the
retained bytes from stderr cannot exceed 256 MiB.

`std::process::Command::output()` does not provide this bound. It returns stdout
and stderr as two `Vec<u8>` values and collects all output before it returns.
The standard-library API has no size-limit parameter. A bounded
implementation must read the pipes incrementally or use a crate with a capture
limit.

The current runner's bound protects memory, but its implementation continues
to drain and discard data after the limit. It reports the error only after the
streams close. This is more code than the user's stated needs justify. If the
bound stays, `subprocess::Communicator::limit_size()` can replace much of that
custom capture code. If the bound goes, use `Command::output()` and accept that
a large diff can use a large amount of memory.

Sources:

- [`std::process::Output` stores stdout and stderr as byte vectors](https://doc.rust-lang.org/std/process/struct.Output.html)
- [`std::process::Child::wait_with_output` collects remaining output](https://doc.rust-lang.org/std/process/struct.Child.html#method.wait_with_output)
- [`subprocess::Communicator::limit_size`](https://docs.rs/subprocess/latest/subprocess/struct.Communicator.html#method.limit_size)
