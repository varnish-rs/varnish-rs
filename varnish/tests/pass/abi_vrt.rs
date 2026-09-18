use varnish::vmod;

fn main() {}

#[vmod(abi = "vrt")]
mod abi_vrt {
    pub fn scalar_fn(x: i64) -> i64 {
        x
    }

    // Exercises the vrt-mode `Workspace` string path (`VRT_StrandsWS`), which only needs to
    // type-check here — actually calling it requires a real workspace from a running
    // varnishd, tested separately via `.vtc`.
    pub fn echo(s: &str) -> String {
        s.to_string()
    }
}
