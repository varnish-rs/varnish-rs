varnish::run_vtc_tests!("tests/*.vtc");

use varnish::vcl::BackendRef;

#[allow(non_camel_case_types)]
struct new {
    backend: BackendRef,
}

/// Parse files into numbers
///
/// This is a simple example of how to handle errors in a Varnish VMOD.
/// All three functions will do the same thing: read a file and try to parse its content into a `VCL_INT`.
/// However, they will handle failure (file not found, permission issue, unparsable content, etc.) differently.
#[varnish::vmod(docs = "README.md")]
mod backend_ref {
    use super::new;

    use varnish::vcl::{BackendRef, Ctx, VclError};

    impl new {
        #[allow(clippy::self_named_constructors)]
        pub fn new(backend_arg: Option<BackendRef>) -> Result<Self, VclError> {
            match backend_arg {
                None => Err("backend_ref.new() can't take a NULL backend".into()),
                Some(backend) => Ok(new { backend })
            }
        }

        pub fn name(&self) -> String {
            self.backend.name().to_string_lossy().into()
        }

        pub fn healthy(&self, ctx: &Ctx) -> bool {
            self.backend.healthy(ctx)
        }

        pub fn backend(&self) -> BackendRef {
            // clone to increment the refcount
            self.backend.clone()
        }
    }
}
