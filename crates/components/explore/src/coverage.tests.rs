use super::*;
use crate::ExploreComponent;
use component_core::{ComponentEventBus, ComponentTarget};
use ratatui::{buffer::Buffer, layout::Rect};
use review_explore::{Exploration, ExplorePass, Significance, SignificanceResult, SourceSide};
use review_repository::repository::{ChangeKind, ChangedFile, DiffStatistics, FileKind, RepoPath};
use std::sync::Arc;
use ui_actions::Action;
use ui_events::{ExploreCommitted, ExploreCoverageRefresh, ExplorePosted, ExploreRestored};

fn comparison() -> Arc<Comparison> {
    let files = ["a.rs", "b.rs"]
        .into_iter()
        .map(|path| ChangedFile {
            old_path: Some(RepoPath::from_bytes(path.as_bytes())),
            new_path: Some(RepoPath::from_bytes(path.as_bytes())),
            old_kind: FileKind::File,
            new_kind: FileKind::File,
            change: ChangeKind::Modified,
            display_path: path.into(),
            statistics: DiffStatistics::default(),
        })
        .collect();
    Arc::new(Comparison {
        checkpoint: review_source::ReviewCheckpoint::new("review", "checkpoint"),
        repository_root: "/tmp".into(),
        files,
        diffs: vec![
            b"diff --git a/a.rs b/a.rs\n@@ -1 +1,3 @@\n-old\n+one\n+two\n+three\n".to_vec(),
            b"diff --git a/b.rs b/b.rs\n@@ -0,0 +1,2 @@\n+four\n+five\n".to_vec(),
        ],
        context: vec![],
        manifest: vec![],
        sources: vec![],
        base: None,
    })
}

fn lines(file: usize, side: SourceSide, first: u32, end: u32) -> CoverageUnit {
    CoverageUnit::Lines {
        file,
        side,
        first,
        end,
    }
}

fn exclude_second_file(coverage: &mut CoverageLedger) {
    coverage.start_classification("test", "attempt".into(), 1);
    assert!(coverage.record_classification_progress(
        SignificanceResult {
            id: "b".into(),
            units: vec![lines(1, SourceSide::New, 1, 3)],
            outcome: Significance::Insignificant,
            model: None,
            rubric: "test".into(),
            criterion: String::new(),
            input_references: vec![],
            omissions: vec![],
            probabilities: std::collections::BTreeMap::default(),
            confidence: None,
            error: None,
        },
        10
    ));
}

/// The saved ledger a posted answer returns, with `units` credited.
fn credited(coverage: &CoverageLedger, units: &[CoverageUnit]) -> CoverageLedger {
    let mut saved = serde_json::to_value(coverage).unwrap();
    saved["credited"] = serde_json::to_value(units).unwrap();
    saved["revision"] = (coverage.revision() + 1).into();
    saved["counts_revision"] = (coverage.counts_revision() + 1).into();
    serde_json::from_value(saved).unwrap()
}

fn comparison_of(files: usize, diffs: Vec<Vec<u8>>) -> Comparison {
    let mut comparison = (*comparison()).clone();
    comparison.files.truncate(files);
    comparison.diffs = diffs;
    comparison
}

struct Fixture {
    bus: ComponentEventBus<Action>,
    target: ComponentTarget,
    pass: ExplorePass,
}

impl Fixture {
    fn new() -> Self {
        let mut bus = ComponentEventBus::new();
        let target = bus.mount(|events| {
            let mut component =
                ExploreComponent::with_keymap(events, comment_editor::KeymapSetting::default());
            component.jev_enabled = true;
            component
        });
        let mut fixture = Self {
            bus,
            target,
            pass: ExplorePass::new(Exploration::new(comparison())),
        };
        fixture.restore();
        fixture
    }

    fn restore(&mut self) {
        self.bus
            .publish(ExploreRestored {
                result: Ok(Some(Arc::new(self.pass.clone()))),
                view: None,
                historical: false,
                storage_error: None,
            })
            .unwrap();
        self.flush();
    }

    fn flush(&mut self) {
        self.bus.publish(ExploreCoverageRefresh).unwrap();
    }

    fn commit(&mut self) {
        self.commit_unflushed();
        self.flush();
    }

    fn commit_unflushed(&mut self) {
        self.pass.revision += 1;
        let (response, received) = std::sync::mpsc::channel();
        self.bus
            .publish(ExploreCommitted {
                pass: Arc::new(self.pass.clone()),
                applied: true,
                response,
            })
            .unwrap();
        assert_eq!(received.recv().unwrap(), Ok(true));
    }

    fn component(&self) -> &ExploreComponent {
        self.bus.get(self.target).unwrap()
    }

    fn counts(&self) -> &CoverageSnapshot {
        self.component().coverage_cache.get().unwrap()
    }

    fn header(&self) -> String {
        let area = Rect::new(0, 0, 160, 8);
        let mut buffer = Buffer::empty(area);
        self.component()
            .navigation_bar(area)
            .render(&mut buffer, ui_theme::Theme::default().palette);
        buffer
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }
}

#[test]
fn streamed_results_and_required_overrides_refresh_cached_counts() {
    let mut fixture = Fixture::new();
    assert_eq!(
        fixture.counts().lines,
        ChangedLineCoverage {
            explored: 0,
            total: 6
        }
    );
    assert_eq!(fixture.counts().files[0].lines.total, 4);
    assert_eq!(fixture.counts().files[1].lines.total, 2);
    assert!(!fixture.header().contains("Jev filtered"));

    exclude_second_file(&mut fixture.pass.coverage);
    fixture.commit();
    assert_eq!(fixture.counts().lines.total, 4);
    assert_eq!(fixture.counts().files[1].lines.total, 0);
    assert!(fixture.header().contains("Jev filtered 33.3%"));
    fixture
        .pass
        .coverage
        .require_review(vec![lines(1, SourceSide::New, 1, 2)]);
    fixture.commit();
    assert_eq!(fixture.counts().lines.total, 5);
    assert_eq!(fixture.counts().files[1].lines.total, 1);
    assert!(fixture.header().contains("Jev filtered 16.6%"));
    assert_eq!(fixture.component().coverage_cache.rebuilds, 3);
    assert_eq!(fixture.pass.coverage.classifications().count(), 1);
}

#[test]
fn posted_answers_refresh_counts_and_repeated_drawing_reuses_them() {
    let mut fixture = Fixture::new();
    exclude_second_file(&mut fixture.pass.coverage);
    fixture.commit();
    // The posted event carries the authoritative ledger after answer credit.
    fixture.pass.coverage = credited(
        &fixture.pass.coverage,
        &[
            lines(0, SourceSide::Old, 1, 2),
            lines(0, SourceSide::New, 1, 2),
            lines(1, SourceSide::New, 1, 2),
        ],
    );
    fixture.pass.revision += 1;
    let request = fixture
        .pass
        .exploration
        .clone()
        .request(None, None)
        .unwrap();
    fixture
        .bus
        .publish(ExplorePosted {
            request,
            result: Ok(Arc::new(fixture.pass.clone())),
        })
        .unwrap();
    fixture.flush();
    assert_eq!(
        fixture.counts().lines,
        ChangedLineCoverage {
            explored: 2,
            total: 4
        }
    );
    assert_eq!(fixture.counts().files[0].lines.explored, 2);
    assert_eq!(fixture.counts().files[1].lines.explored, 0);
    for _ in 0..20 {
        assert!(
            fixture
                .header()
                .contains("Answered-evidence coverage 50% of required lines")
        );
    }
    fixture.commit();
    fixture.restore();
    assert_eq!(fixture.component().coverage_cache.rebuilds, 3);
    assert_eq!(fixture.pass.coverage.classifications().count(), 1);
}

#[test]
fn completed_policy_and_pass_identity_invalidate_matching_revisions() {
    let mut fixture = Fixture::new();
    exclude_second_file(&mut fixture.pass.coverage);
    fixture.commit();
    fixture.pass.completion = Some(review_explore::ReviewCompletion {
        request: "conclusion".into(),
        baseline: "checkpoint".into(),
        completed: true,
        exclusions_enabled: false,
        summary: fixture.pass.coverage.summary(false),
        unexplored: None,
    });
    fixture.commit();
    assert_eq!(fixture.counts().lines.total, 6);
    assert!(!fixture.header().contains("Jev filtered"));
    fixture.pass.completion.as_mut().unwrap().exclusions_enabled = true;
    fixture.commit();
    assert_eq!(fixture.counts().lines.total, 4);

    let counts_revision = fixture.pass.coverage.counts_revision();
    fixture.pass = ExplorePass::new(Exploration::new(comparison()));
    // An override with nothing excluded leaves counts alone but advances the counts
    // revision to the old pass's value, so only the pass identity differs.
    fixture
        .pass
        .coverage
        .require_review(vec![lines(1, SourceSide::New, 1, 2)]);
    assert_eq!(fixture.pass.coverage.counts_revision(), counts_revision);
    fixture.restore();
    assert_eq!(fixture.counts().lines.total, 6);
    assert_eq!(fixture.counts().filtered_lines, 0);
    assert_eq!(fixture.component().coverage_cache.rebuilds, 5);
}

#[test]
fn empty_metadata_and_incomplete_inventories_preserve_display_semantics() {
    let mode_only = comparison_of(
        1,
        vec![b"diff --git a/a.rs b/a.rs\nold mode 100644\nnew mode 100755\n".to_vec()],
    );
    let ledger = CoverageLedger::new(&mode_only);
    let snapshot = CoverageSnapshot::new(&ledger, &mode_only, true);
    assert_eq!(snapshot.lines.total, 0);
    assert_eq!(snapshot.summary.remaining, 1);
    assert_eq!(snapshot.files[0].lines.total, 0);
    let unchanged = comparison_of(0, vec![]);
    assert_eq!(
        CoverageSnapshot::new(&CoverageLedger::new(&unchanged), &unchanged, true)
            .summary
            .remaining,
        0
    );
    let without_geometry = comparison_of(2, vec![]);
    let ledger = CoverageLedger::new(&without_geometry);
    let snapshot = CoverageSnapshot::new(&ledger, &without_geometry, true);
    assert!(!snapshot.summary.inventory_complete);
    assert_eq!(
        snapshot.summary.limitations,
        ["Saved comparison has no complete diff inventory; start a New pass"]
    );
}

#[test]
fn queued_jev_updates_rebuild_once_from_the_latest_pass() {
    let mut fixture = Fixture::new();
    let original = fixture.counts().lines.total;
    exclude_second_file(&mut fixture.pass.coverage);
    fixture.commit_unflushed();
    let stale = fixture.pass.clone();
    fixture
        .pass
        .coverage
        .require_review(vec![lines(1, SourceSide::New, 1, 2)]);
    fixture.commit_unflushed();
    assert_eq!(fixture.counts().lines.total, original);
    fixture.flush();
    assert_eq!(fixture.counts().lines.total, 5);
    assert_eq!(fixture.component().coverage_cache.rebuilds, 2);

    let (response, received) = std::sync::mpsc::channel();
    fixture
        .bus
        .publish(ExploreCommitted {
            pass: Arc::new(stale),
            applied: true,
            response,
        })
        .unwrap();
    assert_eq!(received.recv().unwrap(), Ok(false));
    fixture.flush();
    assert_eq!(fixture.counts().lines.total, 5);
    assert_eq!(fixture.component().coverage_cache.rebuilds, 2);
}

#[test]
fn question_only_revision_does_not_rebuild_counts() {
    let mut fixture = Fixture::new();
    // Reaffirming no overrides advances only the feedback revision.
    fixture.pass.coverage.require_review(vec![]);
    fixture.commit();
    assert_eq!(fixture.component().coverage_cache.rebuilds, 1);
}

#[test]
fn changing_the_jev_rubric_restores_required_lines() {
    let mut fixture = Fixture::new();
    exclude_second_file(&mut fixture.pass.coverage);
    fixture.commit();
    assert_eq!(fixture.counts().lines.total, 4);

    fixture
        .pass
        .coverage
        .start_classification("new rubric", "next attempt".into(), 1);
    fixture.commit();
    assert_eq!(fixture.counts().lines.total, 6);
    assert_eq!(fixture.counts().filtered_lines, 0);
}

#[test]
fn stopped_filtering_bar_expires_without_excluding_unrecorded_windows() {
    let mut fixture = Fixture::new();
    fixture
        .pass
        .coverage
        .start_classification("test", "attempt".into(), 2);
    fixture.commit();
    assert!(fixture.header().contains("Jev filtering"));

    let now = std::time::SystemTime::now();
    fixture.pass.coverage.stop_classification(100, now);
    fixture.commit();
    assert!(fixture.header().contains("Jev stopped"));
    assert_eq!(fixture.counts().lines.total, 6);
    // A resumed attempt that stopped longer ago than the bar's tail.
    fixture
        .pass
        .coverage
        .start_classification("test", "resumed".into(), 2);
    fixture
        .pass
        .coverage
        .stop_classification(100, now - std::time::Duration::from_secs(6));
    fixture.commit();
    assert!(!fixture.header().contains("Jev stopped"));
    assert_eq!(fixture.counts().lines.total, 6);
    fixture.restore();
    assert!(!fixture.header().contains("Jev stopped"));
}
