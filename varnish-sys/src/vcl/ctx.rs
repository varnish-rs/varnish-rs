//! Expose the Varnish context [`vrt_ctx`] as a Rust object
//!
use std::ffi::{c_int, c_uint, c_void, CStr};
use std::io::{self, Write};
use std::net::SocketAddr;

use crate::ffi;
use crate::ffi::{vrt_ctx, VRT_call, VRT_check_call, VRT_fail, VRT_handled, VRT_CTX_MAGIC};
use crate::vcl::{
    sc_to_ptr, subroutine::Subroutine, Acl, HttpHeaders, LogTag, StreamClose, TestWS, VclError,
    VclResult, Workspace,
};

/// VCL context
///
/// A mutable reference to this structure is always passed to vmod functions and provides access to
/// the available HTTP objects, as well as the workspace.
///
/// This struct is a pure Rust structure, mirroring some of the C fields, so you should always use
/// the provided methods to interact with them. If they are not enough, the `raw` field is actually
/// the C original pointer that can be used to directly, and unsafely, act on the structure.
///
/// Which `http_*` are present will depend on which VCL sub routine the function is called from.
///
/// ``` rust
/// # mod varnish { pub use varnish_sys::vcl; }
/// use varnish::vcl::Ctx;
///
/// fn foo(ctx: &Ctx) {
///     if let Some(ref req) = ctx.http_req {
///         for (name, value) in req {
///             println!("header {name} has value {value:?}");
///         }
///     }
/// }
/// ```
#[derive(Debug)]
pub struct Ctx<'a> {
    pub raw: &'a mut vrt_ctx,
    pub http_req: Option<HttpHeaders<'a>>,
    pub http_req_top: Option<HttpHeaders<'a>>,
    pub http_resp: Option<HttpHeaders<'a>>,
    pub http_bereq: Option<HttpHeaders<'a>>,
    pub http_beresp: Option<HttpHeaders<'a>>,
    pub ws: Workspace<'a>,

    req: Option<Req<'a>>,
}

/// The state of a request or response body, mirroring Varnish's `body_status_t`
/// (see `tbl/body_status.h`).
// `Eof` and `Error` aren't exercised by this crate's own tests: `Error`-as-a-
// returned-state isn't hit by this crate's own example/tests (which only call
// `req_body_state` once and surface read failures as `Err` directly), but
// nothing stops a `VclBackend` from calling `req_body_state()` again after a
// failed `req_body()` and observing `Error` itself; `Eof` is uncommon for
// request bodies specifically (no `Content-Length` or `Transfer-Encoding`,
// relying on connection close). `Taken` *is* exercised - see
// `varnish/tests/vmod_test/tests/test16.vtc`'s c10, which pins it by name:
// an uncached body already consumed when the real backend fetch forwarded
// it upstream (not a call into this crate - Varnish's own core does that),
// observed afterward from client context in `vcl_deliver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyState {
    /// No body.
    None,
    /// An error occurred while producing/reading the body.
    Error,
    /// Chunked transfer encoding; the length isn't known upfront.
    Chunked,
    /// A known length (e.g. `Content-Length`).
    Length,
    /// The body ends when the connection closes; the length isn't known upfront.
    Eof,
    /// The body has already been consumed by someone else.
    Taken,
    /// The body has been fully read and is cached as an object.
    Cached,
}

impl BodyState {
    fn from_raw(ptr: ffi::body_status_t) -> Self {
        unsafe {
            if ptr == ffi::BS_NONE.as_ptr() {
                Self::None
            } else if ptr == ffi::BS_ERROR.as_ptr() {
                Self::Error
            } else if ptr == ffi::BS_CHUNKED.as_ptr() {
                Self::Chunked
            } else if ptr == ffi::BS_LENGTH.as_ptr() {
                Self::Length
            } else if ptr == ffi::BS_EOF.as_ptr() {
                Self::Eof
            } else if ptr == ffi::BS_TAKEN.as_ptr() {
                Self::Taken
            } else if ptr == ffi::BS_CACHED.as_ptr() {
                Self::Cached
            } else {
                unreachable!("unknown body_status_t")
            }
        }
    }
}

/// State threaded through Varnish's body-iterate C callback via its `priv_` pointer.
///
/// Used by [`Ctx::req_body`].
struct BodyWriterState<'w, W: Write> {
    writer: &'w mut W,
    error: Option<io::Error>,
}

/// Bridges Varnish's body-iterate callback (`objiterate_f`) to `W::write_all`.
///
/// Monomorphized per `W`. `ObjIterate`/`VRB_Iterate` call this synchronously
/// and sequentially, so at most one `&mut` derived from `priv_` is ever live,
/// and it never outlives that call. Used by [`Ctx::req_body`].
unsafe extern "C" fn write_body_iterate<W: Write>(
    priv_: *mut c_void,
    _flush: c_uint,
    ptr: *const c_void,
    len: isize,
) -> c_int {
    if ptr.is_null() || len <= 0 {
        return 0;
    }
    let writer_state = priv_
        .cast::<BodyWriterState<W>>()
        .as_mut()
        .expect("body-iterate callback priv pointer must not be null");
    let buf = std::slice::from_raw_parts(ptr.cast::<u8>(), len as usize);
    match writer_state.writer.write_all(buf) {
        Ok(()) => 0,
        Err(e) => {
            writer_state.error = Some(e);
            1
        }
    }
}

/// Resolves a completed `VRB_Iterate` call into [`Ctx::req_body`]'s return value: a
/// captured writer error always takes priority over the raw C return code (`rv < 0`
/// signals a stream-side failure instead).
fn resolve_iterate_result<W: Write>(
    rv: isize,
    writer_state: &mut BodyWriterState<'_, W>,
    stream_err: &'static str,
) -> VclResult<bool> {
    if let Some(e) = writer_state.error.take() {
        return Err(e.to_string().into());
    }
    if rv < 0 {
        return Err(stream_err.into());
    }
    Ok(true)
}

impl<'a> Ctx<'a> {
    /// Wrap a raw pointer into an object we can use.
    ///
    /// The pointer must be non-null, and the magic must match
    pub unsafe fn from_ptr(ptr: *const vrt_ctx) -> Self {
        Self::from_ref(
            ptr.cast_mut()
                .as_mut()
                .expect("vrt_ctx pointer must not be null"),
        )
    }

    /// Instantiate from a mutable reference to a [`vrt_ctx`].
    pub fn from_ref(raw: &'a mut vrt_ctx) -> Self {
        assert_eq!(raw.magic, VRT_CTX_MAGIC);
        Self {
            http_req: HttpHeaders::from_ptr(raw.http_req),
            http_req_top: HttpHeaders::from_ptr(raw.http_req_top),
            http_resp: HttpHeaders::from_ptr(raw.http_resp),
            http_bereq: HttpHeaders::from_ptr(raw.http_bereq),
            http_beresp: HttpHeaders::from_ptr(raw.http_beresp),
            ws: Workspace::from_ptr(raw.ws),
            req: unsafe { Req::from_ptr(raw.req) },
            raw,
        }
    }

    /// Log an error message and fail the current VSL task.
    ///
    /// Once the control goes back to Varnish, it will see that the transaction was marked as fail
    /// and will return a synthetic error to the client.
    pub fn fail(&mut self, msg: impl Into<VclError>) {
        let msg = msg.into();
        let msg = msg.as_str();
        unsafe {
            VRT_fail(self.raw, c"%.*s".as_ptr(), msg.len(), msg.as_ptr());
        }
    }

    /// Log a message, attached to the current context
    pub fn log(&mut self, tag: LogTag, msg: impl AsRef<str>) {
        unsafe {
            let vsl = self.raw.vsl;
            if vsl.is_null() {
                log(tag, msg);
            } else {
                let msg = ffi::txt::from_str(msg.as_ref());
                ffi::VSLbt(vsl, tag, msg);
            }
        }
    }

    /// Match an ACL against a provided address.
    pub fn acl_match(&self, acl: &Acl, addr: SocketAddr) -> bool {
        assert!(!acl.raw.0.is_null());

        unsafe {
            let mut sa_buf = vec![0u8; ffi::vsa_suckaddr_len];
            crate::vcl::convert::write_ip_to_buf(addr, &mut sa_buf);
            ffi::VRT_acl_match(self.raw, acl.raw, ffi::VCL_IP(sa_buf.as_ptr().cast())) == 1
        }
    }

    /// Return the name of the listener socket that received the current request.
    ///
    /// This corresponds to the VCL variable `local.socket` and returns the `-a` socket
    /// name (e.g., `"a0"`, `"http-80"`). Returns an `Err` in backend context where the
    /// session isn't available, or if the name is non-UTF-8.
    pub fn local_socket(&self) -> Result<&'a str, VclError> {
        // we're breaking abstraction here, but the other ways are to just reimplement the
        // whole logic in rust (which is admittedly short), or to let the user crash
        if self.raw.req.is_null() && self.raw.bo.is_null() {
            return Err("local.socket isn't available in this context".into());
        }
        let raw = unsafe { ffi::VRT_r_local_socket(self.raw) };
        let cstr = <&CStr>::from(raw);
        Ok(cstr.to_str()?)
    }

    /// Return the address of the local endpoint that received the current request.
    ///
    /// This corresponds to the VCL variable `local.endpoint` and returns the address
    /// string (e.g., `"127.0.0.1:8080"`, `"/var/run/varnish.sock"`). Returns an `Err` in
    /// backend context where the session isn't available, or if the value is non-UTF-8.
    // same notes as for local_socket
    pub fn local_endpoint(&self) -> Result<&'a str, VclError> {
        if self.raw.req.is_null() && self.raw.bo.is_null() {
            return Err("local.endpoint isn't available in this context".into());
        }
        let raw = unsafe { ffi::VRT_r_local_endpoint(self.raw) };
        let cstr = <&CStr>::from(raw);
        Ok(cstr.to_str()?)
    }

    /// Call a VCL subroutine.
    ///
    /// Returns `Ok(true)` if the request was handled after the call, `Ok(false)` otherwise.
    /// Returns `Err` if the subroutine cannot be called in the current context (e.g. wrong VCL
    /// state or incompatible subroutine type).
    /// If `Ok(true)` was returned, no other subroutine can be called, and doing so will result
    /// in a VCL error.
    pub fn call_sub(&mut self, sub: Subroutine) -> Result<bool, VclError> {
        self.check_call_sub(sub)?;
        unsafe { VRT_call(self.raw, sub.vcl_ptr()) };
        Ok(self.is_handled())
    }

    /// Check whether a VCL subroutine can be called in the current context.
    ///
    /// Returns `Ok(())` if the call is valid, or `Err` with the reason otherwise.
    pub fn check_call_sub(&self, sub: Subroutine) -> Result<(), VclError> {
        let result = unsafe { VRT_check_call(self.raw, sub.vcl_ptr()) };
        if result.0.is_null() {
            Ok(())
        } else {
            let msg = unsafe { CStr::from_ptr(result.0) }
                .to_string_lossy()
                .into_owned();
            Err(VclError::new(msg))
        }
    }

    /// Returns `true` if the current request has already been handled.
    /// If `true`, no other subroutine can be called, and doing so will result
    /// in a VCL error.
    pub fn is_handled(&self) -> bool {
        unsafe { VRT_handled(self.raw) != 0 }
    }

    /// Retrieve the cached request body as a list of byte slices.
    ///
    /// Returns slices pointing into the workspace; each slice is a contiguous chunk of the body.
    /// Fails if the body has not been cached (i.e. `std.cache_req_body()` was not called in VCL
    /// before this subroutine ran).
    pub fn cached_req_body(&mut self) -> Result<Vec<&'a [u8]>, VclError> {
        unsafe extern "C" fn chunk_collector(
            priv_: *mut c_void,
            _flush: c_uint,
            ptr: *const c_void,
            len: isize,
        ) -> c_int {
            let v = priv_
                .cast::<Vec<&[u8]>>()
                .as_mut()
                .expect("cached_req_body callback priv pointer must not be null");
            let buf = std::slice::from_raw_parts(ptr.cast::<u8>(), len as usize);
            v.push(buf);
            0
        }

        let req = &mut *self.req.as_mut().ok_or("req object isn't available")?.raw;
        unsafe {
            if req.req_body_status != ffi::BS_CACHED.as_ptr() {
                return Err("request body hasn't been previously cached".into());
            }
        }
        let mut v: Box<Vec<&'a [u8]>> = Box::default();
        let p: *mut Vec<&'a [u8]> = &raw mut *v;
        match unsafe {
            ffi::VRB_Iterate(
                req.wrk,
                req.vsl.as_mut_ptr(),
                req,
                Some(chunk_collector),
                p.cast::<c_void>(),
            )
        } {
            0 => Ok(*v),
            _ => Err("req.body iteration failed".into()),
        }
    }

    /// Return the current state of the request body — `bereq`'s body from a
    /// backend context, or the client `req`'s body directly if called earlier
    /// (`vcl_recv` and later, before any backend is involved).
    ///
    /// From backend context (busyobj set - typically
    /// [`VclBackend::get_response`](crate::vcl::VclBackend::get_response)): if
    /// the body has already been cached as an object (e.g. after
    /// `std.cache_req_body()`, or on a fetch retry), returns
    /// [`BodyState::Cached`]; otherwise reflects the live client body's state,
    /// or [`BodyState::None`] if there is no client request to read from.
    ///
    /// From client context (no busyobj yet, e.g. `vcl_recv`/`vcl_hash`): reflects
    /// `req`'s own state directly - [`BodyState::Cached`] after
    /// `std.cache_req_body()`, otherwise whatever the live, not-yet-consumed
    /// client body's state is.
    pub fn req_body_state(&self) -> VclResult<BodyState> {
        if let Some(bo) = unsafe { self.raw.bo.as_ref() } {
            if !bo.req.is_null() {
                let state = BodyState::from_raw(unsafe { (*bo.req).req_body_status });
                // mirrors V1F_SendReq's `AZ(bo->req)` in its `bo->bereq_body != NULL`
                // branch: a live `bo.req` and an already-cached `bo.bereq_body` are
                // mutually exclusive by the time a backend runs.
                assert!(
                    bo.bereq_body.is_null(),
                    "bo.req and bo.bereq_body are both set"
                );
                return Ok(state);
            }
            return Ok(if bo.bereq_body.is_null() {
                BodyState::None
            } else {
                BodyState::Cached
            });
        }
        if let Some(req) = self.req.as_ref() {
            return Ok(BodyState::from_raw(req.raw.req_body_status));
        }
        Err("req.body/bereq.body isn't available in this context".into())
    }

    /// Copy the request body into `writer` — `bereq`'s body from a backend
    /// context, or the client `req`'s body directly if called earlier
    /// (`vcl_recv` and later, before any backend is involved).
    ///
    /// From backend context (busyobj set - typically
    /// [`VclBackend::get_response`](crate::vcl::VclBackend::get_response)):
    /// transparently handles both a body already cached as an object (e.g.
    /// after `std.cache_req_body()`, or on a fetch retry) and a body streamed
    /// live from the client, hiding the underlying `ObjIterate`/`VRB_Iterate`
    /// choice and bookkeeping.
    ///
    /// From client context (no busyobj yet, e.g. `vcl_recv`/`vcl_hash`): reads
    /// `req`'s body directly, same `ObjIterate`/`VRB_Iterate` machinery, minus
    /// the busyobj-specific `no_retry`/`doclose` bookkeeping (there's no fetch
    /// yet to retry or close).
    ///
    /// **Read-once tradeoff**: per Varnish's own rule, an uncached body can be
    /// read exactly once - either by you here, or later by whatever backend
    /// ends up handling this request (a custom [`VclBackend::get_response`],
    /// or a plain upstream backend forwarding it). Read it once in `vcl_recv`
    /// without caching first, and that *later* read fails
    /// (`BodyState::Taken`/an error), not this one. Call `std.cache_req_body()`
    /// before reading if the body needs to survive for a backend (or a retry)
    /// to read too - check first if you're not sure:
    ///
    /// ```
    /// # mod varnish { pub use varnish_sys::vcl; }
    /// # use varnish::vcl::{Ctx, BodyState};
    /// # fn f(ctx: &mut Ctx) -> Result<(), Box<dyn std::error::Error>> {
    /// match ctx.req_body_state()? {
    ///     BodyState::Cached => {
    ///         // safe: a cached body can be read here and still be read again
    ///         // later (by a backend, or after a retry).
    ///         let mut buf = Vec::new();
    ///         ctx.req_body(&mut buf)?;
    ///     }
    ///     BodyState::None => {
    ///         // no body at all - nothing to read, nothing to worry about.
    ///     }
    ///     BodyState::Length | BodyState::Chunked | BodyState::Eof => {
    ///         // live and not cached: reading now consumes it. Only do this if
    ///         // you're sure no backend/retry downstream also needs it, or call
    ///         // `std.cache_req_body()` first if they might.
    ///     }
    ///     BodyState::Taken | BodyState::Error => {
    ///         // already gone (consumed elsewhere) or failed - nothing left to read.
    ///     }
    /// }
    /// # Ok(()) }
    /// ```
    ///
    /// To consume the body without keeping it, pass [`std::io::sink()`] as `writer`.
    ///
    /// Returns `Ok(false)` without touching `writer` if there is no body at all.
    /// Returns `Ok(true)` once the full body has been copied into `writer`.
    /// Returns `Err(_)` if called outside both a backend and a client context,
    /// if the body stream itself failed to read, or if `writer` errors (the
    /// `io::Error` is captured and turned into a [`VclError`]). Mirroring
    /// upstream's `V1F_SendReq`, a failure while iterating an already-cached
    /// body is not treated as fatal on its own (only a `writer` error is).
    ///
    /// Side effect (backend context only): if the body isn't already cached,
    /// reading it marks the fetch as non-retryable (`bo.no_retry`), mirroring
    /// `V1F_SendReq` — call `std.cache_req_body()` in `vcl_recv` first if the
    /// backend may need to retry after reading the body.
    pub fn req_body<W: Write>(&mut self, writer: &mut W) -> VclResult<bool> {
        let state = self.req_body_state()?;
        if state == BodyState::None {
            return Ok(false);
        }

        if self.raw.bo.is_null() {
            // client context: no busyobj yet, so none of the backend-only
            // no_retry/doclose/err_code bookkeeping below applies - there's no
            // fetch yet to retry or close. Read the read-once tradeoff warning
            // above before relying on this branch.
            let req = &mut *self
                .req
                .as_mut()
                .ok_or("req.body/bereq.body isn't available in this context")?
                .raw;
            let mut writer_state = BodyWriterState {
                writer,
                error: None,
            };
            let state_ptr: *mut BodyWriterState<W> = &raw mut writer_state;
            let rv = unsafe {
                ffi::VRB_Iterate(
                    req.wrk,
                    req.vsl.as_mut_ptr(),
                    req,
                    Some(write_body_iterate::<W>),
                    state_ptr.cast::<c_void>(),
                )
            };
            return resolve_iterate_result(rv, &mut writer_state, "req.body read error");
        }

        let bo = unsafe { self.raw.bo.as_mut() }
            .ok_or("bereq.body isn't available in this context (not a backend fetch)")?;

        let mut writer_state = BodyWriterState {
            writer,
            error: None,
        };
        let state_ptr: *mut BodyWriterState<W> = &raw mut writer_state;

        if state == BodyState::Cached {
            // a previously-cached bereq.body (e.g. via std.cache_req_body(), or a
            // retried fetch) takes priority, mirroring V1F_SendReq. Its return
            // value is deliberately ignored, same as upstream's `(void)ObjIterate(...)`
            // in V1F_SendReq: it conflates "callback stopped iteration" with an
            // internal storage error, and upstream never treats it as fatal here.
            // Untested: a failing cached-storage iteration isn't easily forced
            // from VCL, so this specific claim rests on reading V1F_SendReq, not
            // on a `.vtc` reproduction.
            unsafe {
                ffi::ObjIterate(
                    bo.wrk,
                    bo.bereq_body,
                    state_ptr.cast::<c_void>(),
                    Some(write_body_iterate::<W>),
                    0,
                );
            }
            if let Some(e) = writer_state.error.take() {
                return Err(e.to_string().into());
            }
            Ok(true)
        } else {
            let rv = unsafe {
                ffi::VRB_Iterate(
                    bo.wrk,
                    bo.vsl.as_mut_ptr(),
                    bo.req,
                    Some(write_body_iterate::<W>),
                    state_ptr.cast::<c_void>(),
                )
            };

            // bookkeeping mirrored from V1F_SendReq: needed regardless of whether
            // the writer itself failed, since it reflects the now-(partially-)
            // consumed client body stream.
            let req = unsafe { &mut *bo.req };
            unsafe {
                if req.req_body_status != ffi::BS_CACHED.as_ptr() {
                    bo.no_retry = c"bereq.body not cached".as_ptr();
                }
                // Only treat this as an upstream RX-body failure if the *writer*
                // didn't cause it: mirroring vrb_pull, any early `func()` failure
                // (including ours, from a `writer` error) also leaves the C side's
                // `req_body_status` as `BS_ERROR` unless it happened to coincide
                // with the last chunk read off the wire — so without this guard, a
                // writer error on a multi-chunk body would incorrectly get flagged
                // as a client-stream failure (`doclose`/400) here, even though the
                // writer's own `Err` below already reports it correctly.
                if writer_state.error.is_none() && req.req_body_status == ffi::BS_ERROR.as_ptr() {
                    req.doclose = sc_to_ptr(StreamClose::RxBody);
                    bo.err_code = 400;
                }
            }

            resolve_iterate_result(rv, &mut writer_state, "bereq.body (streamed) read error")
        }
    }

    /// Return a shared reference to the client request object, if present.
    ///
    /// Returns `None` outside of client-facing VCL contexts (e.g. in backend subroutines).
    pub fn req(&self) -> Option<&Req<'_>> {
        self.req.as_ref()
    }

    /// Return a mutable reference to the client request object, if present.
    ///
    /// Returns `None` outside of client-facing VCL contexts (e.g. in backend subroutines).
    pub fn req_mut(&mut self) -> Option<&mut Req<'a>> {
        self.req.as_mut()
    }
}

/// Rust proxy for the C `req` struct.
/// Its methods provide getters and setters for various fields that control how the client request
/// is processed by Varnish.
#[derive(Debug)]
pub struct Req<'a> {
    raw: &'a mut ffi::req,
}

impl Req<'_> {
    /// Wrap a raw pointer into an object we can use.
    pub(crate) unsafe fn from_ptr(p: *mut ffi::req) -> Option<Self> {
        Some(Req { raw: p.as_mut()? })
    }

    /// Return whether this request bypasses the cache lookup and is always treated as a miss.
    ///
    /// Equivalent to `req.hash_always_miss` in VCL.
    pub fn hash_always_miss(&self) -> bool {
        self.raw.hash_always_miss() == 1
    }

    /// Force this request to be treated as a cache miss, skipping any existing cached object.
    ///
    /// Equivalent to setting `req.hash_always_miss` in VCL.
    pub fn set_hash_always_miss(&mut self, val: bool) {
        self.raw.set_hash_always_miss(val.into());
    }

    /// Return whether this request ignores busy (locked) cache objects and fetches from the backend instead of waiting.
    ///
    /// Equivalent to `req.hash_ignore_busy` in VCL.
    pub fn hash_ignore_busy(&self) -> bool {
        self.raw.hash_ignore_busy() == 1
    }

    /// Make this request skip waiting on busy cache objects and go straight to the backend.
    ///
    /// Equivalent to setting `req.hash_ignore_busy` in VCL.
    pub fn set_hash_ignore_busy(&mut self, val: bool) {
        self.raw.set_hash_ignore_busy(val.into());
    }

    /// Return whether this request ignores `Vary` headers during cache lookup.
    ///
    /// Equivalent to `req.hash_ignore_vary` in VCL.
    pub fn hash_ignore_vary(&self) -> bool {
        self.raw.hash_ignore_vary() == 1
    }

    /// Make this request ignore `Vary` headers during cache lookup, collapsing all variants into one cache key.
    ///
    /// Equivalent to setting `req.hash_ignore_vary` in VCL.
    pub fn set_hash_ignore_vary(&mut self, val: bool) {
        self.raw.set_hash_ignore_vary(val.into());
    }
}

/// A struct holding both a native [`vrt_ctx`] struct and the space it points to.
///
/// As the name implies, this struct mainly exist to facilitate testing and should probably not be
/// used elsewhere.
#[derive(Debug)]
pub struct TestCtx {
    vrt_ctx: vrt_ctx,
    test_ws: TestWS,
}

impl TestCtx {
    /// Instantiate a [`vrt_ctx`], as well as the workspace (of size `sz`) it links to.
    pub fn new(sz: usize) -> Self {
        let mut test_ctx = Self {
            vrt_ctx: vrt_ctx {
                magic: VRT_CTX_MAGIC,
                ..vrt_ctx::default()
            },
            test_ws: TestWS::new(sz),
        };
        test_ctx.vrt_ctx.ws = test_ctx.test_ws.as_ptr();
        test_ctx
    }

    /// Return a [`Ctx`] wrapping this test context, for use in unit tests.
    pub fn ctx(&mut self) -> Ctx<'_> {
        Ctx::from_ref(&mut self.vrt_ctx)
    }
}

/// Log a message outside of a request context using a VSL tag.
///
/// Useful in event handlers or other places where no [`Ctx`] is available.
pub fn log(tag: LogTag, msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    unsafe {
        let vxids = ffi::vxids::default();
        ffi::VSL(tag, vxids, c"%.*s".as_ptr(), msg.len(), msg.as_ptr());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctx_test() {
        let mut test_ctx = TestCtx::new(100);
        test_ctx.ctx();
    }
}

/// This is an unsafe struct that holds the per-VCL state.
/// It must be public because it is used by the macro-generated code.
#[doc(hidden)]
#[derive(Debug)]
pub struct PerVclState<T> {
    #[expect(clippy::vec_box)] // FIXME: we may want to rethink this
    pub fetch_filters: Vec<Box<ffi::vfp>>,
    #[expect(clippy::vec_box)] // FIXME: we may want to rethink this
    pub delivery_filters: Vec<Box<ffi::vdp>>,
    pub user_data: Option<Box<T>>,
}

// Implement the default trait that works even when `T` does not impl `Default`.
impl<T> Default for PerVclState<T> {
    fn default() -> Self {
        Self {
            fetch_filters: Vec::default(),
            delivery_filters: Vec::default(),
            user_data: None,
        }
    }
}

impl<T> PerVclState<T> {
    pub fn get_user_data(&self) -> Option<&T> {
        self.user_data.as_ref().map(AsRef::as_ref)
    }
}
