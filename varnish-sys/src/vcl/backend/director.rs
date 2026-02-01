use std::ffi::{c_char, c_void, CString};
use std::ptr;
use std::ptr::null;

use crate::ffi::{VclEvent, VCL_BACKEND, VCL_BOOL, VCL_TIME};
use crate::vcl::{Buffer, Ctx, VclResult};
use crate::{ffi, validate_director};

use super::{BackendRef, ProbeResult};

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

    /// Called when the VCL temperature changes or is discarded
    ///
    /// Corresponds to the `event` callback in `vdi_methods`.
    fn event(&self, event: VclEvent) {
        let _ = event;
    }
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
            release: None,
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

        let backend_ref = unsafe {
            BackendRef::new_withouth_refcount(bep).expect("Backend pointer should never be null")
        };

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
        }
    }
}

impl<D: VclDirector> AsRef<BackendRef> for Director<D> {
    fn as_ref(&self) -> &BackendRef {
        &self.backend_ref
    }
}

// C FFI wrapper functions

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
