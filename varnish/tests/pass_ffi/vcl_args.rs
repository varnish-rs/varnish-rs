use varnish::vmod;

fn main() {}

// FIXME: Some of the Result<T, E> return types are not implemented yet

#[vmod]
mod vcl_args {
    use varnish::ffi::VCL_BACKEND;

    pub fn arg_vcl_backend(_backend: VCL_BACKEND) {
    }
}
