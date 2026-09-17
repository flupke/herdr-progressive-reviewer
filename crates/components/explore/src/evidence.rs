//! Decision evidence and optional supporting sources share native viewers.
use super::{
    Control, ExploreComponent,
    flow::{Content, ConversationLayout},
};
use diff_component::DiffComponent;
use review_explore::SourceSide;
use ui_events::EvidenceView;
use ui_theme::Palette;

impl ExploreComponent {
    pub(super) fn evidence_block(
        &self,
        index: usize,
        layout: &mut ConversationLayout,
        diff: &DiffComponent,
        palette: Palette,
    ) {
        let exploration = self.exploration.as_ref().expect("question exploration");
        let view = EvidenceView {
            turn: index,
            reference: self.turns[index].reference,
        };
        let evidence = exploration.evidence(index);
        let Some(reference) = evidence.get(view.reference) else {
            return;
        };
        let Some(source) = exploration.comparison.source(&reference.location) else {
            return;
        };
        layout.gap();
        layout.text(
            format!("Establishes: {}", reference.relationship),
            palette.text,
            None,
        );
        if !reference.decision_relevance.is_empty() {
            layout.text(
                format!("For your answer: {}", reference.decision_relevance),
                palette.text,
                None,
            );
        }
        layout.gap();
        layout.text(
            format!(
                "{} · {}",
                source.display_path,
                match source.side {
                    SourceSide::Old => "Base",
                    SourceSide::New => "Working copy",
                }
            ),
            palette.dim,
            None,
        );
        if let Some(viewer) = diff.evidence_view(view) {
            if let Some(limitation) = viewer.evidence_limitation() {
                layout.text(limitation, palette.warning, None);
            } else {
                let maximum = layout.area.height.saturating_sub(1).max(3);
                let fit = viewer
                    .fitted_evidence_height(layout.area.width, (layout.area.height / 2).max(3));
                let height = self
                    .heights
                    .get(&view)
                    .copied()
                    .unwrap_or(fit)
                    .clamp(3, maximum);
                layout.push(Content::Window(view), height);
                layout.push(Content::Resize(view), 1);
            }
        } else {
            layout.text(
                "Open evidence",
                palette.focus,
                Some(Control::Evidence(view)),
            );
        }
        self.evidence_controls(view, &evidence, layout, palette);
    }

    fn evidence_controls(
        &self,
        view: EvidenceView,
        evidence: &[review_explore::EvidenceRef],
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let index = view.turn;
        let exploration = self.exploration.as_ref().expect("question exploration");
        let primary = exploration.questions[index].evidence.len();
        let supporting = evidence.len().saturating_sub(primary);
        let mut controls = vec![
            (
                if view.reference < primary {
                    format!("Evidence {}/{}", view.reference + 1, primary)
                } else {
                    format!("Evidence {primary}")
                },
                Control::References(index),
            ),
            (
                "Primary".into(),
                Control::Primary(EvidenceView {
                    turn: index,
                    reference: 0,
                }),
            ),
            ("Fit evidence".into(), Control::Fit(view)),
        ];
        if supporting > 0 {
            controls.push((
                format!("Supporting sources {supporting}"),
                Control::Supporting(index),
            ));
        }
        layout.controls(controls);
        if self.turns[index].references {
            self.evidence_links(
                index,
                evidence.iter().take(primary).enumerate(),
                layout,
                palette,
            );
        }
        if self.turns[index].supporting {
            self.evidence_links(
                index,
                evidence.iter().enumerate().skip(primary),
                layout,
                palette,
            );
        }
    }

    fn evidence_links<'a>(
        &self,
        index: usize,
        evidence: impl Iterator<Item = (usize, &'a review_explore::EvidenceRef)>,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let exploration = self.exploration.as_ref().expect("question exploration");
        for (reference, evidence) in evidence {
            let path = exploration
                .comparison
                .source(&evidence.location)
                .map_or_else(
                    || "Unavailable".into(),
                    |source| source.display_path.clone(),
                );
            layout.text(
                format!("{} · {path}: {}", reference + 1, evidence.relationship),
                palette.focus,
                Some(Control::Evidence(EvidenceView {
                    turn: index,
                    reference,
                })),
            );
        }
    }
}
