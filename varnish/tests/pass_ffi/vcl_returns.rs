use varnish::vmod;

fn main() {}

// FIXME: Some of the Result<T, E> return types are not implemented yet

#[vmod]
mod vcl_returns {
    use varnish::ffi::{
        VCL_ACL, VCL_BACKEND, VCL_BLOB, VCL_BODY, VCL_BOOL, VCL_BYTES, VCL_DURATION, VCL_ENUM,
        VCL_HEADER, VCL_HTTP, VCL_INSTANCE, VCL_INT, VCL_IP, VCL_PROBE, VCL_REAL, VCL_REGEX,
        VCL_STEVEDORE, VCL_STRANDS, VCL_STRING, VCL_SUB, VCL_TIME, VCL_VCL,
    };

    pub fn arg_vcl_backend(backend: VCL_BACKEND) {
    }

    pub unsafe fn val_acl() -> VCL_ACL {
        VCL_ACL::default()
    }
    pub unsafe fn res_acl() -> Result<VCL_ACL, &'static str> {
        Err("")
    }
    pub unsafe fn val_backend() -> VCL_BACKEND {
        VCL_BACKEND::default()
    }
    pub unsafe fn res_backend() -> Result<VCL_BACKEND, &'static str> {
        Err("")
    }
    pub unsafe fn val_blob() -> VCL_BLOB {
        VCL_BLOB::default()
    }
    pub unsafe fn res_blob() -> Result<VCL_BLOB, &'static str> {
        Err("")
    }
    pub unsafe fn val_body() -> VCL_BODY {
        VCL_BODY::default()
    }
    pub unsafe fn res_body() -> Result<VCL_BODY, &'static str> {
        Err("")
    }
    pub unsafe fn val_bool() -> VCL_BOOL {
        VCL_BOOL::default()
    }
    pub unsafe fn res_bool() -> Result<VCL_BOOL, &'static str> {
        Err("")
    }
    pub unsafe fn val_bytes() -> VCL_BYTES {
        VCL_BYTES::default()
    }
    pub unsafe fn res_bytes() -> Result<VCL_BYTES, &'static str> {
        Err("")
    }
    pub unsafe fn val_duration() -> VCL_DURATION {
        VCL_DURATION::default()
    }
    pub unsafe fn res_duration() -> Result<VCL_DURATION, &'static str> {
        Err("")
    }
    pub unsafe fn val_enum() -> VCL_ENUM {
        VCL_ENUM::default()
    }
    pub unsafe fn res_enum() -> Result<VCL_ENUM, &'static str> {
        Err("")
    }
    pub unsafe fn val_header() -> VCL_HEADER {
        VCL_HEADER::default()
    }
    pub unsafe fn res_header() -> Result<VCL_HEADER, &'static str> {
        Err("")
    }
    pub unsafe fn val_http() -> VCL_HTTP {
        VCL_HTTP::default()
    }
    pub unsafe fn res_http() -> Result<VCL_HTTP, &'static str> {
        Err("")
    }
    pub unsafe fn val_instance() -> VCL_INSTANCE {
        panic!()
    }
    // pub unsafe fn res_instance() -> Result<VCL_INSTANCE, &'static str> {
    //     Err("")
    // }
    pub unsafe fn val_int() -> VCL_INT {
        VCL_INT::default()
    }
    pub unsafe fn res_int() -> Result<VCL_INT, &'static str> {
        Err("")
    }
    pub unsafe fn val_ip() -> VCL_IP {
        VCL_IP::default()
    }
    pub unsafe fn res_ip() -> Result<VCL_IP, &'static str> {
        Err("")
    }
    pub unsafe fn val_probe() -> VCL_PROBE {
        VCL_PROBE::default()
    }
    pub unsafe fn res_probe() -> Result<VCL_PROBE, &'static str> {
        Err("")
    }
    pub unsafe fn val_real() -> VCL_REAL {
        VCL_REAL::default()
    }
    pub unsafe fn res_real() -> Result<VCL_REAL, &'static str> {
        Err("")
    }
    pub unsafe fn val_regex() -> VCL_REGEX {
        VCL_REGEX::default()
    }
    pub unsafe fn res_regex() -> Result<VCL_REGEX, &'static str> {
        Err("")
    }
    pub unsafe fn val_stevedore() -> VCL_STEVEDORE {
        VCL_STEVEDORE::default()
    }
    pub unsafe fn res_stevedore() -> Result<VCL_STEVEDORE, &'static str> {
        Err("")
    }
    pub unsafe fn val_strands() -> VCL_STRANDS {
        VCL_STRANDS::default()
    }
    pub unsafe fn res_strands() -> Result<VCL_STRANDS, &'static str> {
        Err("")
    }
    pub unsafe fn val_string() -> VCL_STRING {
        VCL_STRING::default()
    }
    pub unsafe fn res_string() -> Result<VCL_STRING, &'static str> {
        Err("")
    }
    pub unsafe fn val_sub() -> VCL_SUB {
        VCL_SUB::default()
    }
    pub unsafe fn res_sub() -> Result<VCL_SUB, &'static str> {
        Err("")
    }
    pub unsafe fn val_time() -> VCL_TIME {
        VCL_TIME::default()
    }
    pub unsafe fn res_time() -> Result<VCL_TIME, &'static str> {
        Err("")
    }
    pub unsafe fn val_vcl() -> VCL_VCL {
        VCL_VCL::default()
    }
    pub unsafe fn res_vcl() -> Result<VCL_VCL, &'static str> {
        Err("")
    }
}
