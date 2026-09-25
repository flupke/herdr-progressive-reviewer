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
        checkpoint: review_guide::ReviewCheckpoint::new("review", "checkpoint"),
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
    coverage.restart_classification("test", "attempt".into());
    assert!(coverage.record_significance(SignificanceResult {
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
    }));
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
            let mut component = ExploreComponent::new(events);
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
    assert_eq!(fixture.pass.coverage.classifications.len(), 1);
}

#[test]
fn posted_answers_refresh_counts_and_repeated_drawing_reuses_them() {
    let mut fixture = Fixture::new();
    exclude_second_file(&mut fixture.pass.coverage);
    fixture.commit();
    // The posted event carries the authoritative ledger after answer credit.
    fixture.pass.coverage.credited = vec![
        lines(0, SourceSide::Old, 1, 2),
        lines(0, SourceSide::New, 1, 2),
        lines(1, SourceSide::New, 1, 2),
    ];
    fixture.pass.coverage.revision += 1;
    fixture.pass.coverage.counts_revision += 1;
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
        assert!(fixture.header().contains("Coverage 50% of required lines"));
    }
    fixture.commit();
    fixture.restore();
    assert_eq!(fixture.component().coverage_cache.rebuilds, 3);
    assert_eq!(fixture.pass.coverage.classifications.len(), 1);
}

#[test]
fn completed_policy_and_pass_identity_invalidate_matching_revisions() {
    let mut fixture = Fixture::new();
    exclude_second_file(&mut fixture.pass.coverage);
    fixture.commit();
    fixture.pass.completion = Some(review_explore::ReviewCompletion {
        request: "conclusion".into(),
        baseline: "checkpoint".into(),
        marks: vec![],
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

    let revision = fixture.pass.coverage.revision;
    fixture.pass = ExplorePass::new(Exploration::new(comparison()));
    fixture.pass.coverage.revision = revision;
    fixture.restore();
    assert_eq!(fixture.counts().lines.total, 6);
    assert_eq!(fixture.counts().filtered_lines, 0);
    assert_eq!(fixture.component().coverage_cache.rebuilds, 5);
}

#[test]
fn empty_metadata_and_incomplete_inventories_preserve_display_semantics() {
    let comparison = comparison();
    let mut ledger = CoverageLedger::new(&comparison);
    ledger.inventory.units = vec![CoverageUnit::Item {
        file: 0,
        name: "mode".into(),
    }];
    let snapshot = CoverageSnapshot::new(&ledger, &comparison, true);
    assert_eq!(snapshot.lines.total, 0);
    assert_eq!(snapshot.summary.remaining, 1);
    assert_eq!(snapshot.files[0].lines.total, 0);
    ledger.inventory.units.clear();
    assert_eq!(
        CoverageSnapshot::new(&ledger, &comparison, true)
            .summary
            .remaining,
        0
    );
    ledger.inventory.complete = false;
    ledger.inventory.limitations.push("Diff unavailable".into());
    let snapshot = CoverageSnapshot::new(&ledger, &comparison, true);
    assert!(!snapshot.summary.complete);
    assert_eq!(snapshot.summary.limitations, ["Diff unavailable"]);
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
    fixture.pass.coverage.revision += 1;
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
        .restart_classification("new rubric", "next attempt".into());
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
        .restart_classification("test", "attempt".into());
    fixture.pass.coverage.jev_total_windows = 2;
    fixture.commit();
    assert!(fixture.header().contains("Jev filtering"));

    fixture.pass.coverage.finish_classification(false, 100);
    fixture.commit();
    assert!(fixture.header().contains("Jev stopped"));
    assert_eq!(fixture.counts().lines.total, 6);
    fixture.pass.coverage.classification_stopped_at_ms = fixture
        .pass
        .coverage
        .classification_stopped_at_ms
        .map(|stopped| stopped.saturating_sub(6_000));
    fixture.commit();
    assert!(!fixture.header().contains("Jev stopped"));
    assert_eq!(fixture.counts().lines.total, 6);
    fixture.restore();
    assert!(!fixture.header().contains("Jev stopped"));
}
