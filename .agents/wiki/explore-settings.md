# Explore settings

Where the settings of Explore rounds and of the Explore page are saved, and when a change reaches another open reviewer.

- They are saved with the reviewer's other settings, in `settings.json` in
  `$HERDR_PLUGIN_STATE_DIR`, which the reviewers of the machine share
  (`ExploreRoundSettings` in
  [`crates/review-explore-round-settings`](../../crates/review-explore-round-settings),
  `ExplorePageSettings` in
  [`crates/review-explore-page-settings`](../../crates/review-explore-page-settings)).
- No setting needs the reviewer to be reopened. Another reviewer that is already open does
  not see a change at once: it keeps its own settings, and network access as it was, until
  it is reopened or saves a change of its own; then it applies the saved settings.
- [Run-ahead](run-ahead.md) says what run-ahead costs and needs.
- [Explore page: network and tunnel](explore-page-sharing.md) says what the network
  listener and the tunnel expose.
