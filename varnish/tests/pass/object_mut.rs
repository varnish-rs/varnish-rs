use varnish::vmod;

fn main() {}

pub struct Plain;

pub struct Counter {
    value: i64,
}

#[vmod]
mod object_mut {
    use super::{Counter, Plain};
    use varnish::vcl::subroutine::Subroutine;
    use varnish::vcl::{Ctx, Workspace};

    impl Counter {
        pub fn new() -> Self {
            Self { value: 0 }
        }

        /// Only callable from vcl_init and vcl_fini, so `&mut self` is allowed.
        #[restrict(housekeeping)]
        pub fn set(&mut self, value: i64) {
            self.value = value;
        }

        #[restrict(vcl_init)]
        pub fn add(&mut self, _ctx: &Ctx, value: i64) -> Result<(), String> {
            self.value += value;
            Ok(())
        }

        #[restrict(vcl_init)]
        pub fn add_ws(&mut self, _ws: &mut Workspace, value: i64) {
            self.value += value;
        }

        #[restrict(vcl_init, vcl_fini)]
        pub fn reset(&mut self) {
            self.value = 0;
        }

        pub fn get(&self) -> i64 {
            self.value
        }

        /// `&mut Ctx` is fine on a `&self` method that can't run in vcl_init/vcl_fini.
        #[restrict(client, backend)]
        pub fn run(&self, ctx: &mut Ctx, sub: Subroutine) -> i64 {
            _ = ctx.call_sub(sub);
            self.value
        }

        #[restrict(client)]
        pub fn run_client(&self, ctx: &mut Ctx, sub: Subroutine) -> i64 {
            _ = ctx.call_sub(sub);
            self.value
        }

        #[restrict(backend)]
        pub fn run_backend(&self, ctx: &mut Ctx, sub: Subroutine) -> i64 {
            _ = ctx.call_sub(sub);
            self.value
        }

        /// `&Ctx` cannot call a VCL sub, so it is allowed in any scope.
        pub fn get_ctx(&self, _ctx: &Ctx) -> i64 {
            self.value
        }
    }

    /// Without any `&mut self` method, `&mut Ctx` is allowed in any scope.
    impl Plain {
        pub fn plain() -> Self {
            Self
        }

        pub fn run(&self, ctx: &mut Ctx, sub: Subroutine) {
            _ = ctx.call_sub(sub);
        }
    }
}
