#![expect(unused_variables)]

use varnish::vmod;

fn main() {}

pub struct PerVcl;
pub struct Obj1;
pub struct Obj2;
pub struct Obj3;
pub struct Obj4;

#[vmod]
mod obj2 {
    use super::*;
    use varnish::vcl::Ctx;

    impl Obj1 {
        pub fn obj1(#[shared_per_vcl] vcl: &mut Option<Box<PerVcl>>, val: Option<i64>) -> Self {
            Self
        }
    }

    impl Obj2 {
        pub fn obj2(#[shared_per_vcl] vcl: &mut Option<Box<PerVcl>>, val: i64) -> Self {
            Self
        }
    }

    impl Obj3 {
        pub fn obj3(
            ctx: &mut Ctx,
            #[shared_per_vcl] vcl: &mut Option<Box<PerVcl>>,
            val: Option<i64>,
        ) -> Self {
            Self
        }
    }

    impl Obj4 {
        pub fn obj4(
            ctx: &mut Ctx,
            #[shared_per_vcl] vcl: &mut Option<Box<PerVcl>>,
            val: i64,
        ) -> Self {
            Self
        }
    }
}
