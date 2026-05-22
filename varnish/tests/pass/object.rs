#![expect(unused_variables)]
#![expect(non_camel_case_types)]

use varnish::vmod;

fn main() {}

pub struct kv1;
pub struct kv2;
pub struct kv3;

#[vmod]
mod obj {
    use super::*;
    use varnish::vcl::Ctx;

    impl kv1 {
        pub fn kv1(cap: Option<i64>) -> Self {
            Self
        }
        pub fn set(&self, key: &str, value: &str) {}
        pub fn get(&self, key: &str) -> String {
            String::default()
        }
    }

    impl kv2 {
        pub fn kv2(cap: Option<i64>, #[vcl_name] name: &str) -> Self {
            Self
        }
        pub fn set(&self, key: &str, value: Option<&str>) {}
    }

    impl kv3 {
        pub fn kv3(ctx: &mut Ctx, cap: Option<i64>, #[vcl_name] name: &str) -> Self {
            Self
        }
        pub fn set(&self, ctx: &mut Ctx, key: &str, value: Option<&str>) {}
    }
}
