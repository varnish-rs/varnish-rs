//! Facilities to create a VMOD backend
//!
//! [`VCL_BACKEND`] can be a bit confusing to create and manipulate, notably as they
//! involve a bunch of structures with different lifetimes and quite a lot of casting. This
//! module hopes to alleviate those issues by handling the most of them and by offering a more
//! idiomatic interface centered around vmod objects.
//!
//! Here's what's in the toolbox:
//! - the [`Backend`] type wraps a [`VclBackend`]-implementing struct into a C backend
//! - the [`VclBackend`] trait defines which methods to implement to act as a backend, and includes
//!   default implementations for most methods.
//! - the [`VclResponse`] trait provides a way to generate a response body,notably handling the
//!   transfer-encoding for you.
//!
//! Note: You can check out the [example/vmod_be
//! code](https://github.com/varnish-rs/varnish-rs/blob/main/examples/vmod_be/src/lib.rs) for a
//! fully commented vmod.
//!
//! For a very simple example, let's build a backend that just replies with 'A' a predetermined
//! number of times.
//!
//! ```
//! # mod varnish { pub use varnish_sys::vcl; }
//! use varnish::vcl::{Ctx, Backend, VclBackend, VclResponse, VclError};
//!
//! // First we need to define a struct that implement `VclResponse`:
//! struct BodyResponse {
//!     left: usize,
//! }
//!
//! impl VclResponse for BodyResponse {
//!     fn read(&mut self, buf: &mut [u8]) -> Result<usize, VclError> {
//!         let mut done = 0;
//!         for p in buf {
//!              if self.left == 0 {
//!                  break;
//!              }
//!              *p = 'A' as u8;
//!              done += 1;
//!              self.left -= 1;
//!         }
//!         Ok(done)
//!     }
//! }
//!
//! // Then, we need a struct implementing `VclBackend` to build the headers and return a BodyResponse
//! // Here, MyBe only needs to know how many times to repeat the character
//! struct MyBe {
//!     n: usize
//! }
//!
//! impl VclBackend<BodyResponse> for MyBe {
//!      fn get_response(&self, ctx: &mut Ctx) -> Result<Option<BodyResponse>, VclError> {
//!          Ok(Some(
//!            BodyResponse { left: self.n },
//!          ))
//!      }
//! }
//!
//! // Finally, we create a `Backend` wrapping a `MyBe`, and we can ask for a pointer to give to the C
//! // layers.
//! fn some_vmod_function(ctx: &mut Ctx) {
//!     let backend = Backend::new(ctx, "Arepeater", "repeat42", MyBe { n: 42 }).expect("couldn't create the backend");
//!     let ptr = backend.vcl_ptr();
//! }
//! ```

mod backend_main;
mod backend_ref;
mod director;

pub use backend_main::*;
pub use backend_ref::{BackendRef, ProbeResult};
pub use director::{Director, VclDirector};

/// Creates a JSON string with custom indentation and writes it to a Buffer:
/// - Top level line has no indentation
/// - All other lines have 6 extra spaces on top of normal indentation
/// - Appends a trailing comma and newline with indentation
///
/// Usage: `report_details_json!(vsb, serde_json::json!({ "key": "value" }))`
#[macro_export]
macro_rules! report_details_json {
    ($vsb:expr, $json_value:expr) => {{
        let json_str =
            serde_json::to_string_pretty(&$json_value).expect("Failed to serialize JSON");

        let indent = "      "; // 6 spaces
        let lines: Vec<&str> = json_str.lines().collect();
        if let Some((first, rest)) = lines.split_first() {
            let _ = $vsb.write(first);
            for line in rest {
                let _ = $vsb.write(&"\n");
                let _ = $vsb.write(&indent);
                let _ = $vsb.write(line);
            }
        } else {
            let _ = $vsb.write(&json_str);
        }
        let _ = $vsb.write(&",\n");
        let _ = $vsb.write(&indent);
    }};
}
