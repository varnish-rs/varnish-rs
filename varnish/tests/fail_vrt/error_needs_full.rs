// Built without the `full` feature: arguments and returns that need `full`-only APIs must be
// rejected by `#[vmod]` with a clear message, not with errors inside the generated code.

#[varnish::vmod]
mod arg_backend {
    use varnish::vcl::BackendRef;
    pub fn f(_v: Option<BackendRef>) {}
}

#[varnish::vmod]
mod arg_probe {
    use varnish::vcl::Probe;
    pub fn f(_v: Option<Probe>) {}
}

#[varnish::vmod]
mod arg_cow_probe {
    use varnish::vcl::CowProbe;
    pub fn f(_v: Option<CowProbe<'_>>) {}
}

#[varnish::vmod]
mod arg_ip {
    use std::net::SocketAddr;
    pub fn f(_v: Option<SocketAddr>) {}
}

#[varnish::vmod]
mod ret_probe {
    use varnish::vcl::Probe;
    pub fn f() -> Probe {
        unimplemented!()
    }
}

#[varnish::vmod]
mod ret_ip {
    use std::net::SocketAddr;
    pub fn f() -> SocketAddr {
        unimplemented!()
    }
}

#[varnish::vmod]
mod fetch_filters {
    use varnish::vcl::{Ctx, Event, FetchFilters};
    #[event]
    pub fn on_event(_ctx: &mut Ctx, #[event] _e: Event, _vfp: &mut FetchFilters) {}
}

#[varnish::vmod]
mod delivery_filters {
    use varnish::vcl::{Ctx, DeliveryFilters, Event};
    #[event]
    pub fn on_event(_ctx: &mut Ctx, #[event] _e: Event, _vdp: &mut DeliveryFilters) {}
}

fn main() {}
