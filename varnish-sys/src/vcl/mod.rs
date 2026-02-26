//! This module provides the types used by vmod code.
//!
//! # Backends and directors
//!
//! As C construct, [`crate::ffi::VCL_BACKEND`] can be a bit confusing to create and manipulate, notably as it
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

#[cfg(not(varnishsys_6))]
mod backend;
mod convert;
mod ctx;
mod error;
mod http;
mod probe;
#[cfg(not(varnishsys_6))]
mod processor;
mod str_or_bytes;
mod vsb;
mod ws;
mod ws_str_buffer;

#[cfg(not(varnishsys_6))]
pub use backend::*;
pub use convert::*;
pub use ctx::*;
pub use error::*;
pub use http::*;
pub use probe::*;
#[cfg(not(varnishsys_6))]
pub use processor::*;
pub use str_or_bytes::*;
pub use vsb::*;
pub use ws::*;
pub use ws_str_buffer::WsStrBuffer;

pub use crate::ffi::{VclEvent as Event, VslTag as LogTag};
