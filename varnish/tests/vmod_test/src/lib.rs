#![expect(clippy::unnecessary_wraps)]

use std::ffi::CStr;

use varnish::vcl::{Ctx, FetchProcCtx, FetchProcessor, InitResult, PullResult};
use varnish::vmod;

varnish::run_vtc_tests!("tests/*.vtc");

/// Test vmod
#[vmod(docs = "README.md")]
mod rustest {
    use std::ffi::CStr;
    use std::io::Write;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::time::Duration;

    use varnish::ffi::VCL_STRING;
    use varnish::vcl::{CowProbe, Ctx, Event, FetchFilters, Probe, Request, VclError, Workspace};

    use super::VFPTest;

    pub fn set_hdr(ctx: &mut Ctx, name: &str, value: &str) -> Result<(), VclError> {
        if let Some(ref mut req) = ctx.http_req {
            Ok(req.set_header(name, value)?)
        } else {
            Err("http_req isn't accessible".into())
        }
    }

    pub fn unset_hdr(ctx: &mut Ctx, name: &str) -> Result<(), &'static str> {
        if let Some(ref mut req) = ctx.http_req {
            req.unset_header(name);
            Ok(())
        } else {
            Err("http_req isn't accessible")
        }
    }

    pub unsafe fn ws_reserve(ws: &mut Workspace, s: &str) -> Result<VCL_STRING, &'static str> {
        let mut buf = ws.vcl_string_builder().unwrap();
        match write!(buf, "{s} {s} {s}") {
            Ok(()) => {
                let buffer = buf.finish();
                let res = CStr::from_ptr(buffer.0);
                // Note that count_bytes does NOT include nul terminator
                assert_eq!(res.count_bytes(), 3 * s.len() + 2);
                Ok(buffer)
            }
            _ => Err("workspace issue"),
        }
    }

    pub fn out_str() -> &'static str {
        "str"
    }

    pub fn out_res_str() -> Result<&'static str, &'static str> {
        Ok("str")
    }

    pub fn out_string() -> String {
        "str".to_owned()
    }

    pub fn out_res_string() -> Result<String, &'static str> {
        Ok("str".to_owned())
    }

    pub fn out_bool() -> bool {
        true
    }

    pub fn out_res_bool() -> Result<bool, &'static str> {
        Ok(true)
    }

    pub fn out_duration() -> Duration {
        Duration::new(0, 0)
    }

    pub fn out_res_duration() -> Result<Duration, &'static str> {
        Ok(Duration::new(0, 0))
    }

    pub fn build_ip4() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(12, 34, 56, 78)), 9012)
    }

    pub fn build_ip6() -> SocketAddr {
        SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(
                0x1234, 0x5678, 0x9012, 0x3456, 0x7890, 0x1111, 0x2222, 0x3333,
            )),
            4444,
        )
    }

    pub fn print_ip(maybe_ip: Option<SocketAddr>) -> String {
        match maybe_ip {
            None => "0.0.0.0".to_string(),
            Some(ip) => ip.to_string(),
        }
    }

    // this is a pretty terrible idea, the request body is probably big, and your workspace is tiny,
    // but hey, it's a test function
    pub unsafe fn req_body(ctx: &mut Ctx) -> Result<VCL_STRING, VclError> {
        // open a ws reservation and blast the body into it
        let mut buf = ctx.ws.vcl_string_builder()?;
        for chunk in ctx.cached_req_body()? {
            buf.write_all(chunk).map_err(|_| "workspace issue")?;
        }
        Ok(buf.finish())
    }

    pub fn default_arg(#[default("foo")] arg: &str) -> &str {
        arg
    }

    pub fn cowprobe_prop(probe: Option<CowProbe<'_>>) -> String {
        probe_prop(probe.map(|v| v.to_owned()))
    }

    pub fn probe_prop(probe: Option<Probe>) -> String {
        match probe {
            Some(probe) => format!(
                "{}-{}-{}-{}-{}-{}",
                match probe.request {
                    Request::Url(url) => format!("url:{url}"),
                    Request::Text(text) => format!("text:{text}"),
                },
                probe.threshold,
                probe.timeout.as_secs(),
                probe.interval.as_secs(),
                probe.initial,
                probe.window
            ),
            None => "no probe".to_string(),
        }
    }

    pub fn merge_all_names(ctx: &Ctx) -> String {
        let mut s = String::new();
        for (k, _) in ctx.http_req.as_ref().expect("merge_all_names is only available in a client context") {
            s += k;
        }
        s
    }

    #[event]
    pub fn event(event: Event, vfp: &mut FetchFilters) {
        if let Event::Load = event {
            vfp.register::<VFPTest>();
        }
    }
}

// Test issue 20 - null pointer drop
struct VFPTest {
    _buffer: Vec<u8>,
}

// Force a pass here to test to make sure that fini does not panic due to a null priv1 member
impl FetchProcessor for VFPTest {
    fn name() -> &'static CStr {
        c"vfptest"
    }

    fn new(_: &mut Ctx, _: &mut FetchProcCtx) -> InitResult<Self> {
        InitResult::Pass
    }

    fn pull(&mut self, _: &mut FetchProcCtx, _: &mut [u8]) -> PullResult {
        PullResult::Err
    }
}
