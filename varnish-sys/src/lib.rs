extern crate core;

// FIXME: `improper_ctypes` should be `expected`
//    but a nightly version is having issues with it
#[allow(improper_ctypes)]
#[allow(non_snake_case)]
#[allow(clippy::manual_div_ceil)]
#[expect(non_camel_case_types, non_upper_case_globals)]
#[expect(clippy::pedantic)]
// The smaller vrt-only bindings set doesn't trigger these lints, so plain `expect` would fail
// to compile under `--no-default-features` — and, since `varnish-sys` ends up compiled as two
// different feature-variants within a single `cargo clippy --workspace` invocation (once for
// `varnish-macros`'s host-side build-dependency edge, once for `varnish`'s normal one),
// `expect`'s fulfillment tracking gets confused across the two even when only checking the
// default (full) target — hence gating on the feature rather than just using plain `expect`.
#[cfg_attr(
    feature = "full",
    expect(
        clippy::approx_constant,
        clippy::ptr_offset_with_cast,
        clippy::too_many_arguments,
        clippy::useless_transmute,
        unused_qualifications,
    )
)]
#[cfg_attr(
    not(feature = "full"),
    allow(
        clippy::approx_constant,
        clippy::ptr_offset_with_cast,
        clippy::too_many_arguments,
        clippy::useless_transmute,
        unused_qualifications,
    )
)]
pub mod ffi {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

mod extensions;
mod txt;
#[cfg(feature = "full")]
mod utils;

mod validate;

pub mod vcl;

#[cfg(feature = "full")]
pub use utils::*;
pub use validate::*;
