#[varnish::vmod]
mod empty {}

#[varnish::vmod]
mod err_fn {
    fn non_public() {}
    pub async fn async_fn() {}
    pub unsafe fn unsafe_fn() {}
    pub fn ret_vcl() -> Result<VCL_STRING, &'static str> {
        Err("error")
    }
    pub fn ret_opt_sub() -> Option<Subroutine> {
        None
    }
}

fn main() {}
