pub struct Obj;

#[varnish::vmod]
mod vcl_rename_on_impl {
    use super::Obj;

    #[vcl_rename(foo)]
    impl Obj {
        pub fn new() -> Self {
            Self
        }
    }
}

fn main() {}
