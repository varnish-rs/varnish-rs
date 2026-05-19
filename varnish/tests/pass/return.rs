use varnish::vmod;

fn main() {}

#[vmod]
mod returns {
    use varnish::vcl::Subroutine;

    pub fn return_subroutine() -> Subroutine {
        unimplemented!()
    }
}
