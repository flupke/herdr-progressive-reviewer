//! Which changed lines of one file a reviewer has read.
//!
//! A file's review keeps a *reviewed version*: the base with the changes the
//! reviewer accepted applied, and who accepted each ([`Attribution`]). The
//! open diff runs from the reviewed version to the current file, so a change
//! edited after review shows only what changed since then, and a change
//! nobody reviewed shows whole. Hunks are a view of that diff: accepting part
//! of a hunk splits it. [`HunkReview`] relates the three versions: it tells
//! open hunks from reviewed ones ([`FileHunks`]) and edits the reviewed
//! version when hunks or lines are accepted or reopened.

mod attribution;
mod classify;
mod hunks;
mod lines;
mod patch;
mod review;
mod text;

pub use attribution::{Attribution, AuthoredLines, Reviewed};
pub use hunks::{FileHunks, HunkMark, HunkSpan, LineCount, OpenHunk, ReviewedHunk};
pub use lines::LineSelection;
pub use patch::{reverse_apply, unified_diff};
pub use review::{ChangedLines, HunkReview, ReviewedVersion, replay};
