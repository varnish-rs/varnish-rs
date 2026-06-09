//! Types used by VMOD code, plus a guide for building VMODs.
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
//! ```rust,no_run
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
//!
//! For advanced function attributes (`#[restrict]`, `#[shared_per_task]`, `#[shared_per_vcl]`,
//! `#[vcl_name]`), see the [`varnish::vmod`](varnish_macros::vmod) attribute documentation.
//!
//! # Backends and directors
//!
//! As a C construct, [`crate::ffi::VCL_BACKEND`] can be a bit confusing to create and manipulate, notably as it
//! involves a bunch of structures with different lifetimes and quite a lot of casting. This
//! module hopes to alleviate those issues by handling the most of them and by offering a more
//! idiomatic interface centered around vmod objects.
//!
//! Here's what's in the toolbox:
//! - [`Backend`]: an "actual" backend that can be used by Varnish to create an HTTP response. It
//!   relies on two traits:
//!   - [`VclBackend`] reports health, and generates the response headers
//!   - [`VclResponse`] for the response body writer, structs implementing that trait are
//!     returned by [`VclBackend::get_response`]
//! - [`NativeBackend`]: a specialization of [`Backend`], relying on the native Varnish
//!   implementation providing IP and UDS backends
//! - [`NativeBackendBuilder`]: a builder to easily create a [`NativeBackend`]
//! - [`Director`]: a routing object doesn't create responses, but insead pick a [`Backend`]
//!   or [`Director`] object based on the HTTP request, based on the [`VclDirector`].
//! - [`BackendRef`]: a refcounted wrapper around [`Backend`] and [`Director`], this is the primary
//!   type used for arguments and returns of vmod functions.
//!
//!   **Important:** all these types wraps refcounted C structures that Varnish will try to free
//!   when a VCL goes cold, which means you can't hold onto them forever. It's not as scary as it
//!   sounds, and you can approach this two different ways.
//!
//!   Use a vmod object that will own them. The object will automatically be dropped at the end of
//!   the VCL lifetime, as will all its fields, and all the types above will automatically decrease
//!   their refcount to the underlying C structure when this happens.
//!
//!   Otherwise, your vmod can implement the event function and drop the structs on a cold event.

mod backend;
mod convert;
mod ctx;
mod error;
mod http;
mod probe;
mod processor;
mod str_or_bytes;
pub mod subroutine;
mod vsb;
mod ws;
mod ws_str_buffer;

pub use backend::*;
pub use convert::*;
pub use ctx::*;
pub use error::*;
pub use http::*;
pub use probe::*;
pub use processor::*;
pub use str_or_bytes::*;
pub use vsb::*;
pub use ws::*;
pub use ws_str_buffer::WsStrBuffer;

pub use crate::ffi::{VclEvent as Event, VslTag as LogTag};
