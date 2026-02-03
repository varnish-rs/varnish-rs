//! Facilities to create a backend
//!
//! [`VCL_BACKEND`] can be a bit confusing to create and manipulate, notably as they
//! involve a bunch of structures with different lifetimes and quite a lot of casting. This
//! module hopes to alleviate those issues by handling the most of them and by offering a more
//! idiomatic interface centered around vmod objects.
//!
//! Here's what's in the toolbox:
//! - [`Backend`]: an "actual" backend that can be used by Varnish to create an HTTP response. It
//!   relies on two traits:
//!   - [`VclBackend`] reports health, and generates the response headers
//!   - [`VclResponse`] for the response body writer, structs implementing that trait are
//!     returned by [`VclBackend::get_response`]
//! - [`NativeBackend`]: a specialization of [`Backend`], rellying on the native Varnish
//!   implementation providing IP and UDS backends
//! - [`NativeBackendBuilder`]: a builder to easily create a [`NativeBackend`]
//! - [`Director`]: a routing object doesn't create responses, but insead pick a [`Backend`]
//!   or [`Director`] object based on the HTTP request, based on the [`VclDirector`].
//! - [`BackendRef`]: a refcounted wrapper around [`Backend`] and [`Director`], this is the primary
//!   type used for arguments and returns of vmod functions.

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
