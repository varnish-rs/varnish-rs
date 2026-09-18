mod acl;
#[cfg(feature = "full")]
mod backend;
mod convert;
mod ctx;
mod error;
#[cfg(feature = "full")]
mod http;
#[cfg(feature = "full")]
mod probe;
#[cfg(feature = "full")]
mod processor;
mod str_or_bytes;
pub mod subroutine;
#[cfg(feature = "full")]
mod vsb;
mod ws;
#[cfg(feature = "full")]
mod ws_str_buffer;

pub use acl::*;
pub use convert::*;
pub use ctx::*;
pub use error::*;
pub use str_or_bytes::*;
pub use ws::*;

pub use crate::ffi::VclEvent as Event;

// `LogTag` (VSL logging, `Ctx::log`/the free `log()` fn) and everything from these five
// modules needs cache.h-only APIs unavailable under the vrt-only surface — grouped into one
// `use` so the `full` gate isn't repeated on each.
#[cfg(feature = "full")]
pub use {
    crate::ffi::VslTag as LogTag, backend::*, http::*, probe::*, processor::*, vsb::*,
    ws_str_buffer::WsStrBuffer,
};
