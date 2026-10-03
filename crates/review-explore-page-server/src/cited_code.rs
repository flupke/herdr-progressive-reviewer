//! The change the fixed questions cite: one Rust file, `src/drafts.rs`, whose `reopen` no
//! longer clears the reviewer's draft.

use review_explore::{CitedLines, CodeLocation, EvidenceRef, SourceSide};
use review_explore_citations::{Citation, CodeColors};
use review_repository::diff::parse_file_diff;
use review_repository::repository::{ChangeKind, ChangedFile, DiffStatistics, FileKind, RepoPath};
use review_source::SourceLineRange;

const PATH: &str = "src/drafts.rs";

const OLD: &str = "\
pub struct Round {
    pub id: RoundId,
}

impl Round {
    pub fn reopen(&mut self) {
        self.draft = None;
    }
}
";

const NEW: &str = "\
pub struct Round {
    pub id: RoundId,
    pub draft: Option<Draft>,
}

impl Round {
    pub fn reopen(&mut self) {
        // The unsent draft stays with the round, so that an answer the reviewer typed before closing the pane is still there when it opens again.
    }
}
";

const DIFF: &str = "\
diff --git a/src/drafts.rs b/src/drafts.rs
--- a/src/drafts.rs
+++ b/src/drafts.rs
@@ -1,9 +1,10 @@
 pub struct Round {
     pub id: RoundId,
+    pub draft: Option<Draft>,
 }
 
 impl Round {
     pub fn reopen(&mut self) {
-        self.draft = None;
+        // The unsent draft stays with the round, so that an answer the reviewer typed before closing the pane is still there when it opens again.
     }
 }
";

/// A citation of `src/drafts.rs` on `side`: `lines`, or the whole file.
pub(crate) fn evidence(side: SourceSide, lines: Option<(u32, u32)>, notes: &str) -> EvidenceRef {
    EvidenceRef {
        location: CodeLocation {
            path: RepoPath::from_bytes(PATH),
            side,
            lines: lines.map(|(first_line, last_line)| SourceLineRange {
                first_line,
                last_line,
            }),
        },
        notes: notes.into(),
    }
}

/// `evidence` with the lines it cites in the change.
pub(crate) fn cite(evidence: &EvidenceRef) -> Citation {
    let lines = CitedLines::in_diff(
        &evidence.location,
        &parse_file_diff(DIFF.as_bytes(), &changed_file()),
        Some(OLD.into()),
        Some(NEW.into()),
    );
    CodeColors::default().cite(evidence.clone(), lines)
}

fn changed_file() -> ChangedFile {
    ChangedFile {
        old_path: Some(RepoPath::from_bytes(PATH)),
        new_path: Some(RepoPath::from_bytes(PATH)),
        old_kind: FileKind::File,
        new_kind: FileKind::File,
        change: ChangeKind::Modified,
        display_path: PATH.into(),
        statistics: DiffStatistics {
            lines_added: 2,
            lines_removed: 1,
        },
    }
}
