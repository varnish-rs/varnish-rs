//! Native backend support for VMODs
//!
//! This module provides [`NativeBackend`] and [`NativeBackendConfig`] for creating
//! backends where Varnish handles all HTTP networking, connection pooling, and health
//! probes. Unlike synthetic backends ([`super::Backend`]), native backends don't require
//! implementing HTTP handling in Rust - Varnish's built-in HTTP client does all the work.
//!
//! # Example
//!
//! ```ignore
//! use std::net::SocketAddr;
//! use varnish::vcl::{Ctx, NativeBackend, NativeBackendConfig, Endpoint};
//!
//! fn create_backend(ctx: &mut Ctx) -> Result<NativeBackend, VclError> {
//!     let config = NativeBackendConfig::new(
//!         "my_backend",
//!         Endpoint::ip("127.0.0.1:8080".parse().unwrap()),
//!     );
//!     NativeBackend::new(ctx, &config, None)
//! }
//! ```

use std::ptr;
use std::time::Duration;

use crate::ffi::{vrt_backend, vtim_dur, VCL_BACKEND, VRT_BACKEND_MAGIC};
use crate::vcl::{into_vcl_probe, Ctx, Endpoint, IntoVCL, Probe, VclResult};

/// Helper to convert `Option<Duration>` to `vtim_dur` (-1.0 = use default)
fn timeout_to_vtim_dur(t: Option<Duration>) -> vtim_dur {
    vtim_dur(t.map_or(-1.0, |d| d.as_secs_f64()))
}

/// Configuration for a native backend
///
/// This struct mirrors the C `vrt_backend` structure, providing a safe Rust interface
/// for configuring all backend parameters.
#[derive(Debug, Clone, Default)]
pub struct NativeBackendConfig {
    /// Backend name (used in logs and CLI)
    pub name: String,
    /// Network endpoint (IP address or Unix socket)
    pub endpoint: Endpoint,
    /// Host header value (defaults to endpoint address if not set)
    pub host_header: Option<String>,
    /// Authority header for HTTP/2
    pub authority: Option<String>,
    /// Connection timeout (None = use Varnish default)
    pub connect_timeout: Option<Duration>,
    /// Timeout for first byte of response (None = use Varnish default)
    pub first_byte_timeout: Option<Duration>,
    /// Timeout between response bytes (None = use Varnish default)
    pub between_bytes_timeout: Option<Duration>,
    /// Timeout waiting for backend connection slot (None = use Varnish default)
    pub backend_wait_timeout: Option<Duration>,
    /// Maximum concurrent connections (0 = unlimited)
    pub max_connections: u32,
    /// PROXY protocol header version (0 = none, 1 or 2 for PROXY v1/v2)
    pub proxy_header: u8,
    /// Maximum number of requests waiting for connection slot (0 = unlimited)
    pub backend_wait_limit: u32,
    /// Health probe configuration
    pub probe: Option<Probe>,
}

impl NativeBackendConfig {
    /// Create a new backend configuration with required fields
    pub fn new(name: impl Into<String>, endpoint: Endpoint) -> Self {
        Self {
            name: name.into(),
            endpoint,
            ..Default::default()
        }
    }

    /// Set the Host header value
    #[must_use]
    pub fn with_host_header(mut self, host: impl Into<String>) -> Self {
        self.host_header = Some(host.into());
        self
    }

    /// Set the Authority header for HTTP/2
    #[must_use]
    pub fn with_authority(mut self, authority: impl Into<String>) -> Self {
        self.authority = Some(authority.into());
        self
    }

    /// Set the connection timeout
    #[must_use]
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Set the first byte timeout
    #[must_use]
    pub fn with_first_byte_timeout(mut self, timeout: Duration) -> Self {
        self.first_byte_timeout = Some(timeout);
        self
    }

    /// Set the between bytes timeout
    #[must_use]
    pub fn with_between_bytes_timeout(mut self, timeout: Duration) -> Self {
        self.between_bytes_timeout = Some(timeout);
        self
    }

    /// Set the backend wait timeout
    #[must_use]
    pub fn with_backend_wait_timeout(mut self, timeout: Duration) -> Self {
        self.backend_wait_timeout = Some(timeout);
        self
    }

    /// Set maximum concurrent connections
    #[must_use]
    pub fn with_max_connections(mut self, max: u32) -> Self {
        self.max_connections = max;
        self
    }

    /// Set PROXY protocol version (0, 1, or 2)
    #[must_use]
    pub fn with_proxy_header(mut self, version: u8) -> Self {
        self.proxy_header = version;
        self
    }

    /// Set maximum requests waiting for connection slot
    #[must_use]
    pub fn with_backend_wait_limit(mut self, limit: u32) -> Self {
        self.backend_wait_limit = limit;
        self
    }

    /// Set health probe configuration
    #[must_use]
    pub fn with_probe(mut self, probe: Probe) -> Self {
        self.probe = Some(probe);
        self
    }

    /// Convert to FFI `vrt_backend` structure
    pub(crate) fn to_ffi(&self, ctx: &mut Ctx) -> VclResult<vrt_backend> {
        let ws = &mut ctx.ws;

        // Convert endpoint
        let endpoint = self.endpoint.to_ffi(ws)?;

        // Convert name
        let vcl_name = self.name.as_str().into_vcl(ws)?.0;

        // Convert optional strings
        let hosthdr = match &self.host_header {
            Some(s) => s.as_str().into_vcl(ws)?.0,
            None => ptr::null(),
        };

        let authority = match &self.authority {
            Some(s) => s.as_str().into_vcl(ws)?.0,
            None => ptr::null(),
        };

        // Convert probe
        let probe = match &self.probe {
            Some(p) => into_vcl_probe(p.clone(), ws)?,
            None => crate::ffi::VCL_PROBE(ptr::null()),
        };

        Ok(vrt_backend {
            magic: VRT_BACKEND_MAGIC,
            endpoint,
            vcl_name,
            hosthdr,
            authority,
            connect_timeout: timeout_to_vtim_dur(self.connect_timeout),
            first_byte_timeout: timeout_to_vtim_dur(self.first_byte_timeout),
            between_bytes_timeout: timeout_to_vtim_dur(self.between_bytes_timeout),
            backend_wait_timeout: timeout_to_vtim_dur(self.backend_wait_timeout),
            max_connections: self.max_connections,
            proxy_header: u32::from(self.proxy_header),
            backend_wait_limit: self.backend_wait_limit,
            probe,
        })
    }
}

/// A native backend managed by Varnish
///
/// Unlike synthetic backends ([`super::Backend`]), native backends delegate all HTTP
/// handling to Varnish, including connection pooling and health probes. This is the
/// preferred approach when your backend is a standard HTTP server.
///
/// # Lifetime and Cleanup
///
/// Native backends are automatically cleaned up when the VCL is discarded. For most
/// use cases, store the `NativeBackend` in a VMOD object:
///
/// ```ignore
/// struct MyVmod {
///     backend: NativeBackend,
/// }
/// ```
///
/// For dynamic backends that need explicit cleanup, use the [`NativeBackend::delete`]
/// method which requires a context.
#[derive(Debug)]
pub struct NativeBackend {
    ptr: VCL_BACKEND,
}

impl NativeBackend {
    /// Create a new native backend
    ///
    /// # Arguments
    /// * `ctx` - VCL context (typically called in `vcl_init`)
    /// * `config` - Backend configuration
    /// * `via` - Optional "via" backend for request routing (advanced use)
    ///
    /// # Errors
    /// Returns an error if the configuration is invalid or Varnish rejects the backend.
    pub fn new(
        ctx: &mut Ctx,
        config: &NativeBackendConfig,
        via: Option<VCL_BACKEND>,
    ) -> VclResult<Self> {
        // Validate name is not empty
        if config.name.is_empty() {
            return Err("Backend name cannot be empty".into());
        }

        // Convert config to FFI
        let vrt_be = config.to_ffi(ctx)?;

        // Create backend
        let ptr = unsafe {
            crate::ffi::VRT_new_backend(
                ctx.raw,
                &raw const vrt_be,
                via.unwrap_or(VCL_BACKEND(ptr::null_mut())),
            )
        };

        if ptr.0.is_null() {
            return Err(format!("VRT_new_backend returned null for '{}'", config.name).into());
        }

        Ok(Self { ptr })
    }

    /// Get the `VCL_BACKEND` pointer for use in VCL
    ///
    /// This is typically used to return the backend from a VMOD method:
    ///
    /// ```ignore
    /// impl MyVmod {
    ///     fn backend(&self) -> VCL_BACKEND {
    ///         self.backend.vcl_ptr()
    ///     }
    /// }
    /// ```
    pub fn vcl_ptr(&self) -> VCL_BACKEND {
        self.ptr
    }

    /// Check if this backend is currently healthy
    ///
    /// This queries Varnish's health status for this backend, which is determined
    /// by the probe configuration (if any).
    pub fn is_healthy(&self, ctx: &Ctx) -> bool {
        unsafe { crate::ffi::VRT_Healthy(ctx.raw, self.ptr, ptr::null_mut()).0 != 0 }
    }

    /// Explicitly delete this backend
    ///
    /// This method is only needed for dynamic backends. For static backends
    /// (created in `vcl_init` and stored in a VMOD object), cleanup is automatic
    /// when the VCL is discarded.
    ///
    /// # Safety
    /// After calling this method, the `NativeBackend` is consumed and the
    /// underlying pointer is no longer valid.
    pub fn delete(mut self, ctx: &mut Ctx) {
        unsafe {
            crate::ffi::VRT_delete_backend(ctx.raw, &raw mut self.ptr);
        }
        // self is consumed here, preventing use-after-free
    }
}

// Note: We intentionally do not implement Drop for NativeBackend.
// VRT_delete_backend requires a context, which we don't have in Drop.
// Backends are automatically cleaned up when the VCL is discarded.
// For dynamic backends, users must call delete() explicitly.
