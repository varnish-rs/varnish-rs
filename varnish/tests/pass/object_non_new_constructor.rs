#![expect(unused_variables)]
#![expect(non_camel_case_types)]

use varnish::vmod;

fn main() {}

pub struct kv;

#[vmod]
mod non_new_constructor {
    use super::*;

    impl kv {
        pub fn create(cap: Option<i64>) -> Self {
            Self
        }

        pub fn get(&self, key: &str) -> String {
            String::default()
        }
    }
}
