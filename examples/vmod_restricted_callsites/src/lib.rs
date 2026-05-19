varnish::run_vtc_tests!("tests/*.vtc");

#[varnish::vmod]
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
