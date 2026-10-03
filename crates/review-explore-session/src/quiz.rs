//! The reviewer's answers to a conclusion's quiz on the Explore page: saved with the round.

use review_explore_page::{CommandRefusal, PageQuizResponse};

use crate::ExploreSession;

impl ExploreSession {
    /// Saves the reviewer's pick of a quiz item, or skip of the quiz, in the round the session
    /// shows. A response the round no longer takes is stale: the conclusion is no longer
    /// current, or the item has another pick, from another page.
    pub(crate) fn quiz_from_page(
        &mut self,
        response: PageQuizResponse,
    ) -> Result<(), CommandRefusal> {
        let PageQuizResponse {
            conclusion,
            response,
        } = response;
        self.save_from_page(|saved| {
            saved
                .exploration
                .answer_quiz(&conclusion, response)
                .map_err(|error| error.to_string())
        })
    }
}
