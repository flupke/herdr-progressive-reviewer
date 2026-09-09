//! File reads run independently of repository refresh and guide generation.

use super::{
    ApplicationMessageSender, Arc, ChangedFile, DiffContentLoadFailed, DiffContentLoaded,
    EventEnvelope, Path, Receiver, Repository, ReviewCheckpoint, ReviewTracker, Snapshot,
    SourceContentLoadFailed, SourceContentLoaded, SourceLoadMode, SourceLocation, parse_file_diff,
};

#[derive(Debug)]
pub(super) enum Command {
    Snapshot(Snapshot),
    LoadDiff {
        review_checkpoint: ReviewCheckpoint,
        path: String,
    },
    LoadSource {
        snapshot_id: String,
        location: SourceLocation,
        mode: SourceLoadMode,
    },
    Quit,
}

pub(super) struct DocumentWorker {
    pub(super) repository: Repository,
    pub(super) tracker: Arc<ReviewTracker>,
    pub(super) snapshot: Option<Snapshot>,
}

impl DocumentWorker {
    pub(super) fn run(
        &mut self,
        commands: &Receiver<Command>,
        messages: &ApplicationMessageSender,
    ) {
        while let Ok(command) = commands.recv() {
            match command {
                Command::Snapshot(snapshot) => self.snapshot = Some(snapshot),
                Command::LoadDiff {
                    review_checkpoint,
                    path,
                } => self.load_diff(messages, review_checkpoint, path),
                Command::LoadSource {
                    snapshot_id,
                    location,
                    mode,
                } => self.load_source(messages, snapshot_id, location, mode),
                Command::Quit => return,
            }
        }
    }

    pub(super) fn load_source(
        &self,
        messages: &ApplicationMessageSender,
        snapshot_id: String,
        location: SourceLocation,
        mode: SourceLoadMode,
    ) {
        let frozen_content = FrozenSourceLoader {
            repository_root: self.repository.root(),
            tracker: &self.tracker,
            snapshot: self.snapshot.as_ref(),
        }
        .load(&snapshot_id, &location);
        let content = match frozen_content {
            Ok(Some(content)) => Ok(content),
            Ok(None) => std::fs::read(&location.path)
                .map_err(|error| format!("could not read {}: {error}", location.path.display())),
            Err(error) => Err(format!(
                "could not read frozen source {}: {error}",
                location.path.display()
            )),
        };
        let event = match content {
            Err(message) => EventEnvelope::new(SourceContentLoadFailed {
                snapshot_id: snapshot_id.clone(),
                message,
            }),
            Ok(content) => EventEnvelope::new(SourceContentLoaded {
                snapshot_id,
                location,
                content,
                mode,
            }),
        };
        let _ = messages.0.send(event);
    }

    pub(super) fn load_diff(
        &self,
        messages: &ApplicationMessageSender,
        review_checkpoint: ReviewCheckpoint,
        path: String,
    ) {
        let result = self
            .find_file(&review_checkpoint, &path)
            .and_then(|(snapshot, file)| {
                let diff = self.tracker.diff(snapshot, file)?;
                Ok((
                    parse_file_diff(&diff.unified, file),
                    diff.old_content,
                    diff.new_content,
                ))
            });
        let event = match result {
            Ok((rows, old_content, new_content)) => EventEnvelope::new(DiffContentLoaded {
                review_checkpoint,
                path,
                rows,
                old_content,
                new_content,
            }),
            Err(_) => EventEnvelope::new(DiffContentLoadFailed {
                review_checkpoint,
                path,
            }),
        };
        let _ = messages.0.send(event);
    }

    fn find_file<'a>(
        &'a self,
        review_checkpoint: &ReviewCheckpoint,
        path: &str,
    ) -> eyre::Result<(&'a Snapshot, &'a ChangedFile)> {
        let snapshot = self
            .snapshot
            .as_ref()
            .filter(|snapshot| {
                review_checkpoint.matches(
                    snapshot.identity.review_unit(),
                    snapshot.identity.snapshot_id(),
                )
            })
            .ok_or_else(|| eyre::eyre!("the diff snapshot is no longer current"))?;
        let file = snapshot
            .files
            .iter()
            .find(|file| file.review_path().display() == path)
            .ok_or_else(|| eyre::eyre!("the selected file is no longer in the current change"))?;
        Ok((snapshot, file))
    }
}

struct FrozenSourceLoader<'a> {
    repository_root: &'a Path,
    tracker: &'a ReviewTracker,
    snapshot: Option<&'a Snapshot>,
}

enum FrozenSourceSide {
    Old,
    New,
}

impl FrozenSourceLoader<'_> {
    fn load(&self, snapshot_id: &str, location: &SourceLocation) -> eyre::Result<Option<Vec<u8>>> {
        let snapshot = self
            .snapshot
            .filter(|snapshot| snapshot.identity.snapshot_id() == snapshot_id);
        let Some((snapshot, review_path)) =
            snapshot.zip(location.review_path(self.repository_root))
        else {
            return Ok(None);
        };
        let matching_file = snapshot.files.iter().find_map(|file| {
            let old_path_matches = file
                .old_path
                .as_ref()
                .is_some_and(|path| path.display() == review_path);
            let new_path_matches = file
                .new_path
                .as_ref()
                .is_some_and(|path| path.display() == review_path);
            if new_path_matches {
                Some((file, FrozenSourceSide::New))
            } else if old_path_matches {
                Some((file, FrozenSourceSide::Old))
            } else {
                None
            }
        });
        let Some((file, source_side)) = matching_file else {
            return Ok(None);
        };
        let diff = self.tracker.diff(snapshot, file)?;
        let content = match source_side {
            FrozenSourceSide::Old => diff.old_content.or(diff.new_content),
            FrozenSourceSide::New => diff.new_content.or(diff.old_content),
        }
        .ok_or_else(|| eyre::eyre!("the frozen review file has no text content"))?;
        Ok(Some(content))
    }
}
