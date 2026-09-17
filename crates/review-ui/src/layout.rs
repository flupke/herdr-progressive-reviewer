//! Application pane layout and focus state.

use ratatui::layout::Rect;
use ui_panes::SplitPane;

const NARROW_WIDTH: u16 = 72;
const MINIMUM_DIFF_WIDTH: u16 = 16;

use ui_events::{ReviewNavigation, ReviewPane};

pub(super) struct NavigationTabs;

pub(super) fn location_selector_panes(body: Rect, preferred_width: Option<u16>) -> SplitPane {
    SplitPane::new(
        body,
        preferred_width.unwrap_or(body.width * 30 / 100).max(18),
        24.min(body.width / 2),
    )
}

impl NavigationTabs {
    pub(super) const FILES: &str = " [F]iles ";
    pub(super) const EXPLORE: &str = " [E]xplore ";
    pub(super) const THREADS: &str = " [T]hreads ";
    pub(super) const SEPARATOR: &str = "|";
    pub(super) const UNREAD: &str = "● ";

    fn width(unread: bool) -> usize {
        Self::FILES.len()
            + Self::SEPARATOR.len()
            + Self::THREADS.len()
            + Self::SEPARATOR.len()
            + Self::EXPLORE.len()
            + usize::from(unread) * Self::UNREAD.chars().count()
    }

    fn minimum_pane_width() -> u16 {
        u16::try_from(Self::width(true) + 2).expect("navigation tabs fit the terminal width")
    }

    pub(super) fn mode_at(column: u16, unread: bool) -> Option<ReviewNavigation> {
        let column = usize::from(column);
        let threads_start = Self::FILES.len() + Self::SEPARATOR.len();
        let threads_end = threads_start
            + Self::THREADS.len()
            + usize::from(unread) * Self::UNREAD.chars().count();
        if column < Self::FILES.len() {
            Some(ReviewNavigation::Files)
        } else if (threads_start..threads_end).contains(&column) {
            Some(ReviewNavigation::Threads)
        } else if (threads_end + Self::SEPARATOR.len()..Self::width(unread)).contains(&column) {
            Some(ReviewNavigation::Explore)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PaneLayout {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) footer_height: u16,
    pub(crate) file_width: u16,
    wide: bool,
}

impl PaneLayout {
    pub(crate) fn new(width: u16, height: u16, file_width: Option<u16>) -> Self {
        let file_width = if width >= NARROW_WIDTH {
            file_width.unwrap_or(width * 30 / 100).clamp(
                NavigationTabs::minimum_pane_width(),
                width - MINIMUM_DIFF_WIDTH,
            )
        } else {
            width
        };
        Self {
            width,
            height,
            wide: width >= NARROW_WIDTH,
            footer_height: 1,
            file_width,
        }
    }

    pub(crate) fn for_navigation(
        width: u16,
        height: u16,
        file_width: Option<u16>,
        navigation: ReviewNavigation,
    ) -> Self {
        let mut layout = Self::new(width, height, file_width);
        if navigation == ReviewNavigation::Explore {
            layout.wide = false;
            layout.file_width = width;
        }
        layout
    }

    pub(crate) fn is_wide(self) -> bool {
        self.wide
    }

    pub(crate) fn body_height(self) -> u16 {
        self.height.saturating_sub(1 + self.footer_height)
    }

    pub(crate) fn contains_body(self, column: u16, row: u16) -> bool {
        column < self.width && row > 0 && row < self.height.saturating_sub(self.footer_height)
    }

    pub(crate) fn is_separator(self, column: u16, row: u16) -> bool {
        self.is_wide() && self.contains_body(column, row) && column.abs_diff(self.file_width) <= 1
    }

    pub(crate) fn page_rows(self) -> usize {
        usize::from(self.body_height().saturating_sub(2).max(1))
    }

    pub(crate) fn files_content_area(self, focus: ReviewPane) -> Option<Rect> {
        if !self.is_wide() && focus != ReviewPane::Navigation {
            return None;
        }
        let pane_width = if self.is_wide() {
            self.file_width
        } else {
            self.width
        };
        Some(Rect::new(
            1,
            2,
            pane_width.saturating_sub(2),
            self.body_height().saturating_sub(2),
        ))
    }
}
