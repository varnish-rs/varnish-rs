//! Facilities to create a VMOD director
//!
//! Directors select from multiple backends based on custom logic. Unlike
//! [`VclBackend`](super::VclBackend) which handles HTTP directly, directors implement
//! `resolve()` to return another backend.
//!
//! Here's what's in the toolbox:
//! - the [`Director`] type wraps a [`VclDirector`]-implementing struct into a C director
//! - the [`VclDirector`] trait defines which methods to implement to act as a director
//!
//! # Example
//!
//! ```ignore
//! use std::sync::atomic::{AtomicUsize, Ordering};
//! use varnish::vcl::{Ctx, Director, VclDirector, VCL_BACKEND};
//!
//! // A simple round-robin director
//! struct RoundRobin {
//!     backends: Vec<VCL_BACKEND>,
//!     next: AtomicUsize,
//! }
//!
//! impl VclDirector for RoundRobin {
//!     fn resolve(&self, ctx: &mut Ctx) -> Option<VCL_BACKEND> {
//!         if self.backends.is_empty() {
//!             return None;
//!         }
//!         let idx = self.next.fetch_add(1, Ordering::Relaxed) % self.backends.len();
//!         Some(self.backends[idx])
//!     }
//!
//!     fn release(&self) {
//!         // Release backend references when VCL goes cold
//!     }
//! }
//!
//! fn create_director(ctx: &mut Ctx) {
//!     let rr = RoundRobin {
//!         backends: vec![],
//!         next: AtomicUsize::new(0),
//!     };
//!     let director = Director::new(ctx, "round-robin", "my_rr", rr)
//!         .expect("couldn't create director");
//!     let ptr = director.vcl_ptr();
//! }
//! ```

use std::ffi::{c_char, c_int, c_void, CString};
use std::ptr;
use std::time::SystemTime;

use crate::ffi::{VclEvent, VCL_BACKEND, VCL_BOOL, VCL_TIME};
use crate::vcl::{Buffer, Ctx, VclResult};
use crate::{ffi, validate_director};

/// Trait for implementing a director
///
/// Directors select from multiple backends. Unlike [`VclBackend`](super::VclBackend) which handles
/// HTTP directly, directors implement [`VclDirector::resolve`] to return another backend.
///
/// # Thread Safety
///
/// All methods may be called concurrently from multiple worker threads.
/// Implementations must be `Send + Sync`.
///
/// # Lifetime Requirements
///
/// The director must remain valid until [`VclDirector::release`] is called,
/// which happens when the VCL goes cold.
pub trait VclDirector: Send + Sync {
    /// Select a backend for this request
    ///
    /// Called by Varnish when this director is selected. Return `None` if
    /// no healthy backend is available.
    ///
    /// # Thread Safety
    /// This method may be called concurrently from multiple worker threads.
    fn resolve(&self, ctx: &mut Ctx) -> Option<VCL_BACKEND>;

    /// Check if any backend is healthy
    ///
    /// Returns `(is_healthy, last_change_time)`. The default implementation
    /// returns `(true, UNIX_EPOCH)` - override if you track backend health.
    fn healthy(&self, _ctx: &mut Ctx) -> (bool, SystemTime) {
        (true, SystemTime::UNIX_EPOCH)
    }

    /// Release backend references
    ///
    /// Called when the VCL goes cold. Directors MUST release all stored
    /// `VCL_BACKEND` references by calling `VRT_Assign_Backend(ptr, NULL)`.
    ///
    /// Failure to implement this correctly will cause resource leaks.
    fn release(&self);

    /// Called when the director is being destroyed
    ///
    /// Default implementation does nothing. Override if you have resources
    /// beyond backend references to clean up.
    fn destroy(&self) {}

    /// Handle VCL events
    ///
    /// Called for `VCL_EVENT_WARM`, `VCL_EVENT_COLD`, etc.
    fn event(&self, _event: VclEvent) {}

    /// Provide listing output for `varnishadm backend.list`
    fn list(&self, _ctx: &mut Ctx, _vsb: &mut Buffer, _detailed: bool, _json: bool) {}

    /// Provide panic output for debugging
    fn panic(&self, _vsb: &mut Buffer) {}
}

/// Wrapper that registers a [`VclDirector`] implementation with Varnish
///
/// This handles the FFI boilerplate needed to register a director with Varnish.
/// The director is automatically unregistered when dropped.
///
/// Once created, a [`Director`]'s sole purpose is to exist as a C reference for the VCL.
/// As a result, you don't want to drop it until after all requests are done. The most
/// common way is to have the director be part of a vmod object because the object
/// won't be dropped until the VCL is discarded.
#[derive(Debug)]
pub struct Director<D: VclDirector> {
    ptr: VCL_BACKEND,
    #[expect(dead_code)]
    methods: Box<ffi::vdi_methods>,
    inner: Box<D>,
    #[expect(dead_code)]
    ctype: CString,
}

impl<D: VclDirector> Director<D> {
    /// Create a new director
    ///
    /// # Arguments
    /// * `ctx` - VCL context (should be in `vcl_init`)
    /// * `director_type` - Type name shown in logs (e.g., "round-robin")
    /// * `name` - Director instance name
    /// * `director` - The director implementation
    ///
    /// # Errors
    /// Returns error if director creation fails (e.g., out of memory)
    pub fn new(
        ctx: &mut Ctx,
        director_type: &str,
        name: &str,
        director: D,
    ) -> VclResult<Self> {
        let mut inner = Box::new(director);
        let ctype = CString::new(director_type).map_err(|e| e.to_string())?;
        let cname = CString::new(name).map_err(|e| e.to_string())?;

        let methods = Box::new(ffi::vdi_methods {
            magic: ffi::VDI_METHODS_MAGIC,
            type_: ctype.as_ptr(),
            // Director methods
            resolve: Some(wrap_resolve::<D>),
            healthy: Some(wrap_healthy::<D>),
            release: Some(wrap_release::<D>),
            destroy: Some(wrap_destroy::<D>),
            event: Some(wrap_event::<D>),
            list: Some(wrap_list::<D>),
            panic: Some(wrap_panic::<D>),
            // Backend methods - not used for directors
            http1pipe: None,
            gethdrs: None,
            getip: None,
            finish: None,
        });

        let ptr = unsafe {
            ffi::VRT_AddDirector(
                ctx.raw,
                &raw const *methods,
                ptr::from_mut::<D>(&mut *inner).cast::<c_void>(),
                c"%.*s".as_ptr(),
                cname.as_bytes().len(),
                cname.as_ptr().cast::<c_char>(),
            )
        };

        if ptr.0.is_null() {
            return Err(format!("VRT_AddDirector returned null for {name}").into());
        }

        Ok(Self {
            ptr,
            methods,
            inner,
            ctype,
        })
    }

    /// Return the C pointer wrapped by the [`Director`].
    ///
    /// Conventionally used by the `.backend()` methods of VCL objects.
    pub fn vcl_ptr(&self) -> VCL_BACKEND {
        self.ptr
    }

    /// Access the inner director implementation.
    ///
    /// Note that it isn't `mut` as other threads are likely to have access to it too.
    pub fn get_inner(&self) -> &D {
        &self.inner
    }
}

impl<D: VclDirector> Drop for Director<D> {
    fn drop(&mut self) {
        unsafe {
            ffi::VRT_DelDirector(&raw mut self.ptr);
        }
    }
}

// ============================================================================
// FFI wrapper functions
// ============================================================================

/// Helper to extract the director implementation from a validated director
///
/// # Safety
/// The `d` must be a valid director created by [`Director::new`].
fn get_director<D: VclDirector>(d: &ffi::director) -> &D {
    unsafe { d.priv_.cast::<D>().as_ref().expect("null director priv") }
}

unsafe extern "C" fn wrap_resolve<D: VclDirector>(
    ctxp: *const ffi::vrt_ctx,
    be: VCL_BACKEND,
) -> VCL_BACKEND {
    let mut ctx = Ctx::from_ptr(ctxp);
    let director: &D = get_director(validate_director(be));

    director
        .resolve(&mut ctx)
        .unwrap_or(VCL_BACKEND(ptr::null_mut()))
}

unsafe extern "C" fn wrap_healthy<D: VclDirector>(
    ctxp: *const ffi::vrt_ctx,
    be: VCL_BACKEND,
    changed: *mut VCL_TIME,
) -> VCL_BOOL {
    let mut ctx = Ctx::from_ptr(ctxp);
    let director: &D = get_director(validate_director(be));

    let (healthy, when) = director.healthy(&mut ctx);
    if !changed.is_null() {
        *changed = when.try_into().unwrap(); // FIXME: on error?
    }
    healthy.into()
}

unsafe extern "C" fn wrap_release<D: VclDirector>(be: VCL_BACKEND) {
    let director: &D = get_director(validate_director(be));
    director.release();
}

unsafe extern "C" fn wrap_destroy<D: VclDirector>(be: VCL_BACKEND) {
    let director: &D = get_director(validate_director(be));
    director.destroy();
}

unsafe extern "C" fn wrap_event<D: VclDirector>(be: VCL_BACKEND, ev: VclEvent) {
    let director: &D = get_director(validate_director(be));
    director.event(ev);
}

unsafe extern "C" fn wrap_list<D: VclDirector>(
    ctxp: *const ffi::vrt_ctx,
    be: VCL_BACKEND,
    vsbp: *mut ffi::vsb,
    detailed: c_int,
    json: c_int,
) {
    let mut ctx = Ctx::from_ptr(ctxp);
    let mut vsb = Buffer::from_ptr(vsbp);
    let director: &D = get_director(validate_director(be));
    director.list(&mut ctx, &mut vsb, detailed != 0, json != 0);
}

unsafe extern "C" fn wrap_panic<D: VclDirector>(be: VCL_BACKEND, vsbp: *mut ffi::vsb) {
    let mut vsb = Buffer::from_ptr(vsbp);
    let director: &D = get_director(validate_director(be));
    director.panic(&mut vsb);
}
