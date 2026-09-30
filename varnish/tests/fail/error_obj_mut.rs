pub struct Obj;

#[varnish::vmod]
mod obj_mut {
    use super::Obj;
    use varnish::vcl::Ctx;

    impl Obj {
        pub fn new() -> Self {
            Self
        }

        pub fn no_restrict(&mut self) {}

        #[restrict(client)]
        pub fn client_scope(&mut self) {}

        #[restrict(vcl_init, vcl_recv)]
        pub fn mixed_scope(&mut self) {}

        #[restrict(vcl_init)]
        pub fn mut_ctx(&mut self, ctx: &mut Ctx) {}

        #[restrict(vcl_init)]
        pub fn by_value(mut self) {}
    }
}

fn main() {}
