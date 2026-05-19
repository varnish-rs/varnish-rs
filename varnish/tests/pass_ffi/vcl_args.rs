use varnish::vmod;

fn main() {}

// FIXME: Some of the Result<T, E> return types are not implemented yet

#[vmod]
mod vcl_args {
    use varnish::ffi::{VCL_BACKEND, VCL_SUB};

    pub fn arg_vcl_backend(_backend: VCL_BACKEND) {
    }

    pub fn arg_vcl_sub(_sub: VCL_SUB) {
    }
}
