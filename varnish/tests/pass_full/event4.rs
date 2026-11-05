#![expect(unused_variables)]

use varnish::vmod;

fn main() {}

#[vmod]
mod event4 {
    use varnish::vcl::DeliveryFilters;

    #[event]
    pub fn on_event(vdp: &mut DeliveryFilters) {}
}
