# Share one review state across the workspaces of a repository

The reviewer named its state directory after the SHA-256 of the canonical workspace root. Each
jj workspace and each Git worktree of one repository kept its own review marks, threads,
drafts and Explore rounds, although they share the repository and its change IDs: a stack
reviewed in the main workspace showed 0% reviewed in a task's workspace.

The state now lives in `<state>/repositories/<slug>-<hash>/`, which every workspace of the
repository shares:

- `<hash>` is the first 16 hex digits of the SHA-256 of the canonical path of the store the
  workspaces share: the directory `.jj/repo` resolves to (a colocated repository is a jj one),
  or the Git common directory.
- `<slug>` is the name of the parent worktree: the jj workspace that holds `.jj/repo`, the Git
  worktree that holds the common `.git`, or a bare repository's own directory. Every byte
  outside `[A-Za-z0-9._-]` becomes `_`, cut to 64 bytes, so a name stays under 81 bytes.
- `repository.json` holds `{"version": 1, "kind": "jj" | "git", "store": "<path>"}`, written
  before any record. A folder whose record names another store is refused
  (`StateCollision`), so a collision of the short hash is an error, never mixed state.
- A repository that moves gets a new key and starts empty, as before. Its old folder still
  names the old path, so a later tool could move it.

Every read-modify-write of a review's marks runs under an exclusive lock of that review,
`changes/<unit>/lock`: reviewers in several tabs, of several workspaces, write the same
marks. Threads, drafts and Explore already took a lock per review.

**Migration.** Each open (the pane, and `reviewer-control stats`) merges the per-workspace
states of its repository still at the root of the state directory (the folders named by 64
hex digits), under a lock of the whole state root (`migration.lock`):

- A state is the repository's when it is the state of one of its current workspaces (`jj
  workspace list`, `git worktree list`), by path, or when one of its reviews is a change the
  repository holds, hidden ones included (jj resolves `<change>/0` for a hidden change, where a
  bare change ID finds visible ones only; the root change, which every repository holds, does
  not count), or a Git tree it holds. A state no repository can place stays where it is,
  untouched; each repository records the states that hold only other repositories' reviews in
  `foreign-workspaces.json`, to check each once (an empty state is checked again at each
  open, at no cost). Two clones of one project share change IDs:
  the first to open claims such a state.
- Merge, record by record: the later `reviewed_at` wins per mark file (a mark removed in one
  workspace but kept in another comes back: the old layout kept no trace of a removal);
  threads are unioned by thread ID, the arriving messages renumbered after the kept ones, with
  their answered and read positions, so pending comments stay pending; drafts are unioned by
  thread; Explore round, view and run-ahead files are copied when missing or newer, keeping
  their time; a history only one state has is kept, and two histories become one read-only
  history of every round in the order they were saved, unless the shared one already lists
  every round of the other. The merge is idempotent. A thread that both states continued
  apart (only a reviewer of an earlier build still running gives one) has the other state's
  new messages last, and waits for an answer as its last comment says.
- A merged state moves to `merged-workspaces/<key>/` with a `merged-into` file naming the
  shared folder. A reviewer of an earlier build that still runs recreates its folder; the next
  open merges it again.

**Backup.** Before the first merge, every per-workspace state is copied to
`<state root>.backups/<UTC time>/` (beside the state root, keeping relative paths), synced,
renamed from `.partial` once complete, with a `RESTORE.md`; then `migration.json` records it.
If the copy fails, nothing is merged or moved, and the pane says so. A later merge of a state
changed since that backup first copies that state and the shared folder into a new backup.
Nothing in the reviewer deletes or reads a backup. Herdr 0.9.3 only creates
`plugins/<plugin id>`, and lists nothing beside it, so the backups' folder there is safe.

**Tabs.** A reviewer stays tied to its Herdr tab: each review comment records the tab it was
posted from, and a tab's agent is woken for, and fetches, only the pending comments of its tab
and those that a Retry, or reopening a thread, in that tab covered. An Explore round started by another workspace's reviewer reopens read-only: its
turns name that workspace's root, and its agent is the other tab's.

Considered:
- Keying on the parent worktree's path: a deleted or moved main workspace would orphan the
  state, and the store's path is what jj and Git themselves share.
- A plain hash as the folder name, as before: the user asked that a reader find the
  repository from the folder.
- Leaving the old states in place, with no merge: the user wanted the existing state gathered.
- Merging only the states of current workspaces, by path: most old states belong to deleted
  task workspaces, whose path no longer names a workspace.
- SQLite for the state (#111): a larger change; locks per review cover the concurrent writers.
