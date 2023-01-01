/// The boilerplate code generated by `vmodtool-rs.py` uses C types, which are
/// defined in `varnish_sys`. To avoid requiring this crate as a build
/// dependency, we reexport the handful of types we need here.
///
/// From a vmod point of view, you'll probably never only need those. You'll be
/// able to ignore them completely and use the `rust` equivalents, or you'll
/// need to tap into `varnish_sys` directly.
pub use varnish_sys::{ vrt_ctx, vmod_priv, vcl_event_e, VMOD_ABI_Version, VRT_MINOR_VERSION, VRT_MAJOR_VERSION , VCL_ACL , VCL_BACKEND , VCL_BLOB , VCL_BODY , VCL_BOOL , VCL_BYTES , VCL_DURATION , VCL_ENUM , VCL_HEADER , VCL_HTTP , VCL_INT , VCL_IP , VCL_PROBE , VCL_REAL , VCL_REGEX , VCL_STEVEDORE , VCL_STRANDS , VCL_STRING , VCL_SUB , VCL_TIME , VCL_VOID };
