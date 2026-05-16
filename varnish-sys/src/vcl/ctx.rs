//! Expose the Varnish context [`vrt_ctx`] as a Rust object
//!
use std::ffi::{c_int, c_uint, c_void, CStr};

use crate::ffi;
use crate::ffi::{vrt_ctx, VRT_fail, VRT_CTX_MAGIC};
use crate::vcl::{HttpHeaders, LogTag, TestWS, VclError, Workspace};

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
    pub req: Option<Req<'a>>,
    pub ws: Workspace<'a>,
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
            req: Req::from_ptr(raw.req),
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

        let req = unsafe { self.raw.req.as_mut().ok_or("req object isn't available")? };
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
    pub(crate) fn from_ptr(p: *mut ffi::req) -> Option<Self> {
        Some(Req {
            raw: unsafe { p.as_mut()? },
        })
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

    pub fn ctx(&mut self) -> Ctx<'_> {
        Ctx::from_ref(&mut self.vrt_ctx)
    }
}

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
