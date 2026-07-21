use std::io::{Cursor, Write};

use varnish::vcl::{Backend, Ctx, VclBackend, VclError, VclResponse, VclResult};

varnish::run_vtc_tests!("tests/*.vtc");

struct Echo {
    backend: Backend<EchoBackend, EchoResponse>,
}

/// a backend that reads bereq's body and echoes it back as the response body
#[varnish::vmod(docs = "README.md")]
mod echo_backend {
    use varnish::ffi::VCL_BACKEND;
    use varnish::vcl::{Backend, Ctx, VclError};

    use super::{Echo, EchoBackend};

    /// Echo is our VCL object, which just holds a rust Backend,
    /// it only needs two functions:
    /// - `new()`, so that the VCL can instantiate it
    /// - `backend()`, so that we can produce a C pointer for varnish to use
    impl Echo {
        pub fn new(ctx: &mut Ctx, #[vcl_name] name: &str) -> Result<Self, VclError> {
            let backend = Backend::new(ctx, "echo", name, EchoBackend, false)?;
            Ok(Echo { backend })
        }

        pub unsafe fn backend(&self) -> VCL_BACKEND {
            self.backend.as_ref().vcl_ptr()
        }
    }
}

/// [`EchoBackend`] demonstrates [`Ctx::req_body`]: it copies bereq's body
/// (whether streamed live from the client or already cached) and sends it back
/// as the response body, unmodified.
struct EchoBackend;

impl VclBackend<EchoResponse> for EchoBackend {
    fn get_response(&self, ctx: &mut Ctx) -> VclResult<Option<EchoResponse>> {
        let state = format!("{:?}", ctx.req_body_state()?);

        // `x-fail-after-bytes`, if present, demonstrates that a `req_body()`
        // error surfaced by the *writer itself* (as opposed to a C-side read
        // error) propagates correctly.
        let fail_after = ctx
            .http_bereq
            .as_ref()
            .and_then(|h| h.header("x-fail-after-bytes"))
            .and_then(|v| std::str::from_utf8(v.as_ref()).ok()?.parse::<usize>().ok());

        let (had_body, body) = if let Some(remaining) = fail_after {
            let mut writer = FailAfter {
                remaining,
                inner: Vec::new(),
            };
            let had_body = ctx.req_body(&mut writer)?;
            (had_body, writer.inner)
        } else {
            let mut body = Vec::new();
            let had_body = ctx.req_body(&mut body)?;
            (had_body, body)
        };

        let beresp = ctx.http_beresp.as_mut().expect("http_beresp must be set");
        beresp.set_status(200);
        beresp.set_header("x-had-body", if had_body { "1" } else { "0" })?;
        beresp.set_header("x-body-state", &state)?;

        Ok(Some(EchoResponse {
            inner: Cursor::new(body),
        }))
    }
}

/// A `Write` that errors once `remaining` bytes have been accepted, to exercise
/// `req_body`'s writer-error path (as opposed to a C-side read failure).
struct FailAfter {
    inner: Vec<u8>,
    remaining: usize,
}

impl Write for FailAfter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Err(std::io::Error::other("synthetic write failure for testing"));
        }
        let n = buf.len().min(self.remaining);
        let written = self.inner.write(&buf[..n])?;
        self.remaining -= written;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

struct EchoResponse {
    inner: Cursor<Vec<u8>>,
}

impl VclResponse for EchoResponse {
    fn read(&mut self, buf: &mut [u8]) -> VclResult<usize> {
        use std::io::Read;
        self.inner
            .read(buf)
            .map_err(|e| VclError::new(e.to_string()))
    }

    fn len(&self) -> Option<usize> {
        Some(self.inner.get_ref().len())
    }
}
