use std::fs::{self, Permissions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;

use super::document::Configuration;

pub(super) struct ConfigFile {
    path: PathBuf,
    original: Option<String>,
    permissions: Option<Permissions>,
}

impl ConfigFile {
    pub(super) fn install(
        root: &Path,
        path: &Path,
        configuration: &Configuration<'_>,
    ) -> Result<bool, String> {
        // Coordinate installers without adding a lock file to the user's configuration.
        let lock = fs::File::open(root).map_err(|error| error.to_string())?;
        lock.lock_exclusive().map_err(|error| error.to_string())?;
        let config = Self::read(path)?;
        let Some(updated) = configuration.insert(config.text())? else {
            return Ok(false);
        };
        config.write(&updated)?;
        Ok(true)
    }

    fn read(path: &Path) -> Result<Self, String> {
        let parent = path.parent().expect("configuration has a parent");
        if let Ok(metadata) = fs::symlink_metadata(parent)
            && !metadata.is_dir()
        {
            return Err(
                "Configuration directory is not a regular directory; left unchanged".into(),
            );
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() => Some(metadata),
            Ok(_) => return Err("Configuration is not a regular file; left unchanged".into()),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        let original = metadata
            .as_ref()
            .map(|_| fs::read_to_string(path))
            .transpose()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            path: path.to_owned(),
            original,
            permissions: metadata.map(|metadata| metadata.permissions()),
        })
    }

    fn text(&self) -> &str {
        self.original.as_deref().unwrap_or_default()
    }

    fn write(self, updated: &str) -> Result<(), String> {
        let parent = self.path.parent().expect("configuration has a parent");
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
        temporary
            .write_all(updated.as_bytes())
            .map_err(|error| error.to_string())?;
        if let Some(permissions) = self.permissions {
            temporary
                .as_file()
                .set_permissions(permissions)
                .map_err(|error| error.to_string())?;
        }
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        if let Some(original) = self.original {
            let current = Self::read(&self.path)?;
            if current.original.as_ref() != Some(&original) {
                return Err("Configuration changed during setup; retry after saving it".into());
            }
            temporary.persist(&self.path)
        } else {
            temporary.persist_noclobber(&self.path)
        }
        .map_err(|error| error.to_string())?;
        Ok(())
    }
}
