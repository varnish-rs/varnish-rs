//! Example VMOD demonstrating native backend support
//!
//! This VMOD creates backends where Varnish handles all HTTP networking,
//! connection pooling, and health probes. Unlike synthetic backends,
//! no custom HTTP handling is required.

use std::net::SocketAddr;

use varnish::vcl::{Ctx, Endpoint, NativeBackend, NativeBackendConfig, VclError};

varnish::run_vtc_tests!("tests/*.vtc");

/// A native backend object for VCL
#[allow(non_camel_case_types)]
struct backend {
    inner: NativeBackend,
}

/// Native backend VMOD
#[varnish::vmod(docs = "README.md")]
mod native_be {
    use varnish::ffi::VCL_BACKEND;
    use varnish::vcl::{Ctx, VclError};

    use super::{backend, create_backend_from_host_port};

    impl backend {
        /// Create a new native backend
        ///
        /// # Arguments
        /// * `name` - Automatically supplied VCL name
        /// * `host` - Backend host (IP address)
        /// * `port` - Backend port
        pub fn new(
            ctx: &mut Ctx,
            #[vcl_name] name: &str,
            host: &str,
            port: i64,
        ) -> Result<Self, VclError> {
            let inner = create_backend_from_host_port(ctx, name, host, port)?;
            Ok(backend { inner })
        }

        /// Get the backend pointer for use in VCL
        pub unsafe fn backend(&self) -> VCL_BACKEND {
            self.inner.vcl_ptr()
        }

        /// Check if the backend is healthy
        pub fn healthy(&self, ctx: &Ctx) -> bool {
            self.inner.is_healthy(ctx)
        }
    }
}

/// Helper function to create a native backend from host and port
fn create_backend_from_host_port(
    ctx: &mut Ctx,
    name: &str,
    host: &str,
    port: i64,
) -> Result<NativeBackend, VclError> {
    // Parse host as IP address
    let ip: std::net::IpAddr = host
        .parse()
        .map_err(|e| format!("Invalid IP address '{}': {}", host, e))?;

    // Validate port
    let port: u16 = port
        .try_into()
        .map_err(|_| format!("Invalid port: {}", port))?;

    // Create socket address
    let addr = SocketAddr::new(ip, port);

    // Create endpoint
    let endpoint = Endpoint::ip(addr);

    // Create backend config
    let config = NativeBackendConfig::new(name, endpoint);

    // Create and return the backend
    NativeBackend::new(ctx, &config, None)
}
