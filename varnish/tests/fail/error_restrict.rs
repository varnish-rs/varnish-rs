#[varnish::vmod]
mod restrict_unknown_scope {
    #[restrict(vcl_recv, not_a_scope)]
    pub fn bad_scope() -> i64 {
        1
    }
}

#[varnish::vmod]
mod restrict_empty {
    #[restrict()]
    pub fn no_scopes() -> i64 {
        2
    }
}

fn main() {}
