varnish::run_vtc_tests!("tests/*.vtc");

/// A counter that can only be modified during `vcl_init`/`vcl_fini`, and read anywhere.
pub struct InitCounter {
    value: i64,
}

/// Demonstrates `#[restrict(...)]`, which limits which VCL subroutines can call a function.
///
/// A violation is caught at VCL compile time — the `vcl.load` command will fail with
/// "Not available in subroutine". This is useful for functions that only make sense
/// in a specific context (e.g., accessing `bereq` headers is only valid in backend subs).
#[varnish::vmod(docs = "README.md")]
mod restricted_callsites {
    use super::InitCounter;

    /// Only callable from client-side VCL subs (`vcl_recv`, `vcl_pass`, `vcl_hash`, etc.)
    #[restrict(client)]
    pub fn client_only() -> i64 {
        1
    }

    /// Only callable from backend-side VCL subs (`vcl_backend_fetch`, `vcl_backend_response`, etc.)
    #[restrict(backend)]
    pub fn backend_only() -> i64 {
        2
    }

    /// Only callable from `vcl_recv` and `vcl_hash`
    #[restrict(vcl_recv, vcl_hash)]
    pub fn recv_or_hash() -> i64 {
        3
    }

    /// Callable from both client and backend contexts
    #[restrict(client, backend)]
    pub fn client_or_backend() -> i64 {
        4
    }

    impl InitCounter {
        pub fn counter() -> Self {
            Self { value: 0 }
        }

        /// Add to the counter. Methods restricted to `vcl_init`/`vcl_fini` (or `housekeeping`)
        /// may take `&mut self`: they run on the CLI thread while no request can reach the object.
        #[restrict(vcl_init)]
        pub fn add(&mut self, value: i64) {
            self.value += value;
        }

        /// Read the counter from any subroutine
        pub fn get(&self) -> i64 {
            self.value
        }
    }
}
