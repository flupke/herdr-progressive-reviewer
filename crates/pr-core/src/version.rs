//! External tool version checks.

use std::ffi::{OsStr, OsString};
use std::process::Command;

use semver::Version;

use crate::{Error, Result};

/// The first Herdr release with all required live-agent operations.
pub const MINIMUM_HERDR_VERSION: &str = "0.7.5";

/// The oldest jj release that supports the reviewer commands.
pub const MINIMUM_JJ_VERSION: &str = "0.43.0";

/// Installed versions that passed the minimum checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeVersions {
    /// The installed Herdr version.
    pub herdr: Version,
    /// The installed jj version.
    pub jj: Version,
}

/// Check the installed Herdr and jj executable versions.
pub fn check_runtime_versions(
    herdr_program: impl Into<OsString>,
    jj_program: impl Into<OsString>,
) -> Result<RuntimeVersions> {
    let herdr = check_tool(
        "Herdr version check",
        herdr_program.into(),
        "herdr",
        MINIMUM_HERDR_VERSION,
    )?;
    let jj = check_tool(
        "jj version check",
        jj_program.into(),
        "jj",
        MINIMUM_JJ_VERSION,
    )?;

    Ok(RuntimeVersions { herdr, jj })
}

fn check_tool(
    operation: &str,
    program: OsString,
    tool: &'static str,
    minimum: &str,
) -> Result<Version> {
    let output = Command::new(&program)
        .arg("--version")
        .output()
        .map_err(|source| Error::Spawn {
            operation: operation.to_owned(),
            program,
            current_dir: None,
            source,
        })?;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            operation: operation.to_owned(),
            code: output.status.code(),
        });
    }

    let found = parse_version(tool, &output.stdout)?;
    let minimum = Version::parse(minimum).expect("minimum tool versions must be valid semver");
    if found < minimum {
        return Err(Error::UnsupportedVersion {
            tool,
            minimum,
            found,
        });
    }

    Ok(found)
}

fn parse_version(tool: &'static str, output: &[u8]) -> Result<Version> {
    let output = std::str::from_utf8(output).map_err(|_| Error::InvalidVersion { tool })?;
    let mut fields = output.split_ascii_whitespace();
    let reported_tool = fields.next().ok_or(Error::InvalidVersion { tool })?;
    let version = fields.next().ok_or(Error::InvalidVersion { tool })?;
    if reported_tool != tool {
        return Err(Error::InvalidVersion { tool });
    }

    Version::parse(version).map_err(|_| Error::InvalidVersion { tool })
}

/// Get the default Herdr executable from the plugin environment.
pub fn herdr_program_from_environment() -> OsString {
    std::env::var_os("HERDR_BIN_PATH").unwrap_or_else(|| OsStr::new("herdr").to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_supported_version() {
        let version = parse_version("jj", b"jj 0.43.0\n").unwrap();

        assert_eq!(version, Version::new(0, 43, 0));
    }

    #[test]
    fn rejects_an_unexpected_version_shape() {
        let error = parse_version("jj", b"unexpected 0.43.0 repository text").unwrap_err();

        assert!(matches!(error, Error::InvalidVersion { tool: "jj" }));
        assert!(!error.to_string().contains("repository text"));
    }
}
