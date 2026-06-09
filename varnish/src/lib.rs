//! Safe Rust bindings for [Varnish](http://varnish-cache.org/), providing everything needed to
//! write pure-Rust VMODs. See also the
//! [examples](https://github.com/varnish-rs/varnish-rs/tree/main/examples).
//!
//! For a guide to building a VMOD — project structure, `Cargo.toml`, VTC tests — see [`vcl`].

// Re-publish some varnish_sys modules
pub use varnish_sys::vcl;

// Re-export the report_details_json macro
pub use varnish_sys::report_details_json;

#[cfg(not(feature = "ffi"))]
#[doc(hidden)]
pub mod ffi {
    // This list must match the `use_ffi_items` in generator.rs
    pub use varnish_sys::ffi::{
        vmod_data, vmod_priv, vmod_priv_methods, vrt_ctx, VMOD_ABI_Version, VclEvent, VCL_BACKEND,
        VCL_BLOB, VCL_BOOL, VCL_DURATION, VCL_INT, VCL_IP, VCL_MET_BACKEND_ERROR,
        VCL_MET_BACKEND_FETCH, VCL_MET_BACKEND_REFRESH, VCL_MET_BACKEND_RESPONSE, VCL_MET_DELIVER,
        VCL_MET_FINI, VCL_MET_HASH, VCL_MET_HIT, VCL_MET_INIT, VCL_MET_MISS, VCL_MET_PASS,
        VCL_MET_PIPE, VCL_MET_PURGE, VCL_MET_RECV, VCL_MET_SYNTH, VCL_MET_TASK_B, VCL_MET_TASK_C,
        VCL_MET_TASK_H, VCL_PROBE, VCL_REAL, VCL_STRING, VCL_SUB, VCL_TIME, VCL_VOID,
        VMOD_PRIV_METHODS_MAGIC,
    };
}

#[cfg(feature = "ffi")]
pub use varnish_sys::ffi;

pub mod varnishtest;

mod metrics_reader;
pub use metrics_reader::{Metric, MetricFormat, MetricsReader, MetricsReaderBuilder, Semantics};

mod metrics_publisher;
pub use metrics_publisher::{Vsc, VscMetric};
pub use varnish_macros::{run_vtc_tests, vmod, VscMetric};
