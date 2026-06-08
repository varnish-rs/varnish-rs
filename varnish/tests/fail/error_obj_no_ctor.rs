struct ObjNoConstructor;

#[varnish::vmod]
mod err {
    use super::ObjNoConstructor;
    impl ObjNoConstructor {
        pub fn func() -> i64 {
            0
        }
    }
}

fn main() {}
