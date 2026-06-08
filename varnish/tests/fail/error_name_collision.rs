pub struct Foo;
pub struct Bar;

#[varnish::vmod]
mod constructor_constructor_collision {
    use super::*;

    impl Foo {
        pub fn new() -> Self {
            Self
        }
    }

    impl Bar {
        pub fn new() -> Self {
            Self
        }
    }
}

#[varnish::vmod]
mod constructor_function_collision {
    use super::*;

    pub fn new() -> i64 {
        0
    }

    impl Foo {
        pub fn new() -> Self {
            Self
        }
    }
}

fn main() {}
