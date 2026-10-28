//! The repository's saved rounds, which the Explore page on this machine lists and opens read
//! only as past rounds. A past round never goes through the session's round: this reads it from
//! the store, and derives what the page shows of it with the code that derives the session's own
//! round, as a round that offers no action. Its cited code is read at the round's own
//! checkpoint, else from its saved diff, never from today's working copy.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{SystemTime, UNIX_EPOCH};

use review_explore::{EvidenceRef, ExploreRound, RoundHeading, RoundOverview};
use review_explore_page::{
    PastChange, PastRound, PastRoundEntry, PastRounds, PastRoundsPublisher, PublishedRound,
    ReviewName, RoundStage,
};
use review_repository::repository::{Repository, SnapshotIdentity};
use review_store::{ReviewStore, SavedRoundFile};

use crate::page::{CitedCode, LatestTurn, PageCitations, interrupted, latest_turn, sent_answer};

/// What a change with no description is called in the list.
const NO_DESCRIPTION: &str = "No description";

/// The saved rounds of one repository, read from its review store.
pub struct SavedRoundsReader {
    store: ReviewStore,
    repository: Repository,
    /// The heading of each round file read so far, by its path, with the time it was written
    /// when it was read: a file written since is read again. `None` for a file that cannot be
    /// read, such as a round saved in an earlier format.
    headings: Mutex<HashMap<PathBuf, (SystemTime, Option<RoundHeading>)>>,
}

impl SavedRoundsReader {
    /// The saved rounds of `store`, whose cited code `repository` holds.
    pub fn new(store: ReviewStore, repository: Repository) -> Self {
        Self {
            store,
            repository,
            headings: Mutex::default(),
        }
    }

    /// The saved rounds, as the page reaches them: their list, which `refresh` keeps current,
    /// and the round the page opens.
    pub fn serve(self) -> (PastRounds, PastRoundsRefresh) {
        let reader = Arc::new(self);
        let publisher = PastRoundsPublisher::default();
        let opener = reader.clone();
        let rounds = PastRounds::new(&publisher, move |id| opener.open(id));
        (rounds, PastRoundsRefresh::start(reader, publisher))
    }

    /// Every saved round of the repository, grouped by change, the change with the newest round
    /// first, and each change's rounds newest first. A round that cannot be read is left out.
    pub(crate) fn list(&self) -> Vec<PastChange> {
        let mut by_review: BTreeMap<String, Vec<(SystemTime, RoundHeading)>> = BTreeMap::new();
        for (saved_at, heading) in self.headings() {
            by_review
                .entry(heading.review_unit.as_str().to_owned())
                .or_default()
                .push((saved_at, heading));
        }
        let mut changes: Vec<(SystemTime, PastChange)> = by_review
            .into_values()
            .filter_map(|mut rounds| {
                rounds.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.instance.cmp(&b.1.instance)));
                let (newest, heading) = rounds.first()?;
                let change = PastChange {
                    title: title(heading),
                    revision: heading
                        .base
                        .as_ref()
                        .map(SnapshotIdentity::short_revision)
                        .unwrap_or_default(),
                    rounds: rounds
                        .iter()
                        .map(|(saved_at, heading)| PastRoundEntry {
                            id: heading.instance.clone(),
                            saved_at_ms: millis(*saved_at),
                        })
                        .collect(),
                };
                Some((*newest, change))
            })
            .collect();
        changes.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title.cmp(&b.1.title)));
        changes.into_iter().map(|(_, change)| change).collect()
    }

    /// The saved round `id`, as the page shows a past round; `None` when no saved round has
    /// that identity, or when it cannot be read.
    pub(crate) fn open(&self, id: &str) -> Option<PastRound> {
        let (saved_at, heading) = self.find(id).or_else(|| {
            // A round saved since the list was last read.
            self.headings();
            self.find(id)
        })?;
        let round = self
            .store
            .load_explore(&heading.review_unit, &heading.instance)
            .ok()
            .flatten()?;
        let threads = self.store.load_threads(&heading.review_unit).ok();
        let review = ReviewName {
            title: title(&heading),
            ..heading.base.as_ref().map_or_else(
                || ReviewName::unnamed(self.repository.root()),
                |base| ReviewName::of(self.repository.root(), base),
            )
        };
        Some(past_round(
            &round,
            &self.repository,
            review,
            threads,
            millis(saved_at),
        ))
    }

    /// The heading of the round `id`, as the files were last read, with when it was saved.
    fn find(&self, id: &str) -> Option<(SystemTime, RoundHeading)> {
        self.headings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .find_map(|(saved_at, heading)| {
                heading
                    .as_ref()
                    .filter(|heading| heading.instance == id)
                    .map(|heading| (*saved_at, heading.clone()))
            })
    }

    /// The heading of every saved round that can be read, with when it was saved, reading only
    /// the files written since they were last read.
    fn headings(&self) -> Vec<(SystemTime, RoundHeading)> {
        let files = self.store.saved_explore_round_files().unwrap_or_default();
        let mut headings = self.headings.lock().unwrap_or_else(PoisonError::into_inner);
        headings.retain(|path, _| files.iter().any(|file| file.path() == path));
        files
            .iter()
            .filter_map(|file: &SavedRoundFile| {
                let read = headings
                    .get(file.path())
                    .filter(|(saved_at, _)| *saved_at == file.saved_at())
                    .map(|(_, heading)| heading.clone());
                let heading = read.unwrap_or_else(|| {
                    let heading = self.store.read_round_heading(file);
                    headings.insert(file.path().to_owned(), (file.saved_at(), heading.clone()));
                    heading
                });
                Some((file.saved_at(), heading?))
            })
            .collect()
    }
}

/// Keeps the list of saved rounds current: each [`refresh`](Self::refresh) has a thread of its
/// own read the saved rounds again and publish their list, so that the reviewer's other work
/// never waits for it. Refreshes that come while it reads are read once. The thread ends once
/// every clone is dropped.
#[derive(Clone)]
pub struct PastRoundsRefresh(mpsc::Sender<()>);

impl PastRoundsRefresh {
    /// Publishes the list of `reader`'s rounds through `publisher` now, and at each refresh.
    fn start(reader: Arc<SavedRoundsReader>, publisher: PastRoundsPublisher) -> Self {
        let (refresh, refreshes) = mpsc::channel::<()>();
        // Without its thread the list stays empty: the page lists no past round.
        let _ = std::thread::Builder::new()
            .name("past-rounds".into())
            .spawn(move || {
                publisher.publish(reader.list());
                while refreshes.recv().is_ok() {
                    while refreshes.try_recv().is_ok() {}
                    publisher.publish(reader.list());
                }
            });
        Self(refresh)
    }

    /// The saved rounds changed on disk: their list is read again.
    pub fn refresh(&self) {
        let _ = self.0.send(());
    }
}

/// The page's view of `round`, a saved round of the change `review`, whose review's threads are
/// `threads`, last saved at `saved_at_ms`: read only, its citations read at its checkpoint in
/// `repository`.
fn past_round(
    round: &ExploreRound,
    repository: &Repository,
    review: ReviewName,
    threads: Option<review_threads::ReviewThreads>,
    saved_at_ms: u64,
) -> PastRound {
    let exploration = &round.exploration;
    let comparison = &exploration.comparison;
    let code = CitedCode::Saved(repository);
    let mut citations = PageCitations::default();
    let stage = saved_stage(round, &mut citations, code);
    let overview = RoundOverview::of(round, None);
    let lists: Vec<Vec<EvidenceRef>> = overview
        .earlier
        .iter()
        .map(|record| record.question.evidence.clone())
        .collect();
    let earlier_citations = PageCitations::default().lists(&lists, comparison, code);
    let published = PublishedRound {
        id: &exploration.instance,
        review_unit: &comparison.checkpoint.review_unit,
        design: exploration.design(),
        changed_files: comparison.files.len(),
        author: exploration.author,
        // A past round offers no action, whatever these say.
        cancellable: None,
        earlier: true,
        overview: &overview,
        earlier_citations: &earlier_citations,
    };
    PastRound::new(published, stage, review, threads, saved_at_ms)
}

/// Where `round` stopped, as the page shows the session's round once nothing in this process
/// works on it: a turn it saved as pending, or one waiting for Retry, as interrupted; otherwise
/// the question or the conclusion of its latest turn.
fn saved_stage(
    round: &ExploreRound,
    citations: &mut PageCitations,
    code: CitedCode<'_>,
) -> RoundStage {
    let exploration = &round.exploration;
    if let Some(request) = exploration
        .pending_request()
        .or_else(|| exploration.retry_request())
    {
        let answer = sent_answer(round, request, citations, code);
        return interrupted(round, request, answer);
    }
    latest_turn(
        round,
        LatestTurn {
            sending: false,
            takes_quiz_answers: false,
            paths: &BTreeMap::new(),
        },
        citations,
        code,
    )
}

/// What the list calls the change `heading` reviewed: the first line of its description as the
/// round saved it, "Git working tree" for a Git working tree.
fn title(heading: &RoundHeading) -> String {
    heading
        .base
        .as_ref()
        .map(|base| base.title().trim())
        .filter(|title| !title.is_empty())
        .unwrap_or(NO_DESCRIPTION)
        .to_owned()
}

/// `time` in milliseconds since the epoch.
fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |elapsed| {
        u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
    })
}
