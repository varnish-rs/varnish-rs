use varnish::vmod;

fn main() {}

#[vmod]
mod returns {
    use varnish::vcl::subroutine::Subroutine;

    pub fn return_subroutine() -> Subroutine {
        unimplemented!()
    }
}
