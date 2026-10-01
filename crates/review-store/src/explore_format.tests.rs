//! Explore records in their saved format. Loading them and writing them back
//! must reproduce the exact bytes; records of the first format load as absent.

use super::*;

const INSTANCE: &str = "eb9afce9-0943-4b1d-993b-722ec98ca4c5";
const HISTORY: &[u8] = include_bytes!("../testdata/explore/index.json");
const PASS: &[u8] = include_bytes!("../testdata/explore/pass.json");
const VIEW: &[u8] = include_bytes!("../testdata/explore/view.json");

struct SavedFixture {
    _directory: tempfile::TempDir,
    store: ReviewStore,
    unit: ReviewUnit,
    review: PathBuf,
}

impl SavedFixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
        let unit: ReviewUnit = "aabb".into();
        let review = store.explore_review(&unit).unwrap();
        std::fs::create_dir_all(&review).unwrap();
        std::fs::write(review.join("index.json"), HISTORY).unwrap();
        std::fs::write(review.join(format!("{INSTANCE}.json")), PASS).unwrap();
        std::fs::write(review.join(format!("{INSTANCE}.view.json")), VIEW).unwrap();
        Self {
            _directory: directory,
            store,
            unit,
            review,
        }
    }

    fn remove_all(&self) {
        for name in [
            "index.json".to_owned(),
            format!("{INSTANCE}.json"),
            format!("{INSTANCE}.view.json"),
        ] {
            std::fs::remove_file(self.review.join(name)).unwrap();
        }
    }

    fn assert_rewritten(&self) {
        assert_eq!(
            std::fs::read(self.review.join("index.json")).unwrap(),
            HISTORY
        );
        assert_eq!(
            std::fs::read(self.review.join(format!("{INSTANCE}.json"))).unwrap(),
            PASS
        );
        assert_eq!(
            std::fs::read(self.review.join(format!("{INSTANCE}.view.json"))).unwrap(),
            VIEW
        );
    }
}

#[test]
fn saved_history_pass_and_view_load_in_their_current_format() {
    let fixture = SavedFixture::new();

    let history = fixture.store.load_explore_history(&fixture.unit).unwrap();
    let pass = fixture
        .store
        .load_explore(&fixture.unit, INSTANCE)
        .unwrap()
        .unwrap();
    let view = fixture
        .store
        .load_explore_view(&fixture.unit, INSTANCE)
        .unwrap()
        .unwrap();

    assert_eq!(history.passes, vec![INSTANCE.to_owned()]);
    assert!(history.latest_editable);
    assert_eq!(pass.revision, 7);
    assert_eq!(pass.exploration.questions.len(), 1);
    assert_eq!(pass.exploration.answers[0].text, "Keep \"it\"\nwith a test");
    assert!(pass.completion.is_some());
    assert!(pass.last_agent_session.is_some());
    let delivery = pass.implementations.values().next().unwrap();
    assert_eq!(delivery.request.text, "Only the edited task");
    assert_eq!(delivery.state, review_explore::DispatchState::Attempting);
    assert!(
        pass.turns
            .values()
            .any(|turn| turn.state == review_explore::DispatchState::Delivered)
    );
    assert_eq!(view.sequence, 3);
    assert_eq!(
        view.state.tasks.values().next().unwrap().text,
        "Edited \"task\"\n"
    );
}

#[test]
fn saved_history_pass_and_view_are_written_back_unchanged() {
    let fixture = SavedFixture::new();
    let history = fixture.store.load_explore_history(&fixture.unit).unwrap();
    let pass = fixture
        .store
        .load_explore(&fixture.unit, INSTANCE)
        .unwrap()
        .unwrap();
    let view = fixture
        .store
        .load_explore_view(&fixture.unit, INSTANCE)
        .unwrap()
        .unwrap();
    fixture.remove_all();

    let records = fixture.store.lock_explore(&fixture.unit).unwrap();
    records.save_history(&history).unwrap();
    records.save_pass(&pass).unwrap();
    records.save_view(&view).unwrap();
    drop(records);

    fixture.assert_rewritten();
}

#[test]
fn records_of_the_first_format_load_as_absent() {
    let fixture = SavedFixture::new();
    for (name, bytes) in [
        (
            "index.json".to_owned(),
            &include_bytes!("../testdata/explore/v1/index.json")[..],
        ),
        (
            format!("{INSTANCE}.json"),
            &include_bytes!("../testdata/explore/v1/pass.json")[..],
        ),
        (
            format!("{INSTANCE}.view.json"),
            &include_bytes!("../testdata/explore/v1/view.json")[..],
        ),
    ] {
        std::fs::write(fixture.review.join(name), bytes).unwrap();
    }

    let history = fixture.store.load_explore_history(&fixture.unit).unwrap();

    assert!(history.passes.is_empty());
    assert_eq!(
        fixture.store.load_explore(&fixture.unit, INSTANCE).unwrap(),
        None
    );
    assert_eq!(
        fixture
            .store
            .load_explore_view(&fixture.unit, INSTANCE)
            .unwrap(),
        None
    );
}
