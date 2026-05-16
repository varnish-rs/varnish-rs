extern crate core;

// FIXME: `improper_ctypes` should be `expected`
//    but a nightly version is having issues with it
#[allow(improper_ctypes)]
#[allow(non_snake_case)]
#[expect(non_camel_case_types, non_upper_case_globals, unused_qualifications)]
#[expect(
    clippy::approx_constant,
    clippy::pedantic,
    clippy::ptr_offset_with_cast,
    clippy::too_many_arguments,
    clippy::useless_transmute
)]
pub mod ffi {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

mod extensions;
mod txt;
mod utils;

mod validate;

pub mod vcl;

pub use utils::*;
pub use validate::*;
