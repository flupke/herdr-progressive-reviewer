use review_test_support::HerdrTestServer;

use super::*;

fn panes(server: &HerdrTestServer) -> Vec<String> {
    server.run_cli_json(&["pane", "list"])["result"]["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pane| pane["pane_id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_viewer_pane_opens_beside_its_target_and_closes_with_the_session() {
    let repository = tempfile::tempdir().unwrap();
    let server = HerdrTestServer::start(repository.path());
    let workspace = server.run_cli_json(&[
        "workspace",
        "create",
        "--cwd",
        repository.path().to_str().unwrap(),
        "--no-focus",
    ]);
    let target = workspace["result"]["root_pane"]["pane_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let herdr = Herdr::with_environment(server.binary().to_owned(), server.environment().clone());
    let area =
        &server.run_cli_json(&["pane", "layout", "--pane", &target])["result"]["layout"]["area"];
    assert_eq!(
        herdr.pane_size(&target).unwrap(),
        pane(
            u16::try_from(area["width"].as_u64().unwrap()).unwrap(),
            u16::try_from(area["height"].as_u64().unwrap()).unwrap()
        ),
        "a lone pane fills its tab"
    );

    let placement = Placement {
        split: Some(SplitDirection::Right),
        ratio: Some(0.3),
    };
    let viewer = ViewerPane::open(herdr, &target, placement, size(100, 30), |_| {
        vec!["true".into()]
    })
    .unwrap();

    let pane = viewer.pane().to_owned();
    assert!(panes(&server).contains(&pane));
    let layout = server.run_cli_json(&["pane", "layout", "--pane", &target]);
    let width = |id: &str| {
        layout["result"]["layout"]["panes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|pane| pane["pane_id"] == id)
            .unwrap()["rect"]["width"]
            .as_f64()
            .unwrap()
    };
    let share = width(&pane) / (width(&pane) + width(&target));
    assert!(
        (share - 0.3).abs() < 0.05,
        "the viewer takes {share} of the split"
    );
    drop(viewer);
    assert!(!panes(&server).contains(&pane));
    assert!(panes(&server).contains(&target));
}

fn size(cols: u16, rows: u16) -> Size {
    Size { cols, rows }
}

fn pane(width: u16, height: u16) -> PaneSize {
    PaneSize { width, height }
}

#[test]
fn a_wide_pane_fits_the_viewer_beside_the_driver() {
    let (split, ratio) = Placement::default().fit(pane(250, 60), size(100, 30));

    assert_eq!(split, SplitDirection::Right);
    assert!((ratio - 102.5 / 250.0).abs() < 1e-9);
}

#[test]
fn a_tall_pane_fits_the_viewer_below_the_driver() {
    let (split, ratio) = Placement::default().fit(pane(120, 80), size(100, 30));

    assert_eq!(split, SplitDirection::Down);
    assert!((ratio - 32.5 / 80.0).abs() < 1e-9);
}

#[test]
fn a_pane_too_small_for_the_session_leaves_the_driver_a_quarter() {
    let (split, ratio) = Placement::default().fit(pane(110, 25), size(100, 30));

    assert_eq!(split, SplitDirection::Right);
    assert!((ratio - MAX_SHARE).abs() < 1e-9);
}

#[test]
fn a_chosen_split_and_share_are_kept() {
    let placement = Placement {
        split: Some(SplitDirection::Down),
        ratio: Some(0.3),
    };

    assert_eq!(placement.chosen(), Some((SplitDirection::Down, 0.3)));
    assert_eq!(
        placement.fit(pane(250, 60), size(100, 30)),
        (SplitDirection::Down, 0.3)
    );
}

#[test]
fn a_chosen_split_is_fitted_with_its_own_share() {
    let placement = Placement {
        split: Some(SplitDirection::Down),
        ratio: None,
    };

    let (split, ratio) = placement.fit(pane(250, 60), size(100, 30));

    assert_eq!(split, SplitDirection::Down);
    assert!((ratio - 32.5 / 60.0).abs() < 1e-9);
}

#[test]
fn the_viewer_stops_when_the_session_stream_ends() {
    let directory = tempfile::tempdir().unwrap();
    let stream =
        super::super::stream::FrameStream::start(&directory.path().join("stream.sock")).unwrap();
    let mut connection = UnixStream::connect(stream.path()).unwrap();
    let viewer = std::thread::spawn(move || {
        let mut terminal = Vec::new();
        show(&mut connection, &mut terminal).unwrap();
        terminal
    });
    // Let the stream accept the viewer before the session ends.
    std::thread::sleep(std::time::Duration::from_millis(100));

    drop(stream);

    let terminal = viewer.join().unwrap();
    assert!(terminal.starts_with(ENTER));
    assert!(terminal.ends_with(LEAVE));
}
