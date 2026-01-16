//! Example VMOD demonstrating director support using BackendSet
//!
//! This VMOD provides round-robin and fallback directors implemented
//! in pure Rust using the `BackendSet` helper for thread-safe backend
//! management with proper reference counting.
//!
//! Backends are passed directly from VCL as `VCL_BACKEND` references,
//! allowing idiomatic VCL usage like:
//!
//! ```vcl
//! backend be1 { .host = "10.0.0.1"; .port = "80"; }
//! backend be2 { .host = "10.0.0.2"; .port = "80"; }
//!
//! sub vcl_init {
//!     new rr = directors_rs.round_robin();
//!     rr.add_backend(be1);
//!     rr.add_backend(be2);
//! }
//! ```

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use varnish::ffi::VCL_BACKEND;
use varnish::vcl::{BackendSet, Buffer, Ctx, Director, VclDirector};

varnish::run_vtc_tests!("tests/*.vtc");

// ============================================================================
// Round-Robin Director
// ============================================================================

/// Round-robin director implementation
struct RoundRobinImpl {
    backends: BackendSet,
    next: AtomicUsize,
}

impl RoundRobinImpl {
    fn new() -> Self {
        Self {
            backends: BackendSet::new(),
            next: AtomicUsize::new(0),
        }
    }
}

impl VclDirector for RoundRobinImpl {
    fn resolve(&self, ctx: &mut Ctx) -> Option<VCL_BACKEND> {
        let idx = self.next.fetch_add(1, Ordering::Relaxed);
        self.backends.pick_by_index(ctx, idx)
    }

    fn healthy(&self, ctx: &mut Ctx) -> (bool, SystemTime) {
        self.backends.any_healthy(ctx)
    }

    fn release(&self) {
        self.backends.release_all();
    }

    fn list(&self, ctx: &mut Ctx, vsb: &mut Buffer, detailed: bool, _json: bool) {
        let total = self.backends.len();
        let healthy = self.backends.healthy_count(ctx);
        let msg = if detailed {
            format!(
                "{}/{} healthy, next index: {}",
                healthy,
                total,
                self.next.load(Ordering::Relaxed)
            )
        } else {
            format!("{}/{} healthy", healthy, total)
        };
        let _ = vsb.write(&msg);
    }
}

/// VCL object wrapper for round-robin director
#[allow(non_camel_case_types)]
pub struct round_robin {
    director: Director<RoundRobinImpl>,
}

// ============================================================================
// Fallback Director
// ============================================================================

/// Fallback director implementation - returns first healthy backend
struct FallbackImpl {
    backends: BackendSet,
}

impl FallbackImpl {
    fn new() -> Self {
        Self {
            backends: BackendSet::new(),
        }
    }
}

impl VclDirector for FallbackImpl {
    fn resolve(&self, ctx: &mut Ctx) -> Option<VCL_BACKEND> {
        self.backends.pick_first_healthy(ctx)
    }

    fn healthy(&self, ctx: &mut Ctx) -> (bool, SystemTime) {
        self.backends.any_healthy(ctx)
    }

    fn release(&self) {
        self.backends.release_all();
    }

    fn list(&self, ctx: &mut Ctx, vsb: &mut Buffer, _detailed: bool, _json: bool) {
        let total = self.backends.len();
        let healthy = self.backends.healthy_count(ctx);
        let msg = format!("{}/{} healthy", healthy, total);
        let _ = vsb.write(&msg);
    }
}

/// VCL object wrapper for fallback director
#[allow(non_camel_case_types)]
pub struct fallback {
    director: Director<FallbackImpl>,
}

// ============================================================================
// VMOD Definition
// ============================================================================

/// Directors VMOD - provides round-robin and fallback directors
#[varnish::vmod(docs = "README.md")]
mod directors_rs {
    use varnish::ffi::VCL_BACKEND;
    use varnish::vcl::{Ctx, Director, VclError};

    use super::{fallback, round_robin, FallbackImpl, RoundRobinImpl};

    // ------------------------------------------------------------------------
    // Round-Robin Director
    // ------------------------------------------------------------------------

    impl round_robin {
        /// Create a new round-robin director
        ///
        /// The round-robin director distributes requests evenly across all
        /// healthy backends in a circular fashion.
        pub fn new(ctx: &mut Ctx, #[vcl_name] name: &str) -> Result<Self, VclError> {
            let director = Director::new(ctx, "round-robin", name, RoundRobinImpl::new())?;
            Ok(round_robin { director })
        }

        /// Add a backend to the director
        ///
        /// Backends are selected round-robin among those that are healthy.
        pub fn add_backend(&self, be: Option<VCL_BACKEND>) {
            if let Some(be) = be {
                self.director.get_inner().backends.add(be);
            }
        }

        /// Remove a backend from the director
        ///
        /// Returns true if the backend was found and removed, false otherwise.
        pub fn remove_backend(&self, be: Option<VCL_BACKEND>) -> bool {
            if let Some(be) = be {
                self.director.get_inner().backends.remove(be)
            } else {
                false
            }
        }

        /// Get the director backend pointer for use in VCL
        pub unsafe fn backend(&self) -> VCL_BACKEND {
            self.director.vcl_ptr()
        }

        /// Get the number of backends in the director
        pub fn count(&self) -> i64 {
            self.director.get_inner().backends.len() as i64
        }
    }

    // ------------------------------------------------------------------------
    // Fallback Director
    // ------------------------------------------------------------------------

    impl fallback {
        /// Create a new fallback director
        ///
        /// The fallback director returns the first healthy backend in
        /// priority order. Add backends in order of preference.
        pub fn new(ctx: &mut Ctx, #[vcl_name] name: &str) -> Result<Self, VclError> {
            let director = Director::new(ctx, "fallback", name, FallbackImpl::new())?;
            Ok(fallback { director })
        }

        /// Add a backend to the director
        ///
        /// Backends are checked in order - the first healthy one is used.
        /// Add your primary backend first, then fallbacks in order.
        pub fn add_backend(&self, be: Option<VCL_BACKEND>) {
            if let Some(be) = be {
                self.director.get_inner().backends.add(be);
            }
        }

        /// Remove a backend from the director
        ///
        /// Returns true if the backend was found and removed, false otherwise.
        pub fn remove_backend(&self, be: Option<VCL_BACKEND>) -> bool {
            if let Some(be) = be {
                self.director.get_inner().backends.remove(be)
            } else {
                false
            }
        }

        /// Get the director backend pointer for use in VCL
        pub unsafe fn backend(&self) -> VCL_BACKEND {
            self.director.vcl_ptr()
        }

        /// Get the number of backends in the director
        pub fn count(&self) -> i64 {
            self.director.get_inner().backends.len() as i64
        }
    }
}
