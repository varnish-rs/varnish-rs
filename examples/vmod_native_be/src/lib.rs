//! Example VMOD demonstrating native backend support
//!
//! This VMOD creates backends where Varnish handles all HTTP networking,
//! connection pooling, and health probes. Unlike synthetic backends,
//! no custom HTTP handling is required.

use std::net::SocketAddr;
use std::time::Duration;

use varnish::vcl::{Ctx, Endpoint, NativeBackend, NativeBackendConfig, Probe, Request, VclError};

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

    use super::{backend, create_backend};

    impl backend {
        /// Create a new native backend
        ///
        /// # Arguments
        /// * `name` - Automatically supplied VCL name
        /// * `host` - Backend host (IP address)
        /// * `port` - Backend port
        /// * `probe_url` - Optional URL path for health probe (e.g., "/health")
        /// * `probe_interval` - Seconds between probe requests (default: 5.0)
        /// * `probe_timeout` - Seconds before probe times out (default: 2.0)
        /// * `probe_window` - Number of probes in sliding window (default: 8)
        /// * `probe_threshold` - Minimum healthy probes in window (default: 3)
        /// * `probe_initial` - Initial healthy probe count (default: same as threshold)
        #[allow(clippy::too_many_arguments)]
        pub fn new(
            ctx: &mut Ctx,
            #[vcl_name] name: &str,
            host: &str,
            port: i64,
            probe_url: Option<&str>,
            probe_interval: Option<f64>,
            probe_timeout: Option<f64>,
            probe_window: Option<i64>,
            probe_threshold: Option<i64>,
            probe_initial: Option<i64>,
        ) -> Result<Self, VclError> {
            let inner = create_backend(
                ctx,
                name,
                host,
                port,
                probe_url.unwrap_or(""),
                probe_interval.unwrap_or(5.0),
                probe_timeout.unwrap_or(2.0),
                probe_window.unwrap_or(8),
                probe_threshold.unwrap_or(3),
                probe_initial.unwrap_or(-1),
            )?;
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

/// Helper function to create a native backend, optionally with health probe
#[allow(clippy::too_many_arguments)]
fn create_backend(
    ctx: &mut Ctx,
    name: &str,
    host: &str,
    port: i64,
    probe_url: &str,
    probe_interval: f64,
    probe_timeout: f64,
    probe_window: i64,
    probe_threshold: i64,
    probe_initial: i64,
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
    let mut config = NativeBackendConfig::new(name, endpoint);

    // Add probe if URL is provided
    if !probe_url.is_empty() {
        // If probe_initial is -1, use threshold as default
        let initial = if probe_initial < 0 {
            probe_threshold as u32
        } else {
            probe_initial as u32
        };

        let probe = Probe {
            request: Request::Url(probe_url.to_string()),
            interval: Duration::from_secs_f64(probe_interval),
            timeout: Duration::from_secs_f64(probe_timeout),
            exp_status: 200,
            window: probe_window as u32,
            threshold: probe_threshold as u32,
            initial,
        };
        config = config.with_probe(probe);
    }

    // Create and return the backend
    NativeBackend::new(ctx, &config, None)
}
