//! Document coordinates and clipping shared by drawing and pointer routing.
use super::{Control, Reveal, controls::Button};
use diff_component::ClippedViewport;
use markdown_rendering::MarkdownRenderer;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Text,
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};
use std::ops::Range;
use ui_events::{DiffViewportChanged, EvidenceView, ExploreViewports};
use ui_theme::Palette;

#[derive(Clone)]
pub(super) enum Content {
    Text(Text<'static>, Option<Control>),
    Window(EvidenceView),
    Editor(super::EditorTarget),
    Resize(EvidenceView),
    Controls(Vec<Button>),
}

#[derive(Clone)]
pub(super) struct Item {
    pub(super) top: usize,
    pub(super) height: u16,
    pub(super) content: Content,
}

#[derive(Clone, Copy)]
pub(super) struct Window {
    pub(super) view: EvidenceView,
    pub(super) viewport: ClippedViewport,
}

impl Window {
    pub(super) fn new(view: EvidenceView, visible: Rect, skipped: u16, height: u16) -> Self {
        Self {
            view,
            viewport: ClippedViewport::new(visible, skipped, height).with_pinned_header(),
        }
    }
}

#[derive(Clone, Default)]
pub struct ConversationLayout {
    pub(super) area: Rect,
    pub(super) scroll: usize,
    pub(super) height: usize,
    pub(super) items: Vec<Item>,
    pub(super) answer: Option<usize>,
    pub(super) choice: Option<Range<usize>>,
    pub(super) evidence: Option<Range<usize>>,
    pub(super) jev: Option<Range<usize>>,
    pub(super) coverage_diff: Option<Range<usize>>,
    pub(super) frame: Option<Rect>,
    pub(super) navigation: super::navigation::Navigation,
}

impl ConversationLayout {
    pub(super) fn new(area: Rect) -> Self {
        Self {
            area,
            ..Self::default()
        }
    }

    pub(super) fn render_frame(&self, buffer: &mut Buffer, palette: Palette) {
        if let Some(frame) = self.frame {
            Block::default()
                .borders(Borders::ALL)
                .title(" Coverage ")
                .border_style(Style::default().fg(palette.dim))
                .render(frame, buffer);
        }
    }

    pub(super) fn text(&mut self, text: impl Into<String>, color: Color, control: Option<Control>) {
        self.paragraph(Text::styled(text.into(), color), control);
    }

    fn paragraph(&mut self, text: Text<'static>, control: Option<Control>) {
        let height = Paragraph::new(text.clone())
            .wrap(Wrap { trim: false })
            .line_count(self.area.width.max(1));
        self.push(
            Content::Text(text, control),
            u16::try_from(height).unwrap_or(u16::MAX).max(1),
        );
    }

    pub(super) fn push(&mut self, content: Content, height: u16) {
        self.items.push(Item {
            top: self.height,
            height,
            content,
        });
        self.height = self.height.saturating_add(usize::from(height));
    }

    pub(super) fn section(&mut self, title: &str, body: &str, palette: Palette) {
        if body.trim().is_empty() {
            return;
        }
        let lines = MarkdownRenderer::default().render(
            &format!("# {title}\n\n{body}"),
            self.area.width,
            palette,
        );
        self.gap();
        self.paragraph(Text::from(lines), None);
    }

    pub(super) fn gap(&mut self) {
        if self
            .items
            .last()
            .is_some_and(|item| item.top + usize::from(item.height) == self.height)
        {
            self.height = self.height.saturating_add(1);
        }
    }

    pub(super) fn controls(&mut self, controls: impl IntoIterator<Item = (String, Control)>) {
        let labels = controls
            .into_iter()
            .map(|(label, control)| (format!("[{label}]"), Some(control)));
        for row in Button::wrap(self.area.width, labels) {
            self.push(Content::Controls(row), 1);
        }
    }

    pub(super) fn position(&mut self, scroll: usize, reveal: Option<Reveal>) {
        let ceiling = match reveal {
            Some(Reveal::KeepAnswer { .. }) => usize::MAX,
            None => self.maximum_scroll().max(scroll),
            _ => self.maximum_scroll(),
        };
        self.scroll = self.revealed_scroll(scroll, reveal).min(ceiling);
        if self.scroll >= self.height {
            self.scroll = self.maximum_scroll();
        }
        // Retain enough trailing space to keep the visible anchor even when the document
        // previously fit on screen. Idle frames must not clamp it back and shift the text.
        self.height = self
            .height
            .max(self.scroll.saturating_add(usize::from(self.area.height)));
    }

    fn revealed_scroll(&self, scroll: usize, reveal: Option<Reveal>) -> usize {
        match reveal {
            Some(Reveal::Start) => 0,
            Some(Reveal::RestoreScroll) => scroll.min(self.maximum_scroll()),
            Some(Reveal::Editor(target)) => self
                .items
                .iter()
                .find(|item| matches!(item.content, Content::Editor(editor) if editor == target))
                .map_or(scroll, |item| {
                    self.reveal_rows(item.top..item.top.saturating_add(8), scroll)
                }),
            Some(Reveal::Choice) => self
                .choice
                .as_ref()
                .map_or(scroll, |rows| self.reveal_rows(rows.clone(), scroll)),
            Some(Reveal::Evidence) => self
                .evidence
                .as_ref()
                .map_or(scroll, |rows| self.reveal_rows(rows.clone(), scroll)),
            Some(Reveal::Jev) => self
                .jev
                .as_ref()
                .map_or(scroll, |rows| self.reveal_rows(rows.clone(), scroll)),
            Some(Reveal::CoverageDiff) => self
                .coverage_diff
                .as_ref()
                .map_or(scroll, |rows| self.reveal_rows(rows.clone(), scroll)),
            Some(Reveal::KeepAnswer { offset }) => self
                .answer
                .map_or(scroll, |row| row.saturating_add_signed(offset)),
            None => scroll,
        }
    }

    pub(super) fn answer_anchor(&self) -> Option<Reveal> {
        Some(Reveal::KeepAnswer {
            offset: isize::try_from(self.scroll).ok()? - isize::try_from(self.answer?).ok()?,
        })
    }

    fn reveal_rows(&self, rows: Range<usize>, scroll: usize) -> usize {
        if rows.start < scroll {
            rows.start
        } else {
            scroll.max(
                rows.end
                    .saturating_sub(usize::from(self.area.height))
                    .min(rows.start),
            )
        }
    }

    pub(super) fn maximum_scroll(&self) -> usize {
        self.height.saturating_sub(usize::from(self.area.height))
    }

    pub(super) fn visible(&self, item: &Item) -> Option<(Rect, u16)> {
        let start = item.top.max(self.scroll);
        let end =
            (item.top + usize::from(item.height)).min(self.scroll + usize::from(self.area.height));
        if start >= end {
            return None;
        }
        Some((
            Rect::new(
                self.area.x,
                self.area.y + u16::try_from(start - self.scroll).ok()?,
                self.area.width,
                u16::try_from(end - start).ok()?,
            ),
            u16::try_from(start - item.top).ok()?,
        ))
    }

    pub(super) fn window_at(&self, column: u16, row: u16) -> Option<Window> {
        self.items.iter().find_map(|item| {
            let Content::Window(view) = item.content else {
                return None;
            };
            let (visible, skipped) = self.visible(item)?;
            visible
                .contains((column, row).into())
                .then_some(Window::new(view, visible, skipped, item.height))
        })
    }

    pub(super) fn control_at(&self, column: u16, row: u16) -> Option<Control> {
        if let Some(control) = self.navigation.control_at(column, row) {
            return Some(control);
        }
        self.items.iter().find_map(|item| {
            let (area, _) = self.visible(item)?;
            if !area.contains((column, row).into()) {
                return None;
            }
            match &item.content {
                Content::Text(_, action) => *action,
                Content::Editor(super::EditorTarget::Answer) => Some(Control::Edit),
                Content::Editor(super::EditorTarget::Implementation) => {
                    Some(Control::EditImplementation)
                }
                Content::Resize(_) | Content::Window(_) => None,
                Content::Controls(buttons) => buttons
                    .iter()
                    .find(|button| {
                        let offset = usize::from(column.saturating_sub(area.x));
                        (usize::from(button.column)..usize::from(button.column) + button.text.len())
                            .contains(&offset)
                    })
                    .and_then(|button| button.control),
            }
        })
    }

    pub(super) fn resize_at(&self, column: u16, row: u16) -> Option<(EvidenceView, u16)> {
        self.items.iter().enumerate().find_map(|(index, item)| {
            let (area, _) = self.visible(item)?;
            if !area.contains((column, row).into()) {
                return None;
            }
            match item.content {
                Content::Resize(view) => Some((view, self.items[index - 1].height)),
                Content::Window(view)
                    if item.top + usize::from(item.height) - 1
                        == self.scroll + usize::from(row.saturating_sub(self.area.y)) =>
                {
                    Some((view, item.height))
                }
                _ => None,
            }
        })
    }

    pub(super) fn window_height(&self, view: EvidenceView) -> Option<u16> {
        self.items.iter().find_map(|item| {
            matches!(item.content, Content::Window(id) if id == view).then_some(item.height)
        })
    }

    /// Full window sizes, independent of partial clipping by conversation scrolling.
    pub fn viewports(&self) -> ExploreViewports {
        ExploreViewports(
            self.items
                .iter()
                .filter_map(|item| {
                    let Content::Window(view) = item.content else {
                        return None;
                    };
                    Some((
                        view,
                        DiffViewportChanged {
                            width: self.area.width.saturating_sub(2),
                            height: item.height.saturating_sub(2),
                        },
                    ))
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_scroll_fills_the_viewport_without_shifting_live_anchors() {
        let mut restored = ConversationLayout::new(Rect::new(0, 0, 80, 40));
        restored.push(Content::Text(Text::raw("row\n".repeat(100)), None), 100);
        restored.position(90, Some(Reveal::RestoreScroll));
        assert_eq!(restored.scroll, 60);

        let mut live = ConversationLayout::new(Rect::new(0, 0, 80, 40));
        live.push(Content::Text(Text::raw("row\n".repeat(100)), None), 100);
        live.position(90, None);
        assert_eq!(live.scroll, 90);
    }
}
