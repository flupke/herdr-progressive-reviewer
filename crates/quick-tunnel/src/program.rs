//! The `cloudflared` a tunnel runs, and how.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use agent_fork::Wrapper;

use crate::TunnelFailure;

/// How long `cloudflared` has to print the tunnel's address: it asks Cloudflare for a tunnel,
/// then connects to it, which takes a few seconds.
const ADDRESS_WAIT: Duration = Duration::from_secs(30);

/// The `cloudflared` a tunnel runs: its program, the wrapper that ties its life to the
/// reviewer's, and its environment.
#[derive(Clone, Debug)]
pub struct TunnelProgram {
    /// A program name looked up on the environment's `PATH`, or a path.
    pub program: OsString,
    /// What starts the program: `reviewer-control fork-exec` in the reviewer.
    pub wrapper: Wrapper,
    /// The whole environment of the program.
    pub environment: Vec<(OsString, OsString)>,
    /// Its working directory.
    pub directory: PathBuf,
    /// How long it has to print the tunnel's address.
    pub address_wait: Duration,
}

impl TunnelProgram {
    /// `cloudflared` on the reviewer's `PATH`, in the reviewer's environment, started through
    /// `wrapper`.
    pub fn cloudflared(wrapper: Wrapper) -> Self {
        Self {
            program: "cloudflared".into(),
            wrapper,
            environment: std::env::vars_os().collect(),
            directory: std::env::temp_dir(),
            address_wait: ADDRESS_WAIT,
        }
    }

    /// The arguments of a quick tunnel to `target`. The tunnel ends with the reviewer: no
    /// update of `cloudflared` restarts it.
    pub(crate) fn arguments(target: SocketAddr) -> Vec<OsString> {
        ["tunnel", "--no-autoupdate", "--url"]
            .into_iter()
            .map(OsString::from)
            .chain([format!("http://{target}").into()])
            .collect()
    }

    /// The program's executable: the path it names, or the first executable of that name on
    /// the environment's `PATH`.
    pub(crate) fn find(&self) -> Result<PathBuf, TunnelFailure> {
        let missing = || TunnelFailure::Missing {
            program: self.program.to_string_lossy().into_owned(),
        };
        let program = Path::new(&self.program);
        if program.components().count() > 1 {
            return executable(program)
                .then(|| program.to_owned())
                .ok_or_else(missing);
        }
        let path = self
            .environment
            .iter()
            .find(|(name, _)| name == "PATH")
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        std::env::split_paths(&path)
            .map(|directory| directory.join(program))
            .find(|candidate| executable(candidate))
            .ok_or_else(missing)
    }
}

fn executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}
