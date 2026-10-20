pub struct Obj;

#[varnish::vmod]
mod obj_mut_reentry_split {
    use super::Obj;
    use varnish::vcl::subroutine::Subroutine;
    use varnish::vcl::Ctx;

    impl Obj {
        pub fn new() -> Self {
            Self
        }

        #[restrict(vcl_init)]
        pub fn add(&mut self, _value: i64) {}
    }

    // A second `impl` block would hide `run` from the `&mut self` re-entry check.
    impl Obj {
        pub fn other() -> Self {
            Self
        }

        pub fn run(&self, _ctx: &mut Ctx, _sub: Subroutine) {}
    }
}

fn main() {}
