//! Network endpoint configuration for native backends
//!
//! This module provides the [`Endpoint`] type for specifying network addresses
//! when creating native backends. Endpoints can be IPv4/IPv6 addresses or Unix
//! domain sockets.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::ptr;

use crate::ffi::{vrt_endpoint, VRT_ENDPOINT_MAGIC};
use crate::vcl::{IntoVCL, VclResult, Workspace};

/// Network endpoint configuration for a backend
///
/// An endpoint specifies where Varnish should connect when using a native backend.
/// You can specify IPv4, IPv6, or Unix domain socket addresses.
///
/// # Example
///
/// ```ignore
/// use std::net::SocketAddr;
/// use varnish::vcl::Endpoint;
///
/// // IPv4 endpoint
/// let ep = Endpoint::ip("127.0.0.1:8080".parse().unwrap());
///
/// // IPv6 endpoint
/// let ep = Endpoint::ip("[::1]:8080".parse().unwrap());
///
/// // Unix domain socket
/// let ep = Endpoint::uds("/var/run/backend.sock");
/// ```
#[derive(Debug, Clone, Default)]
pub struct Endpoint {
    /// IPv4 address and port
    pub ipv4: Option<SocketAddr>,
    /// IPv6 address and port
    pub ipv6: Option<SocketAddr>,
    /// Unix domain socket path
    pub uds_path: Option<PathBuf>,
    /// PROXY protocol preamble (v1 or v2)
    pub preamble: Option<Vec<u8>>,
}

impl Endpoint {
    /// Create an endpoint from an IPv4 socket address
    ///
    /// # Panics
    /// Panics if the address is not IPv4
    pub fn ipv4(addr: SocketAddr) -> Self {
        assert!(addr.is_ipv4(), "Expected IPv4 address");
        Self {
            ipv4: Some(addr),
            ..Default::default()
        }
    }

    /// Create an endpoint from an IPv6 socket address
    ///
    /// # Panics
    /// Panics if the address is not IPv6
    pub fn ipv6(addr: SocketAddr) -> Self {
        assert!(addr.is_ipv6(), "Expected IPv6 address");
        Self {
            ipv6: Some(addr),
            ..Default::default()
        }
    }

    /// Create an endpoint from an IP socket address (auto-detects v4/v6)
    pub fn ip(addr: SocketAddr) -> Self {
        match addr {
            SocketAddr::V4(_) => Self {
                ipv4: Some(addr),
                ..Default::default()
            },
            SocketAddr::V6(_) => Self {
                ipv6: Some(addr),
                ..Default::default()
            },
        }
    }

    /// Create an endpoint from a Unix domain socket path
    pub fn uds(path: impl Into<PathBuf>) -> Self {
        Self {
            uds_path: Some(path.into()),
            ..Default::default()
        }
    }

    /// Set both IPv4 and IPv6 addresses for dual-stack support
    pub fn dual_stack(ipv4: SocketAddr, ipv6: SocketAddr) -> Self {
        assert!(ipv4.is_ipv4(), "First argument must be IPv4");
        assert!(ipv6.is_ipv6(), "Second argument must be IPv6");
        Self {
            ipv4: Some(ipv4),
            ipv6: Some(ipv6),
            ..Default::default()
        }
    }

    /// Add a PROXY protocol preamble to the endpoint
    #[must_use]
    pub fn with_preamble(mut self, preamble: Vec<u8>) -> Self {
        self.preamble = Some(preamble);
        self
    }

    /// Convert to FFI `vrt_endpoint`, allocating in workspace
    pub(crate) fn to_ffi(&self, ws: &mut Workspace) -> VclResult<*const vrt_endpoint> {
        // At least one of ipv4, ipv6, or uds_path must be set
        if self.ipv4.is_none() && self.ipv6.is_none() && self.uds_path.is_none() {
            return Err("Endpoint must have at least one of: ipv4, ipv6, or uds_path".into());
        }

        // Convert IP addresses to VCL_IP
        let ipv4 = match self.ipv4 {
            Some(addr) => addr.into_vcl(ws)?,
            None => crate::ffi::VCL_IP(ptr::null()),
        };

        let ipv6 = match self.ipv6 {
            Some(addr) => addr.into_vcl(ws)?,
            None => crate::ffi::VCL_IP(ptr::null()),
        };

        // Convert UDS path to C string
        let uds_path = match &self.uds_path {
            Some(path) => {
                let path_str = path
                    .to_str()
                    .ok_or("Unix socket path contains invalid UTF-8")?;
                path_str.into_vcl(ws)?.0
            }
            None => ptr::null(),
        };

        // Convert preamble to vrt_blob
        let preamble = match &self.preamble {
            Some(data) => {
                let blob = ws.copy_blob(data)?;
                blob.0
            }
            None => ptr::null(),
        };

        // Allocate and populate vrt_endpoint in workspace
        let endpoint = ws.copy_value(vrt_endpoint {
            magic: VRT_ENDPOINT_MAGIC,
            ipv4,
            ipv6,
            uds_path,
            preamble,
            sslflags: 0,
            hosthdr: ptr::null(),
        })?;

        Ok(ptr::from_ref(endpoint))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_ipv4() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let ep = Endpoint::ip(addr);
        assert_eq!(ep.ipv4, Some(addr));
        assert_eq!(ep.ipv6, None);
        assert_eq!(ep.uds_path, None);
    }

    #[test]
    fn test_endpoint_ipv6() {
        let addr: SocketAddr = "[::1]:8080".parse().unwrap();
        let ep = Endpoint::ip(addr);
        assert_eq!(ep.ipv4, None);
        assert_eq!(ep.ipv6, Some(addr));
        assert_eq!(ep.uds_path, None);
    }

    #[test]
    fn test_endpoint_uds() {
        let ep = Endpoint::uds("/var/run/backend.sock");
        assert_eq!(ep.ipv4, None);
        assert_eq!(ep.ipv6, None);
        assert_eq!(ep.uds_path, Some(PathBuf::from("/var/run/backend.sock")));
    }

    #[test]
    fn test_endpoint_dual_stack() {
        let v4: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let v6: SocketAddr = "[::1]:8080".parse().unwrap();
        let ep = Endpoint::dual_stack(v4, v6);
        assert_eq!(ep.ipv4, Some(v4));
        assert_eq!(ep.ipv6, Some(v6));
    }

    #[test]
    fn test_endpoint_with_preamble() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let preamble = b"PROXY TCP4 1.2.3.4 5.6.7.8 12345 80\r\n".to_vec();
        let ep = Endpoint::ip(addr).with_preamble(preamble.clone());
        assert_eq!(ep.preamble, Some(preamble));
    }
}
