use std::sync::Arc;

use varnish::vcl::{Backend, Ctx, VclBackend, VclError, VclResponse};

varnish::run_vtc_tests!("tests/*.vtc");

struct Repeater {
    backend: Backend<RepeaterBackend, RepeaterResponseBody>,
}

/// a simple STRING dictionary in your VCL
#[varnish::vmod(docs = "README.md")]
mod simple_backend {
    use std::sync::Arc;

    use varnish::ffi::VCL_BACKEND;
    use varnish::vcl::{Backend, Ctx, VclError};

    use super::{Repeater, RepeaterBackend};

    /// Repeater is our VCL object, which just holds a rust Backend,
    /// it only needs two functions:
    /// - `new()`, so that the VCL can instantiate it
    /// - `backend()`, so that we can produce a C pointer for varnish to use
    impl Repeater {
        pub fn new(
            ctx: &mut Ctx,
            // Varnish automatically supplies this parameter if listed here
            // It is not part of the object instantiation in VCL
            #[vcl_name] name: &str,
            to_repeat: &str,
        ) -> Result<Self, VclError> {
            // to create the backend, we need:
            // - the vcl context, that we just pass along
            // - the vcl_name (how the vcl writer named the object)
            // - a struct that implements the VclBackend trait
            let backend = Backend::new(
                ctx,
                "repeater",
                name,
                RepeaterBackend {
                    data: Arc::from(to_repeat.as_bytes()),
                },
                false,
            )?;

            Ok(Repeater { backend })
        }

        pub unsafe fn backend(&self) -> VCL_BACKEND {
            self.backend.as_ref().vcl_ptr()
        }
    }
}

/// [`RepeaterBackend`] holds the string to repeat, shared via `Arc`
struct RepeaterBackend {
    data: Arc<[u8]>,
}

/// A lot of the [`VclBackend`] trait's methods are optional, but we need to implement
/// - [`Self::get_response`] that actually builds the response headers, and returns a Body
impl VclBackend<RepeaterResponseBody> for RepeaterBackend {
    fn get_response(&self, ctx: &mut Ctx) -> Result<Option<RepeaterResponseBody>, VclError> {
        let beresp = ctx.http_beresp.as_mut().expect("http_beresp must be set");
        beresp.set_status(200);
        beresp.set_header("server", "repeater")?;

        Ok(Some(RepeaterResponseBody {
            inner: std::io::Cursor::new(Arc::clone(&self.data)),
        }))
    }
}

struct RepeaterResponseBody {
    inner: std::io::Cursor<Arc<[u8]>>,
}

impl VclResponse for RepeaterResponseBody {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VclError> {
        use std::io::Read;
        self.inner
            .read(buf)
            .map_err(|e| VclError::new(e.to_string()))
    }

    fn len(&self) -> Option<usize> {
        Some(self.inner.get_ref().len())
    }
}
