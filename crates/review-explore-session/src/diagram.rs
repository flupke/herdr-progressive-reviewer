//! Diagrams the Explore page could not draw: saved with their question in the round.

use review_explore::DiagramError;
use review_explore_page::CommandRefusal;

use crate::{ExploreSession, publish_committed};

impl ExploreSession {
    /// Saves `error` with its question in the round the session shows. A question that round
    /// never posted is stale: the page may show a round the reviewer has left since.
    pub(crate) fn diagram_failed(&mut self, error: DiagramError) -> Result<(), CommandRefusal> {
        let shown = self.state.round.as_ref().ok_or(CommandRefusal::Stale)?;
        if self.state.historical {
            return Err(CommandRefusal::Stale);
        }
        if let Some(storage) = &self.state.storage_error {
            return Err(CommandRefusal::Failed(storage.clone()));
        }
        let unit = shown.exploration.comparison.checkpoint.review_unit.clone();
        let instance = shown.exploration.instance.clone();
        let saved = self.rounds.update(&unit, &instance, |saved| {
            saved
                .exploration
                .record_diagram_error(error)
                .map_err(|error| error.to_string())
        });
        match saved {
            Ok((true, saved)) => {
                self.state.round = Some(saved.clone());
                publish_committed(&self.events, saved);
                Ok(())
            }
            Ok((false, _)) => Ok(()),
            Err(review_store::Error::ExploreRefused(_)) => Err(CommandRefusal::Stale),
            Err(error) => {
                self.state.storage_error = Some(error.to_string());
                let _ = self
                    .events
                    .send(ui_events::ExploreStorageFailed(error.to_string()));
                Err(CommandRefusal::Failed(error.to_string()))
            }
        }
    }
}
