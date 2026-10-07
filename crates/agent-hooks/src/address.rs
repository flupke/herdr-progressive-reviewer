//! Unix sockets at paths of any length. A socket's address holds a path of at most 107 bytes,
//! and a runtime directory can be deeper, as a test's is: a longer path is reached through a
//! descriptor of its directory, as `/proc/self/fd/<descriptor>/<name>`, which Linux resolves to
//! that directory (systemd connects to long paths this way).

use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

/// The size of a socket address's path, its closing zero included.
const ADDRESS_PATH: usize = 108;

/// Listens at `path`.
pub(crate) fn bind(path: &Path) -> io::Result<UnixListener> {
    through_directory(path, UnixListener::bind)
}

/// Connects to the socket at `path`.
pub(crate) fn connect(path: &Path) -> io::Result<UnixStream> {
    through_directory(path, UnixStream::connect)
}

/// `open` of `path`, or of a path through a descriptor of its directory when `path` is too
/// long for a socket's address.
fn through_directory<T>(path: &Path, open: impl FnOnce(PathBuf) -> io::Result<T>) -> io::Result<T> {
    let (Some(directory), Some(name)) = (path.parent(), path.file_name()) else {
        return open(path.to_owned());
    };
    if path.as_os_str().len() < ADDRESS_PATH {
        return open(path.to_owned());
    }
    let directory = File::open(directory)?;
    open(
        Path::new("/proc/self/fd")
            .join(directory.as_raw_fd().to_string())
            .join(name),
    )
}
