varnish::run_vtc_tests!("tests/*.vtc");

/// Demonstrates `#[restrict(...)]`, which limits which VCL subroutines can call a function.
///
/// A violation is caught at VCL compile time — the `vcl.load` command will fail with
/// "Not available in subroutine". This is useful for functions that only make sense
/// in a specific context (e.g., accessing `bereq` headers is only valid in backend subs).
#[varnish::vmod(docs = "README.md")]
mod restricted_callsites {
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
}
