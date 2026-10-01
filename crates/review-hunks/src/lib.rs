//! Which hunks of one file a reviewer has read.
//!
//! A file's review keeps a *reviewed version*: the base with the hunks the
//! reviewer accepted applied. The open diff runs from the reviewed version to
//! the current file, so a hunk edited after review shows only what changed
//! since then, and a hunk nobody reviewed shows its whole change.
//! [`HunkReview`] relates the three versions: it tells open hunks from
//! reviewed ones ([`FileHunks`]) and edits the reviewed version when a hunk is
//! accepted or reopened.

mod classify;
mod hunks;
mod patch;
mod review;
mod text;

pub use hunks::{FileHunks, HunkCount, HunkMark, HunkSpan, OpenHunk, ReviewedHunk};
pub use patch::{reverse_apply, unified_diff};
pub use review::{ChangedLines, HunkReview, ReviewedVersion, replay};
