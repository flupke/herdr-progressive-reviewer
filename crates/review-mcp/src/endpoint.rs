use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::Path;

use sha2::{Digest, Sha256};

/// A stable, loopback-only endpoint for a project, with an explicit port override.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Endpoint {
    port: u16,
}

impl Endpoint {
    pub fn for_repository(root: &Path, port: Option<u16>) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        let hash = Sha256::digest(root.as_os_str().as_encoded_bytes());
        let port = port.unwrap_or(49152 + (u16::from_be_bytes([hash[0], hash[1]]) % 16384));
        if port == 0 {
            return Err("The configured MCP port must be nonzero".into());
        }
        Ok(Self { port })
    }

    pub fn from_env(root: &Path) -> Result<Self, String> {
        let port = std::env::var("HERDR_REVIEWER_MCP_PORT")
            .ok()
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| "HERDR_REVIEWER_MCP_PORT must be a port number")
            })
            .transpose()?;
        Self::for_repository(root, port)
    }

    pub fn address(self) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.port))
    }

    pub fn url(self) -> String {
        format!("http://{}/mcp", self.address())
    }
}
