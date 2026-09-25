use review_threads::{MessageId, ReviewThreads, ThreadId};
use review_types::ReviewUnit;

/// Paste input delivered to the focused component.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextPasted(pub String);

/// The persisted thread history for a logical review.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewThreadsLoaded {
    pub review_unit: ReviewUnit,
    pub result: Result<ReviewThreads, String>,
}

/// The durable outcome of an explicit reviewer post.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadPostFinished {
    pub review_unit: ReviewUnit,
    pub message_id: MessageId,
    pub result: Result<(), String>,
}

/// The two review-wide navigation modes, independent of keyboard focus.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReviewNavigation {
    #[default]
    Files,
    Threads,
    Explore,
}

impl ReviewNavigation {
    #[allow(clippy::return_self_not_must_use)]
    pub fn next(self) -> Self {
        match self {
            Self::Files => Self::Threads,
            Self::Threads => Self::Explore,
            Self::Explore => Self::Files,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReviewNavigationChanged(pub ReviewNavigation);

/// Select or clear a conversation without selecting or loading its anchored file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadSelectionChanged {
    pub thread_id: Option<ThreadId>,
}

/// Select All and jump to the first unread thread, including resolved threads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NewRepliesRequested;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewPane {
    Navigation,
    Detail,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReviewPaneFocusRequested(pub ReviewPane);

/// Current placement is a projection, separate from the immutable saved context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadContext {
    Current,
    Hidden,
    Earlier,
    OutsideDiff,
    Original,
    Unavailable,
}

impl ThreadContext {
    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "Current code",
            Self::Hidden => "Hidden code",
            Self::Earlier => "Earlier code",
            Self::OutsideDiff => "Outside diff",
            Self::Original => "Saved context",
            Self::Unavailable => "Unavailable",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadContextsChanged {
    pub review_unit: ReviewUnit,
    pub contexts: std::collections::HashMap<ThreadId, ThreadContext>,
}

/// Paths retained solely because they contain review threads or unposted drafts.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ThreadFilesChanged {
    pub paths: Vec<String>,
}

impl super::FileSummary {
    /// Build the same file association for thread lists, badges and navigation.
    pub fn thread_paths(files: &[Self]) -> review_threads::ThreadPaths {
        review_threads::ThreadPaths::new(files.iter().map(|file| {
            (
                file.path(),
                (file.file.change == review_repository::repository::ChangeKind::Renamed)
                    .then(|| {
                        file.file
                            .old_path
                            .as_ref()
                            .map(review_repository::repository::RepoPath::display)
                    })
                    .flatten(),
            )
        }))
    }
}
