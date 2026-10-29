//! The rich data set's past rounds: as many as a repository reviewed for weeks keeps, several
//! rounds of one change on the same day, a title too long for a line, one with no space to break
//! at, a change with no description, and a round of the year before.

use crate::round_data::{
    AT_FIRST_QUESTION, AT_SECOND_QUESTION, CONCLUDED, DAY, MINUTE, SavedRound,
};

const NUMBERED_DIFFS: &str = "Give the Explore agent numbered diffs of the unreviewed lines";
const FLUSH_POLICY: &str = "Name the queue's flush policy";

pub(super) const SAVED_ROUNDS: &[SavedRound] = &[
    SavedRound {
        title: "Keep the reviewer's draft across reloads",
        revision: ("kq", "vmtsxy"),
        minutes_ago: DAY,
        steps: CONCLUDED,
    },
    SavedRound {
        title: FLUSH_POLICY,
        revision: ("pw", "nlzrot"),
        minutes_ago: 3 * DAY,
        steps: AT_SECOND_QUESTION,
    },
    SavedRound {
        title: FLUSH_POLICY,
        revision: ("pw", "nlzrot"),
        minutes_ago: 3 * DAY + 32 * MINUTE,
        steps: AT_FIRST_QUESTION,
    },
    SavedRound {
        title: "Make the follow-up fixes the final review of the Explore page left as decisions, \
                and say why each one holds",
        revision: ("l", "lnkyqvq"),
        minutes_ago: 4 * DAY,
        steps: CONCLUDED,
    },
    SavedRound {
        title: "No description",
        revision: ("zw", "pvrrqx"),
        minutes_ago: 5 * DAY + 180 * MINUTE,
        steps: AT_FIRST_QUESTION,
    },
    SavedRound {
        title: NUMBERED_DIFFS,
        revision: ("zu", "rxznpq"),
        minutes_ago: 7 * DAY,
        steps: CONCLUDED,
    },
    SavedRound {
        title: NUMBERED_DIFFS,
        revision: ("zu", "rxznpq"),
        minutes_ago: 7 * DAY + 28 * MINUTE,
        steps: AT_SECOND_QUESTION,
    },
    SavedRound {
        title: NUMBERED_DIFFS,
        revision: ("zu", "rxznpq"),
        minutes_ago: 7 * DAY + 30 * MINUTE,
        steps: AT_FIRST_QUESTION,
    },
    SavedRound {
        title: NUMBERED_DIFFS,
        revision: ("zu", "rxznpq"),
        minutes_ago: 7 * DAY + 60 * MINUTE,
        steps: AT_FIRST_QUESTION,
    },
    SavedRound {
        title: NUMBERED_DIFFS,
        revision: ("zu", "rxznpq"),
        minutes_ago: 7 * DAY + 83 * MINUTE,
        steps: AT_FIRST_QUESTION,
    },
    SavedRound {
        title: "Rename review_explore_page_server::round_data::RoundData::conclusion_stage_with_quiz",
        revision: ("mx", "tqowpe"),
        minutes_ago: 9 * DAY,
        steps: CONCLUDED,
    },
    SavedRound {
        title: "Script an Explore agent for make vision and fix what it found",
        revision: ("zz", "wpvrrq"),
        minutes_ago: 12 * DAY + 300 * MINUTE,
        steps: CONCLUDED,
    },
    SavedRound {
        title: "Show why the revision selector cannot compute a share",
        revision: ("uw", "susurm"),
        minutes_ago: 26 * DAY,
        steps: AT_SECOND_QUESTION,
    },
    SavedRound {
        title: "Draw the meter's share in the tab's icon",
        revision: ("ry", "kmovst"),
        minutes_ago: 400 * DAY,
        steps: AT_FIRST_QUESTION,
    },
];
