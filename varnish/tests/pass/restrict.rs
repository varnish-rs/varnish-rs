use varnish::vmod;

fn main() {}

#[vmod]
mod restrict_scopes {
    #[restrict(client)]
    pub fn client_only() -> i64 {
        1
    }

    #[restrict(backend)]
    pub fn backend_only() -> i64 {
        2
    }

    #[restrict(housekeeping)]
    pub fn housekeeping_only() -> i64 {
        3
    }

    #[restrict(client, backend)]
    pub fn client_or_backend() -> i64 {
        4
    }

    #[restrict(vcl_recv)]
    pub fn recv_only() -> i64 {
        5
    }

    #[restrict(vcl_recv, vcl_hash)]
    pub fn recv_or_hash() -> i64 {
        6
    }

    #[restrict(vcl_backend_fetch, vcl_backend_response, vcl_backend_error)]
    pub fn backend_subs() -> i64 {
        7
    }
}
