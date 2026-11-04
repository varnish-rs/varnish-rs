#![expect(unused_variables)]

use varnish::vmod;

fn main() {}

pub struct PerVcl;
pub struct Obj1;
pub struct Obj2;

#[vmod]
mod event3 {
    use varnish::vcl::{Ctx, DeliveryFilters, Event, FetchFilters};

    use super::{Obj1, Obj2, PerVcl};

    #[event]
    pub fn on_event(
        ctx: &Ctx,
        event: Event,
        #[shared_per_vcl] vcl: &mut Option<Box<PerVcl>>,
        vdp: &mut DeliveryFilters,
        vfp: &mut FetchFilters,
    ) -> Result<(), &'static str> {
        Ok(())
    }

    pub fn access(#[shared_per_vcl] vcl: Option<&PerVcl>) {}

    impl Obj1 {
        pub fn obj1(#[shared_per_vcl] vcl: &mut Option<Box<PerVcl>>) -> Self {
            Self
        }
        pub fn obj_access(&self, #[shared_per_vcl] vcl: Option<&PerVcl>) {}
    }

    impl Obj2 {
        pub fn obj2(vdp: &mut DeliveryFilters) -> Self {
            Self
        }
        pub fn obj_access(&self) {}
    }
}
