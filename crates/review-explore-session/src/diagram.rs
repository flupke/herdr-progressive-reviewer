//! Diagrams the Explore page could not draw: saved with their question in the round.

use review_explore::DiagramError;
use review_explore_page::CommandRefusal;

use crate::ExploreSession;

impl ExploreSession {
    /// Saves `error` with its question in the round the session shows. A question that round
    /// never posted is stale: the page may show a round the reviewer has left since.
    pub(crate) fn diagram_failed(&mut self, error: DiagramError) -> Result<(), CommandRefusal> {
        self.save_from_page(|saved| {
            saved
                .exploration
                .record_diagram_error(error)
                .map_err(|error| error.to_string())
        })
    }
}
