//! Expose the Varnish context [`vrt_ctx`] as a Rust object
//!
#[cfg(not(varnishsys_6))]
use std::ffi::{c_int, c_uint, c_void};

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
    pub ws: Workspace<'a>,
}

impl<'a> Ctx<'a> {
    /// Wrap a raw pointer into an object we can use.
    ///
    /// The pointer must be non-null, and the magic must match
    pub unsafe fn from_ptr(ptr: *const vrt_ctx) -> Self {
        Self::from_ref(ptr.cast_mut().as_mut().unwrap())
    }

    /// Instantiate from a mutable reference to a [`vrt_ctx`].
    #[cfg_attr(not(varnishsys_6), expect(clippy::useless_conversion))] // Varnish v6 has a different struct, requiring .into()
    pub fn from_ref(raw: &'a mut vrt_ctx) -> Self {
        assert_eq!(raw.magic, VRT_CTX_MAGIC);
        Self {
            http_req: HttpHeaders::from_ptr(raw.http_req.into()),
            http_req_top: HttpHeaders::from_ptr(raw.http_req_top.into()),
            http_resp: HttpHeaders::from_ptr(raw.http_resp.into()),
            http_bereq: HttpHeaders::from_ptr(raw.http_bereq.into()),
            http_beresp: HttpHeaders::from_ptr(raw.http_beresp.into()),
            ws: Workspace::from_ptr(raw.ws),
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
    #[cfg(not(varnishsys_6))]
    pub fn cached_req_body(&mut self) -> Result<Vec<&'a [u8]>, VclError> {
        unsafe extern "C" fn chunk_collector(
            priv_: *mut c_void,
            _flush: c_uint,
            ptr: *const c_void,
            len: isize,
        ) -> c_int {
            let v = priv_.cast::<Vec<&[u8]>>().as_mut().unwrap();
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
    #[cfg(not(varnishsys_6))]
    unsafe {
        let vxids = ffi::vxids::default();
        ffi::VSL(tag, vxids, c"%.*s".as_ptr(), msg.len(), msg.as_ptr());
    }
    #[cfg(varnishsys_6)]
    unsafe {
        ffi::VSL(tag, 0, c"%.*s".as_ptr(), msg.len(), msg.as_ptr());
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
    #[cfg(not(varnishsys_6))]
    #[expect(clippy::vec_box)] // FIXME: we may want to rethink this
    pub fetch_filters: Vec<Box<ffi::vfp>>,
    #[cfg(not(varnishsys_6))]
    #[expect(clippy::vec_box)] // FIXME: we may want to rethink this
    pub delivery_filters: Vec<Box<ffi::vdp>>,
    pub user_data: Option<Box<T>>,
}

// Implement the default trait that works even when `T` does not impl `Default`.
impl<T> Default for PerVclState<T> {
    fn default() -> Self {
        Self {
            #[cfg(not(varnishsys_6))]
            fetch_filters: Vec::default(),
            #[cfg(not(varnishsys_6))]
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
