pub struct Obj;

#[varnish::vmod]
mod obj_mut_reentry {
    use super::Obj;
    use varnish::vcl::subroutine::Subroutine;
    use varnish::vcl::Ctx;

    impl Obj {
        pub fn new() -> Self {
            Self
        }

        #[restrict(vcl_init)]
        pub fn add(&mut self, _value: i64) {}

        pub fn unrestricted(&self, _ctx: &mut Ctx, _sub: Subroutine) {}

        #[restrict(vcl_init)]
        pub fn init_only(&self, _ctx: &mut Ctx, _sub: Subroutine) {}

        #[restrict(client, vcl_fini)]
        pub fn client_and_fini(&self, _ctx: &mut Ctx, _sub: Subroutine) {}

        #[restrict(housekeeping)]
        pub fn housekeeping(&self, _ctx: &mut Ctx, _sub: Subroutine) {}

        #[restrict(vcl_recv, vcl_init)]
        pub fn recv_and_init(&self, _ctx: &mut Ctx, _sub: Subroutine) {}
    }
}

fn main() {}
