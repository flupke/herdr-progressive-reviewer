# Review marks in Files

What marking a file or a hunk does, what reopens a reviewed hunk, what Jev's automatic marking covers, and what unmarking a change resets. The terms (review mark, reviewed version, open hunk, reviewed hunk, unreviewed lines) are defined in [CONTEXT.md](../../CONTEXT.md).

## Files and hunks

- A file marked as reviewed shows no diff until it changes again. A later change shows as a
  new diff since the reviewer's last pass.
- A hunk marked as reviewed folds into one row. The Files list and the diff's title count the
  reviewed hunks, and a partly reviewed file has its own mark in the list.
- A folded hunk can be read again without unmarking it, and unmarked.
- When an edit touches a reviewed hunk, the hunk reopens with only the change since the
  review, labelled "changed since review". Reviewed hunks the edit did not touch stay folded.
- Marking the last open hunk of a file marks the whole file reviewed and moves to the next
  file.
- Binary, conflicted and symbolic-link changes are reviewed per file, never per hunk.
- A build from before hunk marks shows a partly reviewed file as unreviewed.
- A reviewed file hides its code diff and keeps its review threads reachable.

## Automatic marking with Jev

The reviewer's "mark insignificant" command (`rf`, in Files or its diff) asks
[Jev](jev.md) about each unreviewed file's full change in the current comparison,
files changed since an earlier review included.

- A file whose changes are all insignificant is marked reviewed.
- In the other files, each hunk whose changed lines are all insignificant is marked, as a
  hunk mark by hand would.
- A hunk that rewrites lines the reviewer already approved stays open: Jev judges a change
  against the base, not against the reviewed version.
- A file with a mode or type change keeps at least one hunk open, since only a whole-file
  mark covers that change.
- Significant, uncertain, failed, oversized, unclassified and metadata changes stay for
  review.
- The run is in the background, and a notification gives its result. Changing the
  comparison, or changing a review mark by hand, cancels it.
- An automatic mark is made at the checkpoint Jev classified, so a later edit needs review
  again. The reviewer can undo an automatic mark like any other.

## The revision under review

The header names the revision under review: its change ID, then its title. The reviewer can
select another revision, or go to a parent or a child, and read the whole commit message.

## Unmarking a whole change

"Set all files to unreviewed" (`rU`) asks for confirmation, then clears the file review
marks of the current change and cancels a running Jev run. Review threads and the review
marks of other changes stay.
