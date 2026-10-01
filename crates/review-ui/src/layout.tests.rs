use ui_events::ReviewNavigation;

use super::NavigationTabs;

#[test]
fn tabs_are_padded_labels_one_space_apart() {
    let columns = |unread| {
        NavigationTabs::new(unread)
            .tabs()
            .map(|tab| (tab.mode, tab.columns))
            .collect::<Vec<_>>()
    };

    assert_eq!(
        columns(false),
        [
            (ReviewNavigation::Files, 0..7),
            (ReviewNavigation::Threads, 8..17),
            (ReviewNavigation::Explore, 18..27),
        ]
    );
    // The unread marker belongs to Threads and pushes Explore along.
    assert_eq!(
        columns(true),
        [
            (ReviewNavigation::Files, 0..7),
            (ReviewNavigation::Threads, 8..19),
            (ReviewNavigation::Explore, 20..29),
        ]
    );
}

#[test]
fn a_click_selects_the_tab_under_it_and_gaps_select_none() {
    let tabs = NavigationTabs::new(true);

    assert_eq!(tabs.mode_at(0), Some(ReviewNavigation::Files));
    assert_eq!(tabs.mode_at(7), None);
    assert_eq!(tabs.mode_at(18), Some(ReviewNavigation::Threads));
    assert_eq!(tabs.mode_at(20), Some(ReviewNavigation::Explore));
    assert_eq!(tabs.mode_at(29), None);
}
