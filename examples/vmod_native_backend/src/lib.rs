varnish::run_vtc_tests!("tests/*.vtc");

#[varnish::vmod(docs = "README.md")]
mod native_backend {
    use std::net::SocketAddr;
    use varnish::vcl::Ctx;
    use varnish::vcl::NativeBackendBuilder;
    use varnish::vcl::{BackendRef, NativeBackend};

    /// Create a dynamic backend from a socket address.
    ///
    /// This function demonstrates creating native backends at runtime.
    /// The backend is stored per-task and reused within the same request.
    ///
    /// Example:
    /// ```vcl
    /// set req.backend_hint = native_backend.create("${server_addr}:${server_port}");
    /// ```
    pub fn create(
        ctx: &mut Ctx,
        /// Socket address string (e.g., "127.0.0.1:8080")
        addr: Option<&str>,
        /// Storage for the created native backend (per-task)
        #[shared_per_task]
        backend_storage: &mut Option<Box<NativeBackend>>,
    ) -> Option<BackendRef> {
        // Parse the socket address string
        let addr_str = addr?;
        let sock_addr: SocketAddr = addr_str.parse().ok()?;

        // Create backend name from address
        let name = format!("native_{sock_addr}");
        let name_cstr = std::ffi::CString::new(name).ok()?;

        // Build the backend
        let native_backend = NativeBackendBuilder::new_ip(&name_cstr, sock_addr)
            .build(ctx)
            .ok()?;

        let backend_ref = native_backend.as_ref().clone();
        *backend_storage = Some(Box::new(native_backend));
        Some(backend_ref)
    }
}
