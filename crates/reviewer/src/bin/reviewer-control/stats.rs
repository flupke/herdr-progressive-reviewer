//! `reviewer-control stats`: the numbers of the saved Explore rounds of the
//! repository the command runs in. It merges the review states of the repository's
//! workspaces first, as the review pane does, and reads the rest.
use std::env;
use std::path::PathBuf;

use review_explore_stats::{Period, Report};
use review_repository::repository::Repository;
use review_store::{ReviewStore, SavedRounds};

const USAGE: &str = "usage: reviewer-control stats [--since DATE|TIME] [--until DATE|TIME]";

/// The plugin's ID, which names the state directory Herdr gives it.
const PLUGIN_ID: &str = "herdr.progressive-reviewer";

/// The command and the period it limits the numbers to, if any.
pub(super) struct Stats {
    period: Option<Period>,
}

impl Stats {
    pub(super) fn from_args(mut arguments: impl Iterator<Item = String>) -> eyre::Result<Self> {
        let (mut since, mut until) = (None, None);
        while let Some(argument) = arguments.next() {
            let bound = match argument.as_str() {
                "--since" => &mut since,
                "--until" => &mut until,
                _ => return Err(eyre::eyre!(USAGE)),
            };
            *bound = Some(arguments.next().ok_or_else(|| eyre::eyre!(USAGE))?);
        }
        let period = (since.is_some() || until.is_some())
            .then(|| Period::parse(since.as_deref(), until.as_deref()))
            .transpose()?;
        Ok(Self { period })
    }

    pub(super) fn run(self) -> eyre::Result<()> {
        let repository = Repository::discover(env::current_dir()?)?;
        let state = Self::state_directory()?;
        // Without a state directory there is nothing to count, and nothing to create.
        let saved = if state.exists() {
            let store = ReviewStore::open(state, repository.shared_store()?)?;
            review_state_migration::migrate(&repository, &store)?;
            store.saved_explore_rounds()?
        } else {
            SavedRounds::default()
        };
        println!("Explore rounds of {}\n", repository.root().display());
        print!("{}", Report::new(&saved, self.period));
        Ok(())
    }

    /// Herdr's state directory for the plugin, as the reviewer pane gets it,
    /// or where Herdr puts it when the command runs outside Herdr.
    fn state_directory() -> eyre::Result<PathBuf> {
        if let Some(directory) = env::var_os("HERDR_PLUGIN_STATE_DIR") {
            return Ok(directory.into());
        }
        let state_home = env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|directory| directory.is_absolute())
            .or_else(|| env::home_dir().map(|home| home.join(".local/state")))
            .ok_or_else(|| eyre::eyre!("set HERDR_PLUGIN_STATE_DIR: no home directory"))?;
        Ok(state_home.join("herdr/plugins").join(PLUGIN_ID))
    }
}
