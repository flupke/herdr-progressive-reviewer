//! Application pane layout, shared by drawing and pointer hit-testing.

use std::ops::Range;

use ratatui::layout::Rect;
use ui_panes::SplitPane;

const NARROW_WIDTH: u16 = 72;
const MINIMUM_DIFF_WIDTH: u16 = 16;

use ui_events::{ReviewNavigation, ReviewPane};
use unicode_width::UnicodeWidthStr;

fn location_selector_panes(body: Rect, preferred_width: Option<u16>) -> SplitPane {
    SplitPane::new(
        body,
        preferred_width.unwrap_or(body.width * 30 / 100).max(18),
        24.min(body.width / 2),
    )
}

/// The Files, Threads and Explore tabs on the navigation pane's top
/// border, laid out once for both drawing and clicks.
pub(super) struct NavigationTabs {
    unread: bool,
}

/// One tab and the columns it covers, counted from the first tab.
pub(super) struct NavigationTab {
    pub(super) mode: ReviewNavigation,
    /// The name, whose first letter is the key that opens the tab.
    name: &'static str,
    /// Whether the unread marker follows the name.
    pub(super) unread: bool,
    columns: Range<usize>,
}

impl NavigationTab {
    /// The letter that opens the tab, and the rest of its name.
    pub(super) fn key_and_rest(&self) -> (&'static str, &'static str) {
        self.name.split_at(1)
    }
}

impl NavigationTabs {
    const NAMES: [(ReviewNavigation, &'static str); 3] = [
        (ReviewNavigation::Files, "Files"),
        (ReviewNavigation::Threads, "Threads"),
        (ReviewNavigation::Explore, "Explore"),
    ];
    /// On each side of a name, inside the tab.
    pub(super) const PADDING: &'static str = " ";
    pub(super) const GAP: &'static str = " ";
    pub(super) const UNREAD: &'static str = "● ";

    /// The tabs, with the unread marker on Threads when `unread` is set.
    pub(super) fn new(unread: bool) -> Self {
        Self { unread }
    }

    pub(super) fn tabs(&self) -> impl Iterator<Item = NavigationTab> + '_ {
        let mut start = 0;
        Self::NAMES.into_iter().map(move |(mode, name)| {
            let unread = self.unread && mode == ReviewNavigation::Threads;
            let width = name.len()
                + 2 * Self::PADDING.len()
                + if unread { Self::UNREAD.width() } else { 0 };
            let tab = NavigationTab {
                mode,
                name,
                unread,
                columns: start..start + width,
            };
            start += width + Self::GAP.len();
            tab
        })
    }

    fn width(&self) -> usize {
        self.tabs().last().map_or(0, |tab| tab.columns.end)
    }

    fn minimum_pane_width() -> u16 {
        u16::try_from(Self::new(true).width() + 2).expect("navigation tabs fit the terminal width")
    }

    pub(super) fn mode_at(&self, column: u16) -> Option<ReviewNavigation> {
        self.tabs()
            .find(|tab| tab.columns.contains(&usize::from(column)))
            .map(|tab| tab.mode)
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

#[cfg(test)]
#[path = "layout.tests.rs"]
mod tests;
