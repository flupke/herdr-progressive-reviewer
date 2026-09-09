//! In-process reads of pinned jj commits; workspace mutations remain with the CLI.

use futures::{AsyncReadExt, executor::block_on};
use jj_lib::backend::CommitId;
use jj_lib::commit::Commit;
use jj_lib::config::{ConfigLayer, ConfigSource, StackedConfig};
use jj_lib::conflicts::{
    ConflictMaterializeOptions, MaterializedTreeValue, materialize_tree_value,
};
use jj_lib::default_backend_factories::{
    default_backend_factories, default_working_copy_factories,
};
use jj_lib::diff_presentation::unified::{GitDiffPart, git_diff_part};
use jj_lib::merged_tree::MergedTree;
use jj_lib::repo::RepoLoader;
use jj_lib::repo_path::RepoPath as JjPath;
use jj_lib::settings::UserSettings;
use jj_lib::workspace::Workspace;

use super::jj_patch::GitPatch;
use super::{COMMAND_OUTPUT_LIMIT, ChangedFile, RepoPath, Repository, Snapshot};
use crate::{Error, Result};

pub(super) struct JjReader {
    loader: RepoLoader,
    materialize: ConflictMaterializeOptions,
    context: usize,
}

impl std::fmt::Debug for JjReader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("JjReader").finish_non_exhaustive()
    }
}

impl JjReader {
    pub(super) fn open(repository: &Repository) -> Result<Self> {
        let output = repository.run_jj([
            "--ignore-working-copy",
            "config",
            "list",
            "--include-defaults",
            "--template",
            r#"name ++ " = " ++ value ++ "\n""#,
        ])?;
        let text =
            std::str::from_utf8(&output.stdout).map_err(|_| Self::error("read configuration"))?;
        let mut config = StackedConfig::with_defaults();
        config.add_layer(
            ConfigLayer::parse(ConfigSource::CommandArg, text)
                .map_err(|_| Self::error("parse configuration"))?,
        );
        let settings =
            UserSettings::from_config(config).map_err(|_| Self::error("load settings"))?;
        let workspace = Workspace::load(
            &settings,
            repository.root(),
            &default_backend_factories(),
            &default_working_copy_factories(),
        )
        .map_err(|_| Self::error("open workspace"))?;
        let loader = workspace.repo_loader().clone();
        let materialize = ConflictMaterializeOptions {
            marker_style: settings
                .get("ui.conflict-marker-style")
                .map_err(|_| Self::error("read conflict style"))?,
            marker_len: None,
            merge: loader.store().merge_options().clone(),
        };
        let context = settings
            .get("diff.git.context")
            .map_err(|_| Self::error("read diff context"))?;
        Ok(Self {
            loader,
            materialize,
            context,
        })
    }

    pub(super) fn file_at(
        &self,
        repository: &Repository,
        revision: &str,
        path: &RepoPath,
    ) -> Result<Vec<u8>> {
        block_on(async {
            Self::check_cancelled(repository)?;
            let commit = self.commit(revision).await?;
            let part = self.part(repository, &commit.tree(), Some(path)).await?;
            if part.mode.is_none() {
                return Err(Self::error("read missing file"));
            }
            Ok(part.content.contents.to_vec())
        })
    }

    pub(super) fn base_file_at(
        &self,
        repository: &Repository,
        snapshot: &Snapshot,
        path: &RepoPath,
    ) -> Result<Vec<u8>> {
        block_on(async {
            Self::check_cancelled(repository)?;
            let commit = self.commit(snapshot.identity.snapshot_id()).await?;
            let tree = self.parent_tree(&commit).await?;
            let part = self.part(repository, &tree, Some(path)).await?;
            if part.mode.is_none() {
                return Err(Self::error("read missing parent file"));
            }
            Ok(part.content.contents.to_vec())
        })
    }

    pub(super) fn diff(
        &self,
        repository: &Repository,
        snapshot: &Snapshot,
        file: &ChangedFile,
    ) -> Result<Vec<u8>> {
        block_on(async {
            Self::check_cancelled(repository)?;
            let commit = self.commit(snapshot.identity.snapshot_id()).await?;
            let parent = self.parent_tree(&commit).await?;
            let before = self
                .part(repository, &parent, file.old_path.as_ref())
                .await?;
            let after = self
                .part(repository, &commit.tree(), file.new_path.as_ref())
                .await?;
            let patch = GitPatch::new(file, before, after, self.context).render();
            Self::check_size(repository, patch.len())?;
            Ok(patch)
        })
    }

    async fn commit(&self, revision: &str) -> Result<Commit> {
        let id =
            CommitId::try_from_hex(revision).ok_or_else(|| Self::error("parse pinned commit"))?;
        self.loader
            .store()
            .get_commit_async(&id)
            .await
            .map_err(|_| Self::error("read commit"))
    }

    async fn parent_tree(&self, commit: &Commit) -> Result<MergedTree> {
        match commit.parent_ids() {
            [] => Ok(self.loader.store().empty_merged_tree()),
            [parent] => self
                .loader
                .store()
                .get_commit_async(parent)
                .await
                .map(|commit| commit.tree())
                .map_err(|_| Self::error("read parent commit")),
            _ => {
                // Loading a specific operation avoids load_at_head(), which may
                // reconcile concurrent operations and write repository state.
                let heads = self
                    .loader
                    .op_heads_store()
                    .get_op_heads()
                    .await
                    .map_err(|_| Self::error("read operation heads"))?;
                let [head] = heads.as_slice() else {
                    return Err(Self::error("read concurrent operation heads"));
                };
                let operation = self
                    .loader
                    .load_operation(head)
                    .await
                    .map_err(|_| Self::error("read operation"))?;
                let repo = self
                    .loader
                    .load_at(&operation)
                    .await
                    .map_err(|_| Self::error("load operation"))?;
                commit
                    .parent_tree(repo.as_ref())
                    .await
                    .map_err(|_| Self::error("merge parent trees"))
            }
        }
    }

    async fn part(
        &self,
        repository: &Repository,
        tree: &MergedTree,
        path: Option<&RepoPath>,
    ) -> Result<GitDiffPart> {
        Self::check_cancelled(repository)?;
        let Some(path) = path else {
            return git_diff_part(
                JjPath::root(),
                MaterializedTreeValue::Absent,
                &self.materialize,
            )
            .await
            .map_err(|_| Self::error("read absent file"));
        };
        let path = std::str::from_utf8(path.as_bytes()).map_err(|_| Self::error("decode path"))?;
        let path = JjPath::from_internal_string(path).map_err(|_| Self::error("parse path"))?;
        let value = tree
            .path_value(path)
            .await
            .map_err(|_| Self::error("read tree path"))?;
        let mut value = materialize_tree_value(self.loader.store(), path, value, tree.labels())
            .await
            .map_err(|_| Self::error("materialize file"))?;
        if matches!(value, MaterializedTreeValue::Tree(_)) {
            return Err(Self::error("read directory as file"));
        }
        if let MaterializedTreeValue::File(file) = &mut value {
            let reader = std::mem::replace(&mut file.reader, Box::pin(futures::io::empty()));
            file.reader = Box::pin(reader.take((COMMAND_OUTPUT_LIMIT + 1) as u64));
        }
        let part = git_diff_part(path, value, &self.materialize)
            .await
            .map_err(|_| Self::error("read file contents"))?;
        Self::check_cancelled(repository)?;
        Self::check_size(repository, part.content.contents.len())?;
        Ok(part)
    }

    fn check_cancelled(repository: &Repository) -> Result<()> {
        if repository.cancellation.is_cancelled() {
            return Err(Error::CommandCancelled {
                operation: "read jj repository".to_owned(),
                path: repository.root().to_owned(),
            });
        }
        Ok(())
    }

    fn check_size(repository: &Repository, bytes: usize) -> Result<()> {
        if bytes > COMMAND_OUTPUT_LIMIT {
            return Err(Error::CommandOutputTooLarge {
                operation: "read jj repository".to_owned(),
                path: repository.root().to_owned(),
            });
        }
        Ok(())
    }

    fn error(operation: &'static str) -> Error {
        Error::JjLibrary { operation }
    }
}
