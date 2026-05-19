use varnish::vmod;

fn main() {}

#[vmod]
mod args {
    use varnish::vcl::Subroutine;

    pub fn arg_subroutine(_sub: Subroutine) {}
}
