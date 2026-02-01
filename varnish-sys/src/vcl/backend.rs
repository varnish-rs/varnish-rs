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
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::marker::PhantomData;
use std::mem::size_of;
use std::net::{SocketAddr, TcpStream};
use std::os::unix::io::FromRawFd;
use std::ptr;
use std::ptr::{null, null_mut};
use std::time::SystemTime;

use crate::ffi::{VclEvent, VfpStatus, VCL_BACKEND, VCL_BOOL, VCL_IP, VCL_TIME};
use crate::utils::get_backend;
use crate::vcl::{Buffer, Ctx, IntoVCL, LogTag, VclError, VclResult, Workspace};
use crate::{
    ffi, validate_director, validate_vdir, validate_vfp_ctx, validate_vfp_entry, validate_vrt_ctx,
};

/// Fat wrapper around [`VCL_BACKEND`].
///
/// It will handle almost all the necessary boilerplate needed to create a vmod. Most importantly,
/// it destroys/unregisters the backend as part of it's `Drop` implementation, and
/// will convert the C methods to something more idiomatic.
///
/// Once created, a [`Backend`]'s sole purpose is to exist as a C reference for the VCL. As a
/// result, you don't want to drop it until after all the transfers are done. The most common way
/// is just to have the backend be part of a vmod object because the object won't be dropped until
/// the VCL is discarded and that can only happen once all the backend fetches are done.
#[derive(Debug)]
pub struct Backend<S: VclBackend<T>, T: VclResponse> {
    #[expect(dead_code)]
    methods: Box<ffi::vdi_methods>,
    inner: Box<S>,
    #[expect(dead_code)]
    ctype: CString,
    phantom: PhantomData<T>,
    backend_ref: BackendRef,
}

impl<S: VclBackend<T>, T: VclResponse> Backend<S, T> {
    /// Access the inner type wrapped by [Backend]. Note that it isn't `mut` as other threads are
    /// likely to have access to it too.
    pub fn get_inner(&self) -> &S {
        &self.inner
    }

    /// Create a new builder, wrapping the `inner` structure (that implements [`VclBackend`]),
    /// calling the backend `backend_id`. If the backend has a probe attached to it, set `has_probe` to
    /// true.
    pub fn new(
        ctx: &mut Ctx,
        backend_type: &str,
        backend_id: &str,
        be: S,
        has_probe: bool,
    ) -> VclResult<Self> {
        let mut inner = Box::new(be);
        let ctype: CString = CString::new(backend_type).map_err(|e| e.to_string())?;
        let cname: CString = CString::new(backend_id).map_err(|e| e.to_string())?;
        let methods = Box::new(ffi::vdi_methods {
            type_: ctype.as_ptr(),
            magic: ffi::VDI_METHODS_MAGIC,
            destroy: None,
            event: Some(wrap_event::<S, T>),
            finish: Some(wrap_finish::<S, T>),
            gethdrs: Some(wrap_gethdrs::<S, T>),
            getip: Some(wrap_getip::<T>),
            healthy: has_probe.then_some(wrap_healthy::<S, T>),
            http1pipe: Some(wrap_pipe::<S, T>),
            list: Some(wrap_list::<S, T>),
            panic: Some(wrap_panic::<S, T>),
            resolve: None,
            release: None,
        });

        let bep = unsafe {
            ffi::VRT_AddDirector(
                ctx.raw,
                &raw const *methods,
                ptr::from_mut::<S>(&mut *inner).cast::<c_void>(),
                c"%.*s".as_ptr(),
                cname.as_bytes().len(),
                cname.as_ptr().cast::<c_char>(),
            )
        };
        if bep.0.is_null() {
            return Err(format!("VRT_AddDirector return null while creating {backend_id}").into());
        }

        let backend_ref = unsafe { BackendRef::new_withouth_refcount(bep).expect("Backend pointer should never be null") };

        Ok(Backend {
            ctype,
            inner,
            methods,
            phantom: PhantomData,
            backend_ref,
        })
    }
}

/// The trait to implement to "be" a backend
///
/// [`VclBackend`] maps to the `vdi_methods` structure of the C api, but presented in a more
/// "rusty" form. Apart from [`VclBackend::get_response`] all methods are optional.
///
/// If your backend doesn't return any content body, you can implement `VclBackend<()>` as `()` has a default
/// [`VclResponse`] implementation.
pub trait VclBackend<T: VclResponse> {
    /// If the VCL pick this backend (or a director ended up choosing it), this method gets called
    /// so that the [`VclBackend`] implementer can:
    /// - inspect the request headers (`ctx.http_bereq`)
    /// - fill the response headers (`ctx.http_beresp`)
    /// - possibly return a [`VclResponse`] object that will generate the response body
    ///
    /// If this function returns a `Ok(_)` without having set the method and protocol of
    /// `ctx.http_beresp`, we'll default to `HTTP/1.1 200 OK`
    fn get_response(&self, _ctx: &mut Ctx) -> Result<Option<T>, VclError>;

    /// Once a backend transaction is finished, the [`Backend`] has a chance to clean up, collect
    /// data and others in the finish methods.
    fn finish(&self, _ctx: &mut Ctx) {}

    /// Is your backend healthy, and when did its health change for the last time.
    fn probe(&self, _ctx: &mut Ctx) -> (bool, SystemTime) {
        (true, SystemTime::UNIX_EPOCH)
    }

    /// If your backend is used inside `vcl_pipe`, this method is in charge of sending the request
    /// headers that Varnish already read, and then the body. The second argument, a `TcpStream` is
    /// the raw client stream that Varnish was using (converted from a raw fd).
    ///
    /// Once done, you should return a `StreamClose` describing how/why the transaction ended.
    fn pipe(&self, ctx: &mut Ctx, _tcp_stream: TcpStream) -> StreamClose {
        ctx.log(LogTag::Error, "Backend does not support pipe");
        StreamClose::TxError
    }

    /// The method will get called when the VCL changes temperature or is discarded. It's notably a
    /// chance to start/stop probes to consume fewer resources.
    fn event(&self, _event: VclEvent) {}

    fn panic(&self, _vsb: &mut Buffer) {}

    /// Generate simple report output for `varnishadm backend.list` (no flags)
    ///
    /// Corresponds to the `list` callback in `vdi_methods` when neither `-p` nor `-j` is passed.
    fn report(&self, ctx: &mut Ctx, vsb: &mut Buffer) {
        let state = if self.probe(ctx).0 {
            "healthy"
        } else {
            "sick"
        };
        vsb.write(&"0/0\t").unwrap();
        vsb.write(&state).unwrap();
    }

    /// Generate detailed report output for `varnishadm backend.list -p`
    ///
    /// Corresponds to the `list` callback in `vdi_methods` when `-p` is passed.
    fn report_details(&self, _ctx: &mut Ctx, _vsb: &mut Buffer) {}

    /// Generate simple JSON report output for `varnishadm backend.list -j`
    ///
    /// Corresponds to the `list` callback in `vdi_methods` when `-j` is passed.
    fn report_json(&self, ctx: &mut Ctx, vsb: &mut Buffer) {
        let state = if self.probe(ctx).0 {
            "healthy"
        } else {
            "sick"
        };
        vsb.write(&"[0, 0, ").unwrap();
        vsb.write(&state).unwrap();
        vsb.write(&"]").unwrap();
    }

    /// Generate detailed JSON report output for `varnishadm backend.list -j -p`
    ///
    /// Corresponds to the `list` callback in `vdi_methods` when both `-j` and `-p` are passed.
    fn report_details_json(&self, _ctx: &mut Ctx, vsb: &mut Buffer) {
        let _ = vsb.write(&"{}");
    }
}

/// An in-flight response body
///
/// When [`VclBackend::get_response`] get called, the backend [`Backend`] can return a
/// `Result<Option<VclResponse>>`:
/// - `Err(_)`: something went wrong, the error will be logged and synthetic backend response will be
///   generated by Varnish
/// - `Ok(None)`: headers are set, but the response as no content body.
/// - `Ok(Some(VclResponse))`: headers are set, and Varnish will use the [`VclResponse`] object to build
///   the response body.
#[expect(clippy::len_without_is_empty)] // FIXME: should there be an is_empty() method?
pub trait VclResponse {
    /// The only mandatory method, it will be called repeated so that the [`VclResponse`] object can
    /// fill `buf`. The transfer will stop if any of its calls returns an error, and it will
    /// complete successfully when `Ok(0)` is returned.
    ///
    /// `.read()` will never be called on an empty buffer, and the implementer must return the
    /// number of bytes written (which therefore must be less than the buffer size).
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VclError>;

    /// If returning `Some(_)`, we know the size of the body generated, and it'll be used to fill the
    /// `content-length` header of the response. Otherwise, chunked encoding will be used, which is
    /// what's assumed by default.
    fn len(&self) -> Option<usize> {
        None
    }

    /// Potentially return the IP:port pair that the backend is using to transfer the body. It
    /// might not make sense for your implementation.
    fn get_ip(&self) -> Result<Option<SocketAddr>, VclError> {
        Ok(None)
    }
}

impl VclResponse for () {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, VclError> {
        Ok(0)
    }
}

unsafe extern "C" fn vfp_pull<T: VclResponse>(
    ctxp: *mut ffi::vfp_ctx,
    vfep: *mut ffi::vfp_entry,
    ptr: *mut c_void,
    len: *mut isize,
) -> VfpStatus {
    let ctx = validate_vfp_ctx(ctxp);
    let vfe = validate_vfp_entry(vfep);

    let buf = std::slice::from_raw_parts_mut(ptr.cast::<u8>(), *len as usize);
    if buf.is_empty() {
        *len = 0;
        return VfpStatus::Ok;
    }

    let reader = vfe.priv1.cast::<T>().as_mut().unwrap();
    match reader.read(buf) {
        Err(e) => {
            // TODO: we should grow a VSL object
            // SAFETY: we assume ffi::VSLbt() will not store the pointer to the string's content
            let msg = ffi::txt::from_str(e.as_str().as_ref());
            ffi::VSLbt(ctx.req.as_ref().unwrap().vsl, ffi::VslTag::Error, msg);
            VfpStatus::Error
        }
        Ok(0) => {
            *len = 0;
            VfpStatus::End
        }
        Ok(l) => {
            *len = l as isize;
            VfpStatus::Ok
        }
    }
}

unsafe extern "C" fn wrap_event<S: VclBackend<T>, T: VclResponse>(be: VCL_BACKEND, ev: VclEvent) {
    let backend: &S = get_backend(validate_director(be));
    backend.event(ev);
}

unsafe extern "C" fn wrap_list<S: VclBackend<T>, T: VclResponse>(
    ctxp: *const ffi::vrt_ctx,
    be: VCL_BACKEND,
    vsbp: *mut ffi::vsb,
    detailed: i32,
    json: i32,
) {
    let mut ctx = Ctx::from_ptr(ctxp);
    let mut vsb = Buffer::from_ptr(vsbp);
    let backend: &S = get_backend(validate_director(be));
    match (json != 0, detailed != 0) {
        (true, true) => backend.report_details_json(&mut ctx, &mut vsb),
        (true, false) => backend.report_json(&mut ctx, &mut vsb),
        (false, true) => backend.report_details(&mut ctx, &mut vsb),
        (false, false) => backend.report(&mut ctx, &mut vsb),
    }
}

unsafe extern "C" fn wrap_panic<S: VclBackend<T>, T: VclResponse>(
    be: VCL_BACKEND,
    vsbp: *mut ffi::vsb,
) {
    let mut vsb = Buffer::from_ptr(vsbp);
    let backend: &S = get_backend(validate_director(be));
    backend.panic(&mut vsb);
}

unsafe extern "C" fn wrap_pipe<S: VclBackend<T>, T: VclResponse>(
    ctxp: *const ffi::vrt_ctx,
    be: VCL_BACKEND,
) -> ffi::stream_close_t {
    let mut ctx = Ctx::from_ptr(ctxp);
    let req = ctx.raw.validated_req();
    let sp = req.validated_session();
    let fd = sp.fd;
    assert_ne!(fd, 0);
    let tcp_stream = TcpStream::from_raw_fd(fd);

    let backend: &S = get_backend(validate_director(be));
    sc_to_ptr(backend.pipe(&mut ctx, tcp_stream))
}

// CStr is tied to the lifetime of bep, but we only use it for error messages
impl VCL_BACKEND {
    unsafe fn get_type(&self) -> &str {
        CStr::from_ptr(
            self.0
                .as_ref()
                .unwrap()
                .vdir
                .as_ref()
                .unwrap()
                .methods
                .as_ref()
                .unwrap()
                .type_
                .as_ref()
                .unwrap(),
        )
        .to_str()
        .unwrap()
    }
}

#[allow(clippy::too_many_lines)] // fixme
unsafe extern "C" fn wrap_gethdrs<S: VclBackend<T>, T: VclResponse>(
    ctxp: *const ffi::vrt_ctx,
    bep: VCL_BACKEND,
) -> c_int {
    let mut ctx = Ctx::from_ptr(ctxp);
    let be = validate_director(bep);
    let backend: &S = get_backend(be);
    assert!(!be.vcl_name.is_null()); // FIXME: is this validation needed?
    validate_vdir(be); // FIXME: is this validation needed?

    match backend.get_response(&mut ctx) {
        Ok(res) => {
            // default to HTTP/1.1 200 if the backend didn't provide anything
            let beresp = ctx.http_beresp.as_mut().unwrap();
            if beresp.status().is_none() {
                beresp.set_status(200);
            }
            if beresp.proto().is_none() {
                if let Err(e) = beresp.set_proto("HTTP/1.1") {
                    ctx.fail(format!("{:?}: {e}", bep.get_type()));
                    return 1;
                }
            }
            let bo = ctx.raw.bo.as_mut().unwrap();
            let Some(htc) = ffi::WS_Alloc(bo.ws.as_mut_ptr(), size_of::<ffi::http_conn>() as u32)
                .cast::<ffi::http_conn>()
                .as_mut()
            else {
                ctx.fail(format!("{}: insufficient workspace", bep.get_type()));
                return -1;
            };
            htc.magic = ffi::HTTP_CONN_MAGIC;
            htc.doclose = &raw const ffi::SC_REM_CLOSE[0];
            htc.content_length = 0;
            match res {
                None => {
                    htc.body_status = ffi::BS_NONE.as_ptr();
                }
                Some(transfer) => {
                    match transfer.len() {
                        None => {
                            htc.body_status = ffi::BS_CHUNKED.as_ptr();
                            htc.content_length = -1;
                        }
                        Some(0) => {
                            htc.body_status = ffi::BS_NONE.as_ptr();
                        }
                        Some(l) => {
                            htc.body_status = ffi::BS_LENGTH.as_ptr();
                            htc.content_length = l as isize;
                        }
                    }
                    htc.priv_ = Box::into_raw(Box::new(transfer)).cast::<c_void>();
                    // build a vfp to wrap the VclResponse object if there's something to push
                    if htc.body_status != ffi::BS_NONE.as_ptr() {
                        let Some(vfp) =
                            ffi::WS_Alloc(bo.ws.as_mut_ptr(), size_of::<ffi::vfp>() as u32)
                                .cast::<ffi::vfp>()
                                .as_mut()
                        else {
                            ctx.fail(format!("{}: insufficient workspace", bep.get_type()));
                            return -1;
                        };
                        let Ok(t) = Workspace::from_ptr(bo.ws.as_mut_ptr())
                            .copy_bytes_with_null(bep.get_type())
                        else {
                            ctx.fail(format!("{}: insufficient workspace", bep.get_type()));
                            return -1;
                        };

                        vfp.name = t.b;
                        vfp.init = None;
                        vfp.pull = Some(vfp_pull::<T>);
                        vfp.fini = None;
                        vfp.priv1 = null();

                        let Some(vfe) = ffi::VFP_Push(bo.vfc, vfp).as_mut() else {
                            ctx.fail(format!("{}: couldn't insert vfp", bep.get_type()));
                            return -1;
                        };
                        // we don't need to clean vfe.priv1 at the vfp level, the backend will
                        // do it in wrap_finish
                        vfe.priv1 = htc.priv_;
                    }
                }
            }

            bo.htc = htc;
            0
        }
        Err(s) => {
            let typ = bep.get_type();
            ctx.log(LogTag::FetchError, format!("{typ}: {s}"));
            1
        }
    }
}

unsafe extern "C" fn wrap_healthy<S: VclBackend<T>, T: VclResponse>(
    ctxp: *const ffi::vrt_ctx,
    be: VCL_BACKEND,
    changed: *mut VCL_TIME,
) -> VCL_BOOL {
    let backend: &S = get_backend(validate_director(be));

    let mut ctx = Ctx::from_ptr(ctxp);
    let (healthy, when) = backend.probe(&mut ctx);
    if !changed.is_null() {
        *changed = when.try_into().unwrap(); // FIXME: on error?
    }
    healthy.into()
}

unsafe extern "C" fn wrap_getip<T: VclResponse>(
    ctxp: *const ffi::vrt_ctx,
    _be: VCL_BACKEND,
) -> VCL_IP {
    let ctxp = validate_vrt_ctx(ctxp);
    let bo = ctxp.bo.as_ref().unwrap();
    assert_eq!(bo.magic, ffi::BUSYOBJ_MAGIC);
    let htc = bo.htc.as_ref().unwrap();
    // FIXME: document why htc does not use a different magic number
    assert_eq!(htc.magic, ffi::BUSYOBJ_MAGIC);
    let transfer = htc.priv_.cast::<T>().as_ref().unwrap();

    let mut ctx = Ctx::from_ptr(ctxp);

    transfer
        .get_ip()
        .and_then(|ip| match ip {
            Some(ip) => Ok(ip.into_vcl(&mut ctx.ws)?),
            None => Ok(VCL_IP(null())),
        })
        .unwrap_or_else(|e| {
            ctx.fail(format!("{e}"));
            VCL_IP(null())
        })
}

unsafe extern "C" fn wrap_finish<S: VclBackend<T>, T: VclResponse>(
    ctxp: *const ffi::vrt_ctx,
    be: VCL_BACKEND,
) {
    let prev_backend: &S = get_backend(validate_director(be));

    // FIXME: shouldn't the ctx magic number be checked? If so, use validate_vrt_ctx()
    let ctx = ctxp.as_ref().unwrap();
    let bo = ctx.bo.as_mut().unwrap();

    // drop the VclResponse
    if let Some(htc) = ptr::replace(&raw mut bo.htc, null_mut()).as_mut() {
        if let Some(val) = ptr::replace(&raw mut htc.priv_, null_mut())
            .cast::<T>()
            .as_mut()
        {
            drop(Box::from_raw(val));
        }
    }

    // FIXME?: should _prev be set to NULL?
    prev_backend.finish(&mut Ctx::from_ptr(ctx));
}

impl<S: VclBackend<T>, T: VclResponse> Drop for Backend<S, T> {
    fn drop(&mut self) {
        unsafe {
            let mut bep = self.backend_ref.vcl_ptr();
            ffi::VRT_DelDirector(&raw mut bep);
        };
    }
}

impl<S: VclBackend<T>, T: VclResponse> AsRef<BackendRef> for Backend<S, T> {
    fn as_ref(&self) -> &BackendRef {
        &self.backend_ref
    }
}

/// Return type for [`VclBackend::pipe`]
///
/// When piping a response, the backend is in charge of closing the file descriptor (which is done
/// automatically by the rust layer), but also to provide how/why it got closed.
#[derive(Debug, Clone, Copy)]
pub enum StreamClose {
    RemClose,
    ReqClose,
    ReqHttp10,
    RxBad,
    RxBody,
    RxJunk,
    RxOverflow,
    RxTimeout,
    RxCloseIdle,
    TxPipe,
    TxError,
    TxEof,
    RespClose,
    Overload,
    PipeOverflow,
    RangeShort,
    ReqHttp20,
    VclFailure,
}

fn sc_to_ptr(sc: StreamClose) -> ffi::stream_close_t {
    unsafe {
        match sc {
            StreamClose::RemClose => ffi::SC_REM_CLOSE.as_ptr(),
            StreamClose::ReqClose => ffi::SC_REQ_CLOSE.as_ptr(),
            StreamClose::ReqHttp10 => ffi::SC_REQ_HTTP10.as_ptr(),
            StreamClose::RxBad => ffi::SC_RX_BAD.as_ptr(),
            StreamClose::RxBody => ffi::SC_RX_BODY.as_ptr(),
            StreamClose::RxJunk => ffi::SC_RX_JUNK.as_ptr(),
            StreamClose::RxOverflow => ffi::SC_RX_OVERFLOW.as_ptr(),
            StreamClose::RxTimeout => ffi::SC_RX_TIMEOUT.as_ptr(),
            StreamClose::RxCloseIdle => ffi::SC_RX_CLOSE_IDLE.as_ptr(),
            StreamClose::TxPipe => ffi::SC_TX_PIPE.as_ptr(),
            StreamClose::TxError => ffi::SC_TX_ERROR.as_ptr(),
            StreamClose::TxEof => ffi::SC_TX_EOF.as_ptr(),
            StreamClose::RespClose => ffi::SC_RESP_CLOSE.as_ptr(),
            StreamClose::Overload => ffi::SC_OVERLOAD.as_ptr(),
            StreamClose::PipeOverflow => ffi::SC_PIPE_OVERFLOW.as_ptr(),
            StreamClose::RangeShort => ffi::SC_RANGE_SHORT.as_ptr(),
            StreamClose::ReqHttp20 => ffi::SC_REQ_HTTP20.as_ptr(),
            StreamClose::VclFailure => ffi::SC_VCL_FAILURE.as_ptr(),
        }
    }
}

/// Result from a probe health check
///
/// Contains both the health status and when it last changed.
#[derive(Debug, Clone, Copy)]
pub struct ProbeResult {
    pub healthy: bool,
    pub last_changed: SystemTime,
}

/// Trait for wrapping a C `struct director`
///
/// This trait provides a safe interface to interact with Varnish directors,
/// which are responsible for selecting backends. Directors receive requests
/// and decide which backend should handle them, implementing load balancing
/// and health checking strategies.
///
/// The trait methods map to the C `vdi_methods` structure function pointers.
pub trait VclDirector {
    /// Resolve the director to a concrete backend
    ///
    /// This is called when Varnish needs to select a backend to handle a request.
    /// The director should inspect the context (request headers, etc.) and return
    /// the appropriate backend, or `None` if no backend is available.
    ///
    /// Corresponds to the `resolve` callback in `vdi_methods`.
    fn resolve(&self, ctx: &mut Ctx) -> Option<BackendRef>;

    /// Check if the director (or its backends) are healthy
    ///
    /// Returns a `ProbeResult` containing the health status and when it last changed.
    ///
    /// Corresponds to the `healthy` callback in `vdi_methods`.
    fn probe(&self, ctx: &mut Ctx) -> ProbeResult;

    /// Generate simple report output for `varnishadm backend.list` (no flags)
    ///
    /// Corresponds to the `list` callback in `vdi_methods` when neither `-p` nor `-j` is passed.
    fn report(&self, _ctx: &mut Ctx, _vsb: &mut Buffer) {}

    /// Generate detailed report output for `varnishadm backend.list -p`
    ///
    /// Corresponds to the `list` callback in `vdi_methods` when `-p` is passed.
    fn report_details(&self, _ctx: &mut Ctx, _vsb: &mut Buffer) {}

    /// Generate simple JSON report output for `varnishadm backend.list -j`
    ///
    /// Corresponds to the `list` callback in `vdi_methods` when `-j` is passed.
    fn report_json(&self, _ctx: &mut Ctx, vsb: &mut Buffer) {
        let _ = vsb.write(&"{}");
    }

    /// Generate detailed JSON report output for `varnishadm backend.list -j -p`
    ///
    /// Corresponds to the `list` callback in `vdi_methods` when both `-j` and `-p` are passed.
    fn report_details_json(&self, _ctx: &mut Ctx, vsb: &mut Buffer) {
        let _ = vsb.write(&"{}");
    }

    /// Called when the director is being destroyed
    ///
    /// Use this to clean up any resources like backend references.
    /// Corresponds to the `release` callback in `vdi_methods`.
    fn release(&self) {}

    /// Called when the VCL temperature changes or is discarded
    ///
    /// Corresponds to the `event` callback in `vdi_methods`.
    fn event(&self, event: VclEvent) {
        let _ = event;
    }
}

unsafe extern "C" fn wrap_director_resolve<D: VclDirector>(
    ctxp: *const ffi::vrt_ctx,
    director: VCL_BACKEND,
) -> VCL_BACKEND {
    let mut ctx = Ctx::from_ptr(ctxp);
    let dir = validate_director(director);
    let dir_impl: &D = &*dir.priv_.cast::<D>();
    dir_impl
        .resolve(&mut ctx)
        .map_or(VCL_BACKEND(null()), |backend_ref| backend_ref.vcl_ptr())
}

unsafe extern "C" fn wrap_director_healthy<D: VclDirector>(
    ctxp: *const ffi::vrt_ctx,
    director: VCL_BACKEND,
    changed: *mut VCL_TIME,
) -> VCL_BOOL {
    let mut ctx = Ctx::from_ptr(ctxp);
    let dir = validate_director(director);
    let dir_impl: &D = &*dir.priv_.cast::<D>();
    let result = dir_impl.probe(&mut ctx);
    if !changed.is_null() {
        *changed = result.last_changed.try_into().unwrap();
    }
    result.healthy.into()
}

unsafe extern "C" fn wrap_director_list<D: VclDirector>(
    ctxp: *const ffi::vrt_ctx,
    director: VCL_BACKEND,
    vsbp: *mut ffi::vsb,
    detailed: i32,
    json: i32,
) {
    let mut ctx = Ctx::from_ptr(ctxp);
    let mut vsb = Buffer::from_ptr(vsbp);
    let dir = validate_director(director);
    let dir_impl: &D = &*dir.priv_.cast::<D>();
    match (json != 0, detailed != 0) {
        (true, true) => dir_impl.report_details_json(&mut ctx, &mut vsb),
        (true, false) => dir_impl.report_json(&mut ctx, &mut vsb),
        (false, true) => dir_impl.report_details(&mut ctx, &mut vsb),
        (false, false) => dir_impl.report(&mut ctx, &mut vsb),
    }
}

unsafe extern "C" fn wrap_director_event<D: VclDirector>(director: VCL_BACKEND, ev: VclEvent) {
    let dir = validate_director(director);
    let dir_impl: &D = &*dir.priv_.cast::<D>();
    dir_impl.event(ev);
}

unsafe extern "C" fn wrap_director_release<D: VclDirector>(director: VCL_BACKEND) {
    let dir = validate_director(director);
    let dir_impl: &D = &*dir.priv_.cast::<D>();
    dir_impl.release();
}

/// Safe wrapper around a `struct director` pointer with a trait implementation
///
/// This struct wraps a C director along with a Rust implementation that provides
/// the director's behavior through the [`VclDirector`] trait. The wrapper handles
/// the FFI boundary and ensures proper lifetime management.
///
/// Directors are typically used to implement load balancing strategies (round-robin,
/// random, hash-based, etc.) by selecting from multiple backends.
#[derive(Debug)]
pub struct Director<D: VclDirector> {
    #[expect(dead_code)]
    methods: Box<ffi::vdi_methods>,
    inner: Box<D>,
    #[expect(dead_code)]
    ctype: CString,
    backend_ref: BackendRef,
}

impl<D: VclDirector> Director<D> {
    /// Create a new director by calling `VRT_AddDirector`
    ///
    /// This registers the director with Varnish and sets up the appropriate callbacks.
    /// The director will be automatically unregistered when dropped.
    pub fn new(ctx: &mut Ctx, director_type: &str, vcl_name: &str, inner: D) -> VclResult<Self> {
        let mut inner = Box::new(inner);
        let ctype = CString::new(director_type).map_err(|e| e.to_string())?;
        let cname = CString::new(vcl_name).map_err(|e| e.to_string())?;
        let methods = Box::new(ffi::vdi_methods {
            type_: ctype.as_ptr(),
            magic: ffi::VDI_METHODS_MAGIC,
            http1pipe: None,
            healthy: Some(wrap_director_healthy::<D>),
            resolve: Some(wrap_director_resolve::<D>),
            gethdrs: None,
            getip: None,
            finish: None,
            event: Some(wrap_director_event::<D>),
            release: Some(wrap_director_release::<D>),
            destroy: None,
            panic: None,
            list: Some(wrap_director_list::<D>),
        });

        let bep = unsafe {
            ffi::VRT_AddDirector(
                ctx.raw,
                &raw const *methods,
                ptr::from_mut::<D>(&mut *inner).cast::<c_void>(),
                c"%.*s".as_ptr(),
                cname.as_bytes().len(),
                cname.as_ptr().cast::<c_char>(),
            )
        };
        if bep.0.is_null() {
            return Err(format!("VRT_AddDirector returned null while creating {vcl_name}").into());
        }

        unsafe {
            assert_eq!((*bep.0).magic, ffi::DIRECTOR_MAGIC);
        }

        let backend_ref = unsafe { BackendRef::new_withouth_refcount(bep).expect("Backend pointer should never be null") };

        Ok(Director {
            methods,
            inner,
            ctype,
            backend_ref,
        })
    }

    /// Access the bep director implementation
    pub fn get_inner(&self) -> &D {
        &self.inner
    }

    /// Access the bep director implementation mutably
    pub fn get_inner_mut(&mut self) -> &mut D {
        &mut self.inner
    }

    /// Resolve this director to a backend using `VRT_DirectorResolve`
    ///
    /// This calls into Varnish's resolution mechanism, which will invoke
    /// the director's `resolve` method if needed.
    pub fn resolve(&self, ctx: &Ctx) -> VCL_BACKEND {
        unsafe { ffi::VRT_DirectorResolve(ctx.raw, self.backend_ref.vcl_ptr()) }
    }

    /// Check if this director is healthy using `VRT_Healthy`
    pub fn probe(&self, ctx: &Ctx) -> ProbeResult {
        self.backend_ref.probe(ctx)
    }
}

impl<D: VclDirector> Drop for Director<D> {
    fn drop(&mut self) {
        unsafe {
            let mut bep = self.backend_ref.vcl_ptr();
            ffi::VRT_DelDirector(&raw mut bep);
        };
    }
}

impl<D: VclDirector> AsRef<BackendRef> for Director<D> {
    fn as_ref(&self) -> &BackendRef {
        &self.backend_ref
    }
}

/// A native Varnish backend created via `VRT_new_backend()`
///
/// This wraps a regular Varnish backend (the kind you'd normally define in VCL)
/// but created dynamically from Rust code. Unlike [`Backend`], which allows you
/// to implement custom backend logic, `NativeBackend` creates a standard HTTP/1
/// backend that connects to a real server.
///
/// Use [`NativeBackendBuilder`] to construct instances with a fluent API.
///
/// # Example
///
/// ```ignore
/// let backend = NativeBackend::builder()
///     .host("example.com", 80)
///     .vcl_name("my_backend")
///     .connect_timeout(Duration::from_secs(5))
///     .build(ctx)?;
///
/// let backend_ref = backend.backend_ref();
/// ```
#[derive(Debug)]
pub struct NativeBackend {
    // Keep C structures alive for the backend's lifetime
    #[expect(dead_code)]
    endpoint: Box<ffi::vrt_endpoint>,
    #[expect(dead_code)]
    backend_config: Box<ffi::vrt_backend>,
    backend_ref: BackendRef,
}

impl NativeBackend {
    /// Return a [`BackendRef`] for this backend
    pub fn backend_ref(&self) -> BackendRef {
        self.backend_ref.clone()
    }
}

impl Drop for NativeBackend {
    fn drop(&mut self) {
        unsafe {
            // VRT_delete_backend doesn't actually use the context (it's cast to void)
            let mut bep = self.backend_ref.vcl_ptr();
            ffi::VRT_delete_backend(null(), &raw mut bep);
        }
    }
}

impl AsRef<BackendRef> for NativeBackend {
    fn as_ref(&self) -> &BackendRef {
        &self.backend_ref
    }
}

/// Internal enum to store the backend endpoint type
#[derive(Debug, Clone, Copy)]
enum BackendEndpoint<'a> {
    Ip(SocketAddr),
    Uds(&'a CStr),
}

/// Macro to generate builder setter methods
macro_rules! builder_setter {
    ($name:ident, $type:ty, $doc:expr) => {
        #[doc = $doc]
        #[must_use]
        pub fn $name(mut self, $name: $type) -> Self {
            self.$name = Some($name);
            self
        }
    };
}

/// Builder for creating a [`NativeBackend`]
///
/// Provides a fluent interface for configuring and creating native Varnish backends.
#[derive(Debug)]
pub struct NativeBackendBuilder<'a> {
    endpoint: Option<BackendEndpoint<'a>>,
    vcl_name: &'a CStr,
    hosthdr: Option<&'a CStr>,
    authority: Option<&'a CStr>,
    connect_timeout: Option<std::time::Duration>,
    first_byte_timeout: Option<std::time::Duration>,
    between_bytes_timeout: Option<std::time::Duration>,
    backend_wait_timeout: Option<std::time::Duration>,
    max_connections: Option<u32>,
    proxy_header: Option<u32>,
    backend_wait_limit: Option<u32>,
}

impl<'a> NativeBackendBuilder<'a> {
    /// Create a new builder for a TCP/IP backend
    pub fn new_ip(vcl_name: &'a CStr, addr: SocketAddr) -> Self {
        Self {
            endpoint: Some(BackendEndpoint::Ip(addr)),
            vcl_name,
            hosthdr: None,
            authority: None,
            connect_timeout: None,
            first_byte_timeout: None,
            between_bytes_timeout: None,
            backend_wait_timeout: None,
            max_connections: None,
            proxy_header: None,
            backend_wait_limit: None,
        }
    }

    /// Create a new builder for a Unix domain socket backend
    pub fn new_uds(vcl_name: &'a CStr, path: &'a CStr) -> Self {
        Self {
            endpoint: Some(BackendEndpoint::Uds(path)),
            vcl_name,
            hosthdr: None,
            authority: None,
            connect_timeout: None,
            first_byte_timeout: None,
            between_bytes_timeout: None,
            backend_wait_timeout: None,
            max_connections: None,
            proxy_header: None,
            backend_wait_limit: None,
        }
    }

    builder_setter!(hosthdr, &'a CStr, "Set the Host header to use when connecting");
    builder_setter!(authority, &'a CStr, "Set the authority for this backend");
    builder_setter!(connect_timeout, std::time::Duration, "Set the connection timeout");
    builder_setter!(first_byte_timeout, std::time::Duration, "Set the first byte timeout");
    builder_setter!(between_bytes_timeout, std::time::Duration, "Set the between bytes timeout");
    builder_setter!(backend_wait_timeout, std::time::Duration, "Set the backend wait timeout");
    builder_setter!(max_connections, u32, "Set the maximum number of connections");
    builder_setter!(proxy_header, u32, "Set the proxy protocol header version (0 = disabled, 1 or 2)");
    builder_setter!(backend_wait_limit, u32, "Set the backend wait limit");

    /// Build the native backend
    pub fn build(self, ctx: &mut Ctx) -> VclResult<NativeBackend> {
        // Validate required fields
        let endpoint_type = self
            .endpoint
            .expect("endpoint must be set before calling build()");

        // Create the endpoint
        let mut endpoint = Box::new(ffi::vrt_endpoint {
            magic: ffi::VRT_ENDPOINT_MAGIC,
            ipv4: VCL_IP(null()),
            ipv6: VCL_IP(null()),
            uds_path: null(),
            preamble: null(),
        });

        // Set endpoint based on type
        match endpoint_type {
            BackendEndpoint::Uds(path) => {
                endpoint.uds_path = path.as_ptr();
            }
            BackendEndpoint::Ip(addr) => {
                let sa = addr.into_vcl(&mut ctx.ws)?;
                match addr {
                    SocketAddr::V4(_) => endpoint.ipv4 = sa,
                    SocketAddr::V6(_) => endpoint.ipv6 = sa,
                }
            }
        }

        // Create the backend config
        let backend_config = Box::new(ffi::vrt_backend {
            magic: ffi::VRT_BACKEND_MAGIC,
            endpoint: &raw const *endpoint,
            vcl_name: self.vcl_name.as_ptr(),
            hosthdr: self.hosthdr.map_or(null(), CStr::as_ptr),
            authority: self.authority.map_or(null(), CStr::as_ptr),
            connect_timeout: ffi::vtim_dur(self
                .connect_timeout
                .map_or(-1.0, |d| d.as_secs_f64())),
            first_byte_timeout: ffi::vtim_dur(self
                .first_byte_timeout
                .map_or(-1.0, |d| d.as_secs_f64())),
            between_bytes_timeout: ffi::vtim_dur(self
                .between_bytes_timeout
                .map_or(-1.0, |d| d.as_secs_f64())),
            backend_wait_timeout: ffi::vtim_dur(self
                .backend_wait_timeout
                .map_or(-1.0, |d| d.as_secs_f64())),
            max_connections: self.max_connections.unwrap_or(0),
            proxy_header: self.proxy_header.unwrap_or(0),
            backend_wait_limit: self.backend_wait_limit.unwrap_or(0),
            probe: ffi::VCL_PROBE(null()),
        });

        // Create the backend via VRT_new_backend (NULL via backend)
        let bep = unsafe { ffi::VRT_new_backend(ctx.raw, &raw const *backend_config, VCL_BACKEND(null())) };

        if bep.0.is_null() {
            return Err(
                format!("VRT_new_backend returned null for {}", self.vcl_name.to_string_lossy()).into(),
            );
        }

        let backend_ref = unsafe { BackendRef::new_withouth_refcount(bep).expect("Backend pointer should never be null") };

        Ok(NativeBackend {
            endpoint,
            backend_config,
            backend_ref,
        })
    }
}

#[derive(Debug)]
pub struct BackendRef {
    refcounted: bool,
    bep: VCL_BACKEND,
}

impl BackendRef {
    pub unsafe fn new(bep: VCL_BACKEND) -> Option<Self> {
        if bep.0.is_null() {
            return None;
        }
        unsafe {
            let dir = bep.0.as_ref()?;
            assert_eq!(dir.magic, ffi::DIRECTOR_MAGIC);
            let vdir = dir.vdir.as_mut().expect("vdir can't be null");
            assert_eq!(vdir.magic, ffi::VCLDIR_MAGIC);
            if vdir.flags & ffi::VDIR_FLG_NOREFCNT == 0 {
                ffi::Lck__Lock(
                    &raw mut vdir.dlck,
                    c"BackendRef::new".as_ptr(),
                    line!() as i32,
                );
                assert!(vdir.refcnt > 0);
                vdir.refcnt += 1;
                ffi::Lck__Unlock(
                    &raw mut vdir.dlck,
                    c"BackendRef::new".as_ptr(),
                    line!() as i32,
                );
            }
        }
        Some(BackendRef { bep, refcounted: true })
    }

    // this doesn't grab a reference and is only used to
    // create the backend_ref fields of Backend, NativeBackend and Director
    unsafe fn new_withouth_refcount(bep: VCL_BACKEND) -> Option<Self> {
        if bep.0.is_null() {
            return None;
        }
        Some(BackendRef { bep, refcounted: false })
    }

    pub fn probe(&self, ctx: &Ctx) -> ProbeResult {
        let mut changed = VCL_TIME::default();
        let healthy = unsafe { ffi::VRT_Healthy(ctx.raw, self.bep, &raw mut changed).into() };
        let last_changed = changed.try_into().unwrap_or(SystemTime::UNIX_EPOCH);
        ProbeResult {
            healthy,
            last_changed,
        }
    }

    pub fn name(&self) -> &CStr {
        assert!(!self.bep.0.is_null());
        unsafe {
            let dir = *self.bep.0;
            assert_eq!(dir.magic, ffi::DIRECTOR_MAGIC);
            CStr::from_ptr(dir.vcl_name)
        }
    }

    pub fn vcl_ptr(&self) -> VCL_BACKEND {
        // need to clone, we'll clear bep on drop
        self.bep
    }
}

impl Clone for BackendRef {
    fn clone(&self) -> BackendRef {
        // self.vcl_ptr() will never be null
        unsafe { BackendRef::new(self.vcl_ptr()).unwrap() }
    }
}

impl Drop for BackendRef {
    fn drop(&mut self) {
        assert!(!self.bep.0.is_null());
        if self.refcounted {
            unsafe {
                ffi::VRT_Assign_Backend(&raw mut self.bep, VCL_BACKEND(null()));
            }
        };
    }
}

/// Creates a JSON string with custom indentation and writes it to a Buffer:
/// - Top level line has no indentation
/// - All other lines have 6 extra spaces on top of normal indentation
/// - Appends a trailing comma and newline with indentation
/// 
/// Usage: `report_details_json!(vsb, serde_json::json!({ "key": "value" }))`
#[macro_export]
macro_rules! report_details_json {
    ($vsb:expr, $json_value:expr) => {{
        let json_str = serde_json::to_string_pretty(&$json_value)
            .expect("Failed to serialize JSON");
        
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

