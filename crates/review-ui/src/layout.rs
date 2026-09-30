//! Application pane layout, shared by drawing and pointer hit-testing.

use ratatui::layout::Rect;
use ui_panes::SplitPane;

const NARROW_WIDTH: u16 = 72;
const MINIMUM_DIFF_WIDTH: u16 = 16;

use ui_events::{ReviewNavigation, ReviewPane};

pub(super) struct NavigationTabs;

fn location_selector_panes(body: Rect, preferred_width: Option<u16>) -> SplitPane {
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
    width: u16,
    height: u16,
    footer_height: u16,
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

    fn for_navigation(
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

    fn is_wide(self) -> bool {
        self.wide
    }

    fn body_height(self) -> u16 {
        self.height.saturating_sub(1 + self.footer_height)
    }

    fn contains_body(self, column: u16, row: u16) -> bool {
        column < self.width && row > 0 && row < self.height.saturating_sub(self.footer_height)
    }

    fn is_separator(self, column: u16, row: u16) -> bool {
        self.is_wide() && self.contains_body(column, row) && column.abs_diff(self.file_width) <= 1
    }

    fn page_rows(self) -> usize {
        usize::from(self.body_height().saturating_sub(2).max(1))
    }
}

/// The terminal size and the reviewer's preferred navigation pane width.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Viewport {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) file_width: Option<u16>,
}

/// Where each part of the screen goes for one navigation state.
///
/// The application computes it once per state change; drawing and pointer
/// hit-testing both read this value, so they cannot disagree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScreenLayout {
    area: Rect,
    file_width: Option<u16>,
    navigation: ReviewNavigation,
    focus: ReviewPane,
    locations_active: bool,
    panes: PaneLayout,
    body: Body,
}

/// How the area between the header and the footer is divided.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Body {
    /// Explore fills the body, beside the location selector while it is open.
    Explore {
        locations: Option<SplitPane>,
        source: Rect,
    },
    /// The navigation pane beside the diff on a wide terminal.
    Split(SplitPane),
    /// One pane at a time on a narrow terminal.
    Single { pane: ReviewPane, area: Rect },
}

impl ScreenLayout {
    pub(crate) fn new(
        viewport: Viewport,
        navigation: ReviewNavigation,
        focus: ReviewPane,
        locations_active: bool,
    ) -> Self {
        Self::within(
            Rect::new(0, 0, viewport.width, viewport.height),
            viewport.file_width,
            navigation,
            focus,
            locations_active,
        )
    }

    fn within(
        area: Rect,
        file_width: Option<u16>,
        navigation: ReviewNavigation,
        focus: ReviewPane,
        locations_active: bool,
    ) -> Self {
        let panes = PaneLayout::for_navigation(area.width, area.height, file_width, navigation);
        let body = Rect::new(area.x, area.y + 1, panes.width, panes.body_height());
        let body = if navigation == ReviewNavigation::Explore {
            let locations = locations_active.then(|| location_selector_panes(body, file_width));
            Body::Explore {
                source: locations.map_or(body, |split| split.right),
                locations,
            }
        } else if panes.is_wide() {
            Body::Split(SplitPane::new(body, panes.file_width, 0))
        } else {
            Body::Single {
                pane: focus,
                area: body,
            }
        };
        Self {
            area,
            file_width,
            navigation,
            focus,
            locations_active,
            panes,
            body,
        }
    }

    /// This layout for another render area.
    ///
    /// The application renders into the viewport it laid out, so this is
    /// the same value there; only a caller drawing into another area gets
    /// the layout recomputed for that area.
    pub(crate) fn fitted_to(self, area: Rect) -> Self {
        if area == self.area {
            return self;
        }
        Self::within(
            area,
            self.file_width,
            self.navigation,
            self.focus,
            self.locations_active,
        )
    }

    pub(crate) fn body(self) -> Body {
        self.body
    }

    pub(crate) fn header(self) -> Rect {
        Rect::new(self.area.x, self.area.y, self.area.width, 1)
    }

    pub(crate) fn footer(self) -> Rect {
        Rect::new(
            self.area.x,
            self.area.bottom().saturating_sub(self.panes.footer_height),
            self.area.width,
            self.panes.footer_height,
        )
    }

    pub(crate) fn body_area(self) -> Rect {
        Rect::new(
            self.area.x,
            self.area.y + 1,
            self.area.width,
            self.panes.body_height(),
        )
    }

    /// The pane that shows Files, Threads or Explore, with its tabs on top.
    pub(crate) fn navigation_pane(self) -> Option<Rect> {
        match self.body {
            Body::Explore { source, .. } => Some(source),
            Body::Split(split) => Some(split.left),
            Body::Single {
                pane: ReviewPane::Navigation,
                area,
            } => Some(area),
            Body::Single { .. } => None,
        }
    }

    /// The area that receives pointer input for the navigation component.
    ///
    /// Explore handles its whole pane; Files and Threads handle the inside
    /// of their border.
    pub(crate) fn navigation_input_area(self) -> Option<Rect> {
        let pane = self.navigation_pane()?;
        Some(match self.body {
            Body::Explore { .. } => pane,
            Body::Split(_) | Body::Single { .. } => Rect::new(
                pane.x + 1,
                pane.y + 1,
                pane.width.saturating_sub(2),
                pane.height.saturating_sub(2),
            ),
        })
    }

    /// The diff pane, whether or not a narrow terminal currently shows it.
    ///
    /// Explore shows the diff as evidence inside its own pane instead.
    pub(crate) fn diff_pane(self) -> Option<Rect> {
        match self.body {
            Body::Explore { .. } => None,
            Body::Split(split) => Some(split.right),
            Body::Single { area, .. } => Some(area),
        }
    }

    /// The diff pane when it is on screen.
    pub(crate) fn visible_diff_pane(self) -> Option<Rect> {
        match self.body {
            Body::Single {
                pane: ReviewPane::Navigation,
                ..
            } => None,
            _ => self.diff_pane(),
        }
    }

    /// Where the location selector goes while it is open.
    pub(crate) fn locations_pane(self) -> Rect {
        match self.body {
            Body::Explore {
                locations: Some(split),
                ..
            }
            | Body::Split(split) => split.left,
            Body::Explore { source, .. } => source,
            Body::Single { area, .. } => area,
        }
    }

    /// The row of navigation tabs, inside the navigation pane's top border.
    pub(crate) fn tabs(self) -> Option<Rect> {
        self.navigation_pane()
            .map(|pane| Rect::new(pane.x + 1, pane.y, pane.width.saturating_sub(2), 1))
    }

    pub(crate) fn page_rows(self) -> usize {
        self.panes.page_rows()
    }

    pub(crate) fn is_separator(self, column: u16, row: u16) -> bool {
        self.panes.is_separator(column, row)
    }
}
