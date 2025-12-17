//! # Varnish bindings
//!
//! This module provides access to various [Varnish](http://varnish-cache.org/) facilities, notably those needed to create
//! pure-rust vmods (check out examples [here](https://github.com/varnish-rs/varnish-rs/tree/main/examples)).
//! Note that it doesn't aim to be a 1-to-1 mirror of the C API, as Rust allows for better
//! ergonomics than what the C code can provide (notably around strings and buffer handling).
//!
//! # Building a VMOD
//!
//! The main idea for this crate is to make the building framework as light as possible for the
//! vmod writer, here's a checklist, but you can also just check the [source
//! code](https://github.com/varnish-rs/varnish-rs/tree/main/examples/vmod_example).
//!
//! The general structure of your code should look like this:
//!
//! ```text
//! .
//! ├── Cargo.lock       # This code is a cdylib, so you should lock and track dependencies
//! ├── Cargo.toml       # Add varnish as a dependency here
//! ├── README.md        # This file can be auto-generated/updated by the Varnish macro
//! ├── src
//! │   └── lib.rs       # Your main code that uses  #[vmod(docs = "README.md")]
//! └── tests
//!     └── test01.vtc   # Your VTC tests, executed with  run_vtc_tests!("tests/*.vtc") in lib.rs
//! ```
//!
//! ## Cargo.toml
//!
//! ```toml
//! [dependencies]
//! varnish = "..."
//! ```
//!
//! ## src/lib.rs
//!
//! ```rust
//! // Run all matching tests as part of `cargo test` using varnishtest utility. Fails if no tests are found.
//! // Due to some limitations, make sure to run `cargo build` before `cargo test`
//! varnish::run_vtc_tests!("tests/*.vtc");
//!
//! /// A VMOD must have one module tagged with `#[varnish::vmod]`.  All public functions in this module
//! /// will be exported as Varnish VMOD functions.  The name of the module will be the name of the VMOD.
//! /// Use `#[varnish::vmod(docs = "README.md")]` to auto-generate a `README.md` file from the doc comments.
//! #[varnish::vmod]
//! mod hello_world {
//!     /// This function becomes available in VCL as `hello_world.is_even`
//!     pub fn is_even(n: i64) -> bool {
//!         n % 2 == 0
//!     }
//! }
//! ```
//!
//! ## tests/test01.vtc
//!
//! This test will check that the `is_even` function works as expected. Make sure to run `cargo build` before `cargo test`.
//!
//! ```vtc
//! server s1 {
//!     rxreq
//!     expect req.http.even == "true"
//!     txresp
//! } -start
//!
//! varnish v1 -vcl+backend {
//!     import hello_world from "${vmod}";
//!
//!     sub vcl_recv {
//!         set req.http.even = hello_world.is_even(8);
//!     }
//! } -start
//!
//! client c1 {
//!     txreq
//!     rxresp
//!     expect resp.status == 200
//! ```

// Re-publish some varnish_sys modules
pub use varnish_sys::vcl;

#[cfg(not(feature = "ffi"))]
#[doc(hidden)]
pub mod ffi {
    // This list must match the `use_ffi_items` in generator.rs
    #[cfg(varnishsys_6_priv_free_f)]
    pub use varnish_sys::ffi::vmod_priv_free_f;
    pub use varnish_sys::ffi::{
        vmod_data, vmod_priv, vrt_ctx, VMOD_ABI_Version, VclEvent, VCL_BACKEND, VCL_BLOB, VCL_BOOL,
        VCL_DURATION, VCL_INT, VCL_IP, VCL_PROBE, VCL_REAL, VCL_STRING, VCL_TIME, VCL_VOID,
    };
    #[cfg(not(varnishsys_6_priv_free_f))]
    pub use varnish_sys::ffi::{vmod_priv_methods, VMOD_PRIV_METHODS_MAGIC};
}

#[cfg(feature = "ffi")]
pub use varnish_sys::ffi;

pub mod varnishtest;

mod metrics_reader;
pub use metrics_reader::{Metric, MetricFormat, MetricsReader, MetricsReaderBuilder, Semantics};

mod metrics_publisher;
pub use metrics_publisher::{Vsc, VscMetric};
pub use varnish_macros::{vmod, VscMetric};

/// Run all VTC tests using `varnishtest` utility.
///
/// Varnish provides a very handy tool for end-to-end testing:
/// [`varnishtest`](https://varnish-cache.org/docs/trunk/reference/varnishtest.html) which will
/// test various scenarios you describe in a [`VTC file`](https://varnish-cache.org/docs/trunk/reference/vtc.html):
///
/// ```rust
/// varnish::run_vtc_tests!("tests/*.vtc");
/// ```
///
/// This will create all the necessary code to run `varnishtest` alongside your unit
/// tests when you run `cargo test`.
///
/// **Important note:** you need to first build your vmod (i.e., with `cargo build`) before the tests can be run,
/// otherwise you'll get a panic.
///
/// Tests will automatically time out after 5s. To override, set `VARNISHTEST_DURATION` env var.
///
/// To debug the tests, pass `true` as the second argument:
/// ```rust
/// varnish::run_vtc_tests!("tests/*.vtc", true);
/// ```
#[macro_export]
macro_rules! run_vtc_tests {
    ( $glob_path:expr ) => {
        $crate::run_vtc_tests!($glob_path, false);
    };
    ( $glob_path:expr, $debug:expr ) => {
        #[cfg(test)]
        #[test]
        fn run_vtc_tests() {
            if let Err(err) = $crate::varnishtest::run_all_tests(
                env!("LD_LIBRARY_PATH"),
                env!("CARGO_PKG_NAME"),
                $glob_path,
                option_env!("VARNISHTEST_DURATION").unwrap_or("5s"),
                $debug,
            ) {
                panic!("{err}");
            }
        }
    };
}
