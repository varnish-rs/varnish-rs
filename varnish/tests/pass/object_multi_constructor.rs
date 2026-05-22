#![expect(unused_variables)]
#![expect(non_camel_case_types)]

use varnish::vmod;

fn main() {}

pub struct kv;

#[vmod]
mod multi_constructor {
    use super::*;

    impl kv {
        /// Create a new key-value store with optional capacity.
        pub fn new(cap: Option<i64>) -> Self {
            Self
        }

        /// Create a new key-value store with a fixed capacity of 10.
        pub fn new_fixed() -> Self {
            Self
        }

        pub fn set(&self, key: &str, value: &str) {}

        pub fn get(&self, key: &str) -> String {
            String::default()
        }
    }
}
