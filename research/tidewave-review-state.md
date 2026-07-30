# Tidewave review-state cache

Date: 2026-07-30

## Conclusion

Tidewave stores one pair of Git indexes for each review session:

- `review-index-<timestamp>` is the comparison baseline.
- `working-index-<timestamp>` is a snapshot of the working tree.
- `objects/` stores loose, content-addressed Git blobs that the indexes reference.

In this cache, every review index exactly matches a real commit tree. The
working index contains later tracked changes and untracked files. Thus, the
supported model is:

```text
review diff = review-index baseline -> refreshed working-index snapshot
```

The cache does not prove that Tidewave moves one file in the review index when
the user marks that file as reviewed. None of the nine review indexes contains
a mixed, per-file baseline: each one matches one complete commit tree.

For Herdr, do not copy this pair-per-session design if review state must be
shared between concurrent sessions. Use one atomic record for each
`(repository, jj change ID, path)`. Store either the reviewed file content or a
content-addressed blob ID. A full content snapshot is the smaller MVP unless
the implementation already has a durable object store.

## Primary evidence

The inspected source is the local cache:

```text
/home/flupke/.cache/tidewave/hubert/git/
```

`file` identifies all 18 state files as Git index version 2 files. There are
nine pairs. Their suffix is a millisecond Unix timestamp. For example,
`1785435636055` is `2026-07-30 20:20:36 +0200`.

The following command reads an index without changing the repository:

```sh
GIT_INDEX_FILE="$index" git ls-files --stage
```

The pair comparison showed these results:

| Pair suffix | Review entries | Working entries | Difference |
| --- | ---: | ---: | --- |
| `1785015181750` | 49 | 49 | none |
| `1785015182730` | 49 | 49 | `flake.nix` blob changed |
| `1785015370274` | 49 | 49 | `flake.nix` blob changed |
| `1785015549214` | 49 | 49 | `flake.nix` blob changed again |
| `1785016548283` | 71 | 71 | two blobs changed |
| `1785251553005` | 71 | 71 | 12 blobs changed |
| `1785251843817` | 71 | 71 | the same 12 blobs changed |
| `1785251906514` | 93 | 93 | none |
| `1785435636055` | 93 | 94 | untracked file `foo` added |

For the newest pair, the real repository index and the review index have the
same SHA-256 digest over `git ls-files --stage` output:

```text
real repository index: 6e7d64c43b73086de08bf7eaca0369a04f7186c374dd7c268c9c334a1ddadc2d
review index:          6e7d64c43b73086de08bf7eaca0369a04f7186c374dd7c268c9c334a1ddadc2d
working index:         1e02cc5428d90d38613822cefd58c2701575dbe8c37857a90ea18ad5e717c44d
```

The repository reports `foo` as untracked. The working index records it with
blob ID `f69bbb831db34940bb75cb79245529baeb5d304b`, which is also the result of:

```sh
git -C /home/flupke/src/hubert hash-object foo
```

I compared each review index with every reachable commit tree:

```sh
git -C /home/flupke/src/hubert ls-tree -r \
  --format='%(objectmode) %(objectname) 0%x09%(path)' "$commit"
```

All nine review indexes matched a complete commit:

```text
1785015181750 -> a4e96e5d9c27a2da672f02be612ef92c4a553fb9
1785015182730 -> a4e96e5d9c27a2da672f02be612ef92c4a553fb9
1785015370274 -> a4e96e5d9c27a2da672f02be612ef92c4a553fb9
1785015549214 -> a4e96e5d9c27a2da672f02be612ef92c4a553fb9
1785016548283 -> 307b30c1b2e244836f13d247e1ee4034476038e9
1785251553005 -> d25d67a4d8edd194ac1d8be17410b4ef35cd8941
1785251843817 -> d25d67a4d8edd194ac1d8be17410b4ef35cd8941
1785251906514 -> 730f15fc721d7a1475d6cfd216b19bc1ba3a8913
1785435636055 -> 730f15fc721d7a1475d6cfd216b19bc1ba3a8913
```

## Update timing

The file suffix records the session start time. The filesystem birth times
show that Tidewave later replaces some index files:

```text
review-index-1785251906514
  suffix time: 2026-07-28 17:18:26 +0200
  birth/mtime: 2026-07-30 17:09:06 +0200

working-index-1785435636055
  suffix time: 2026-07-30 20:20:36 +0200
  birth/mtime: 2026-07-30 20:59:27 +0200
```

The newest working-index replacement and the new `foo` blob have the same
second-level timestamp. This is evidence that Tidewave refreshes the working
index when it observes a working-tree change. The available cache does not
show whether this uses a file watcher, polling, or a UI refresh.

## Objects directory

The `objects/xx/yyyy...` layout and the 40-hex object IDs in each index are the
standard loose Git object format. The cache has 1,243 loose object files. Of
the 153 unique object IDs referenced by the surviving indexes, 70 are present
in this cache and are Git blobs. The other IDs are available from the real
repository object database. This indicates that Tidewave uses its cache for
working content while it can reuse repository objects for committed content.

## Concurrency implications

Observed facts:

- Each session has its own timestamp-named pair, so normal concurrent sessions
  do not write the same index files.
- The shared object directory is content-addressed, so equal content has the
  same destination.
- Old suffixes with new birth times show whole-file replacement. This is
  consistent with atomic temporary-file-and-rename updates.

Inferences and limits:

- A collision is possible if two sessions start in the same millisecond.
- Atomic replacement protects readers from a partial index, but two writers
  for the same pair still have last-write-wins behavior.
- Separate pairs mean that Tidewave review baselines are session-local. They
  do not supply the shared per-file semantics requested for Herdr.
- A per-file Herdr state layout reduces unrelated write conflicts. It still
  needs atomic replacement for two sessions that mark the same file at once.
