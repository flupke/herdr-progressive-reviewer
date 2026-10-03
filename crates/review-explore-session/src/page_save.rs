//! What the reviewer saves in the round from the Explore page, beside the turns: the diagrams
//! the page could not draw, and the answers to a conclusion's quiz.

use review_explore::ExploreRound;
use review_explore_page::CommandRefusal;

use crate::{ExploreSession, publish_committed};

impl ExploreSession {
    /// Saves `change` to the round the session shows. `change` returns whether it changed the
    /// round, or why it refuses: a refusal is stale, since the page may show a round the
    /// reviewer has left since.
    pub(crate) fn save_from_page(
        &mut self,
        change: impl FnOnce(&mut ExploreRound) -> Result<bool, String>,
    ) -> Result<(), CommandRefusal> {
        let shown = self.state.round.as_ref().ok_or(CommandRefusal::Stale)?;
        if self.state.historical {
            return Err(CommandRefusal::Stale);
        }
        if let Some(storage) = &self.state.storage_error {
            return Err(CommandRefusal::Failed(storage.clone()));
        }
        let unit = shown.exploration.comparison.checkpoint.review_unit.clone();
        let instance = shown.exploration.instance.clone();
        match self.rounds.update(&unit, &instance, change) {
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
