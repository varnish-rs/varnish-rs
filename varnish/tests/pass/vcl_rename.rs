#![expect(unused_variables)]
#![expect(non_camel_case_types)]

use varnish::vmod;

fn main() {}

pub struct kv;

#[vmod]
mod vcl_rename {
    use super::*;

    pub fn foo() -> i64 {
        0
    }

    #[vcl_rename(bar)]
    pub fn renamed_fn() -> i64 {
        0
    }

    impl kv {
        #[vcl_rename(make)]
        pub fn new(cap: Option<i64>) -> Self {
            Self
        }

        pub fn get(&self, key: &str) -> String {
            String::default()
        }

        #[vcl_rename(store)]
        pub fn set(&self, key: &str, value: &str) {}
    }
}
