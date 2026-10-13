### jj

Checkpoint is the reviewed commit. Diff with jj --ignore-working-copy diff --git -r <checkpoint>,
and read base text with jj --ignore-working-copy file show -r '<checkpoint>-' -- <old-path>,
from the merged parent tree for a merge, or report a limitation when that tree cannot be
established.
