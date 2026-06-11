//! Health check probe definitions for Varnish backends
//!
//! A [`Probe`] describes how Varnish should actively check whether a backend is healthy.
//! It is passed to Varnish when constructing a backend, and Varnish will periodically send
//! the probe request and evaluate the response against the configured thresholds.
//!
//! Use [`Request::Url`] for a simple HTTP GET probe, or [`Request::Text`] to send a raw
//! request string (useful for non-HTTP protocols or custom verbs).
//!
//! [`CowProbe`] is a borrowed variant that avoids allocation when converting from VCL types.

use std::borrow::Cow;
use std::ffi::{c_char, c_uint, CStr};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::ffi::{vrt_backend_probe, VCL_DURATION, VCL_PROBE, VRT_BACKEND_PROBE_MAGIC};
use crate::vcl::{IntoVCL, VclError, Workspace};

/// The request sent by Varnish to check backend health.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum Request<T> {
    /// Send an HTTP `GET` request to this URL path (e.g. `"/health"`).
    Url(T),
    /// Send a raw request string verbatim (e.g. a full HTTP request or a custom protocol message).
    Text(T),
}

/// A backend health check probe definition.
///
/// Describes how frequently Varnish polls a backend and what constitutes a healthy response.
/// Pass this to the backend constructor; Varnish owns the actual probing loop.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Probe<T = String> {
    /// The request to send on each probe attempt.
    pub request: Request<T>,
    /// How long to wait for a response before considering the probe failed.
    pub timeout: Duration,
    /// How often to send probe requests.
    pub interval: Duration,
    /// The HTTP status code considered healthy (e.g. `200`). `0` means any 2xx status.
    pub exp_status: c_uint,
    /// Number of most recent probes to consider when evaluating health (sliding window size).
    pub window: c_uint,
    /// Minimum number of successful probes within [`window`](Self::window) to consider the backend healthy.
    pub threshold: c_uint,
    /// Number of probes that are assumed successful when the backend starts up.
    pub initial: c_uint,
}

/// A [`Probe`] that borrows its request string to avoid allocation when converting from VCL.
pub type CowProbe<'a> = Probe<Cow<'a, str>>;

impl CowProbe<'_> {
    /// Convert this borrowed probe into an owned [`Probe<String>`].
    pub fn to_owned(&self) -> Probe {
        Probe {
            request: match &self.request {
                Request::Url(cow) => Request::Url(cow.to_string()),
                Request::Text(cow) => Request::Text(cow.to_string()),
            },
            timeout: self.timeout,
            interval: self.interval,
            exp_status: self.exp_status,
            window: self.window,
            threshold: self.threshold,
            initial: self.initial,
        }
    }
}

/// Helper to convert a probe into a VCL object
pub(crate) fn into_vcl_probe<T: AsRef<str>>(
    src: Probe<T>,
    ws: &mut Workspace,
) -> Result<VCL_PROBE, VclError> {
    let probe = ws.copy_value(vrt_backend_probe {
        magic: VRT_BACKEND_PROBE_MAGIC,
        timeout: src.timeout.into(),
        interval: src.interval.into(),
        exp_status: src.exp_status,
        window: src.window,
        initial: src.initial,
        ..Default::default()
    })?;

    match src.request {
        Request::Url(s) => {
            probe.url = s.as_ref().into_vcl(ws)?.0;
        }
        Request::Text(s) => {
            probe.request = s.as_ref().into_vcl(ws)?.0;
        }
    }

    Ok(VCL_PROBE(probe))
}

/// Helper to convert a VCL probe into a Rust probe wrapper
pub(crate) fn from_vcl_probe<'a, T: From<Cow<'a, str>>>(value: VCL_PROBE) -> Option<Probe<T>> {
    let pr = unsafe { value.0.as_ref()? };
    assert!(
        (pr.url.is_null() && !pr.request.is_null()) || pr.request.is_null() && !pr.url.is_null()
    );
    Some(Probe {
        request: if pr.url.is_null() {
            Request::Text(from_str(pr.request).into())
        } else {
            Request::Url(from_str(pr.url).into())
        },
        timeout: VCL_DURATION(pr.timeout).into(),
        interval: VCL_DURATION(pr.interval).into(),
        exp_status: pr.exp_status,
        window: pr.window,
        threshold: pr.threshold,
        initial: pr.initial,
    })
}

/// Helper function to convert a C string into a Rust string
fn from_str<'a>(value: *const c_char) -> Cow<'a, str> {
    if value.is_null() {
        Cow::Borrowed("")
    } else {
        // FIXME: this should NOT be lossy IMO
        unsafe { CStr::from_ptr(value).to_string_lossy() }
    }
}
