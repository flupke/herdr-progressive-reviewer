//! Keep parallel fixture processes from choosing the same listening port.

use std::fs::{File, OpenOptions};
use std::net::{Ipv4Addr, TcpListener};

use fs2::FileExt;

/// A cross-process port lease, retained even while a test restarts its server.
pub struct TestPort {
    port: u16,
    _lock: File,
}

impl Default for TestPort {
    fn default() -> Self {
        Self::new()
    }
}

impl TestPort {
    /// Allocate outside the usual ephemeral client range, avoiding live listeners.
    pub fn new() -> Self {
        for offset in 0..20_000 {
            let port = 10_000 + u16::try_from((std::process::id() + offset) % 20_000).unwrap();
            if let Some(lease) = Self::reserve(port) {
                return lease;
            }
        }
        panic!("No free private test port");
    }

    /// Lease a derived endpoint without changing the discovery route under test.
    pub fn reserve(port: u16) -> Option<Self> {
        assert_ne!(port, 0, "A port lease needs a fixed port number");
        let directory = std::env::temp_dir().join("progressive-reviewer-test-ports");
        std::fs::create_dir_all(&directory).unwrap();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(directory.join(port.to_string()))
            .unwrap();
        match lock.try_lock_exclusive() {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return None,
            Err(error) => panic!("Cannot lock a private test port: {error}"),
        }
        match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
            Ok(_) => Some(Self { port, _lock: lock }),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => None,
            Err(error) => panic!("Cannot reserve a private test port: {error}"),
        }
    }

    pub fn number(&self) -> u16 {
        self.port
    }
}
