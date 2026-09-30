use varnish::vmod;

fn main() {}

pub struct Counter {
    value: i64,
}

#[vmod]
mod object_mut {
    use super::Counter;
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
    }
}
