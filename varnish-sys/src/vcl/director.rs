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
//!
//! # Director Lifecycle
//!
//! ## Creation
//!
//! Directors are created during VCL initialization by calling `Director::new()`. This function
//! performs the following operations:
//!
//! 1. Boxes the director implementation on the heap
//! 2. Creates a `vdi_methods` structure with function pointers to FFI wrappers
//! 3. Calls `VRT_AddDirector()` to register the director with Varnish
//! 4. Returns a `Director<D>` wrapper containing the VCL_BACKEND pointer
//!
//! The director implementation is stored in the `Director` wrapper and will remain valid
//! for the lifetime of the VCL.
//!
//! ## Backend Registration
//!
//! When a director stores backend references, it must use `VRT_Assign_Backend()` to properly
//! manage reference counts. This function increments the reference count of the assigned backend
//! and decrements the reference count of any previously assigned backend.
//!
//! Correct usage:
//!
//! ```ignore
//! let mut slot = VCL_BACKEND(std::ptr::null_mut());
//! unsafe { ffi::VRT_Assign_Backend(&mut slot, backend); }
//! self.backends.push(slot);
//! ```
//!
//! Incorrect usage that will cause use-after-free bugs:
//!
//! ```ignore
//! self.backends.push(backend);  // Missing VRT_Assign_Backend call
//! ```
//!
//! ## Request Processing
//!
//! When a request is processed and the director is selected as the backend hint, Varnish calls
//! the `resolve` callback through the FFI wrapper. The `resolve` method is expected to return
//! a `VCL_BACKEND` pointer to the selected backend, or `None` if no backend is available.
//!
//! The `resolve` method may be called concurrently from multiple worker threads. All state
//! accessed by `resolve` must be protected by appropriate synchronization primitives.
//!
//! ## Health Checking
//!
//! Varnish may call the `healthy` callback to determine the overall health of the director.
//! This is typically used to aggregate the health state of child backends. The method returns
//! a boolean indicating health status and a timestamp of the last health change.
//!
//! ## VCL State Transitions
//!
//! VCL goes through several states during its lifetime:
//!
//! 1. COLD - Initial state after loading, not handling requests
//! 2. WARM - Active state, handling requests
//! 3. COLD - Transitioned back when VCL is replaced
//! 4. Discarded - VCL is unloaded and resources are freed
//!
//! When VCL transitions from WARM to COLD, the `release` callback is invoked. This callback
//! must release all backend references by calling `VRT_Assign_Backend(ptr, NULL)` for each
//! stored backend pointer. The `release` method must be idempotent as it may be called
//! multiple times.
//!
//! Example release implementation:
//!
//! ```ignore
//! fn release(&self) {
//!     let mut inner = self.backends.write().unwrap();
//!     for be in &mut inner.backends {
//!         unsafe {
//!             ffi::VRT_Assign_Backend(be, VCL_BACKEND(std::ptr::null_mut()));
//!         }
//!     }
//!     inner.backends.clear();
//! }
//! ```
//!
//! ## Destruction
//!
//! When the `Director<D>` is dropped, the `Drop` implementation calls `VRT_DelDirector()`
//! to unregister the director from Varnish. This triggers the `destroy` callback, which can
//! be used for final cleanup. After `VRT_DelDirector()` returns, the boxed director
//! implementation is dropped.
//!
//! The destruction sequence ensures that Varnish will not call director methods after the
//! implementation is freed.
//!
//! ## Backend Reference Counting
//!
//! Backend reference counting follows these rules:
//!
//! 1. `VRT_Assign_Backend(&mut slot, backend)` increments the refcount of `backend`
//! 2. `VRT_Assign_Backend(&mut slot, backend)` decrements the refcount of the old value in `slot`
//! 3. `VRT_Assign_Backend(&mut slot, NULL)` decrements the refcount and clears `slot`
//! 4. When refcount reaches zero, the backend may be freed by Varnish
//!
//! Example reference count flow:
//!
//! - Backend created in VCL: refcount = 1
//! - Director calls `VRT_Assign_Backend(&slot, backend)`: refcount = 2
//! - Director calls `VRT_Assign_Backend(&slot, NULL)`: refcount = 1
//! - VCL discarded: refcount = 0, backend freed
//!
//! ## Thread Safety Requirements
//!
//! Director implementations must be Send and Sync. The following methods may be called
//! concurrently:
//!
//! - `resolve` - Called by multiple worker threads
//! - `healthy` - Called by worker threads and health check thread
//!
//! The following methods are called single-threaded:
//!
//! - `release` - Called during VCL_EVENT_COLD
//! - `destroy` - Called during director destruction
//! - `event` - Called during VCL events
//!
//! Appropriate synchronization must be used for any shared mutable state. Common patterns:
//!
//! - AtomicUsize for counters
//! - RwLock for collections that are read frequently and written rarely
//! - Mutex for collections with frequent writes
//!
//! Using RefCell will cause runtime panics when resolve is called concurrently.
//!
//! ## Common Implementation Errors
//!
//! Missing release implementation:
//!
//! ```ignore
//! impl VclDirector for MyDirector {
//!     fn resolve(&self, ctx: &mut Ctx) -> Option<VCL_BACKEND> {
//!         self.backends.first()
//!     }
//!     // Missing: fn release(&self) { ... }
//! }
//! ```
//!
//! Result: Memory leak when VCL is discarded.
//!
//! Using RefCell for shared state:
//!
//! ```ignore
//! struct MyDirector {
//!     state: RefCell<State>,
//! }
//!
//! impl VclDirector for MyDirector {
//!     fn resolve(&self, ctx: &mut Ctx) -> Option<VCL_BACKEND> {
//!         self.state.borrow_mut()  // Panics under concurrent access
//!     }
//! }
//! ```
//!
//! Result: Runtime panic when multiple requests hit the director concurrently.
//!
//! Storing backends without refcounting:
//!
//! ```ignore
//! impl MyDirector {
//!     fn add(&mut self, be: VCL_BACKEND) {
//!         self.backends.push(be);  // Missing VRT_Assign_Backend
//!     }
//! }
//! ```
//!
//! Result: Backend may be freed while director still references it.
//!
//! Creating backends outside vcl_init:
//!
//! ```ignore
//! fn process_request(ctx: &mut Ctx) {
//!     let backend = NativeBackend::new(ctx, &config, None);  // Wrong VCL state
//! }
//! ```
//!
//! Result: Varnish may reject or crash due to VCL state machine violation.
//!
//! ## Recommended Practices
//!
//! 1. Always implement the `release` method and call `VRT_Assign_Backend(ptr, NULL)` for all backends
//! 2. Use `BackendSet` helper to automate reference counting
//! 3. Use lock-free atomics for simple counters
//! 4. Use RwLock for collections that are read concurrently
//! 5. Create backends only in vcl_init or ensure correct VCL state
//! 6. Test VCL reload scenarios to verify correct refcounting
//! 7. Run tests under Valgrind to detect memory leaks
//! 8. Make the release method idempotent

use std::ffi::{c_char, c_int, c_void, CString};
use std::mem::ManuallyDrop;
use std::ptr;
use std::time::SystemTime;

use crate::ffi::{vtim_real, VclEvent, VCL_BACKEND, VCL_BOOL, VCL_TIME};
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
    /// Wrapped in ManuallyDrop to ensure VRT_DelDirector is called before
    /// the inner director is dropped. This prevents use-after-free if Varnish
    /// calls director methods during VRT_DelDirector.
    inner: ManuallyDrop<Box<D>>,
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
    pub fn new(ctx: &mut Ctx, director_type: &str, name: &str, director: D) -> VclResult<Self> {
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
            inner: ManuallyDrop::new(inner),
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
        // SAFETY: We must call VRT_DelDirector first to unregister the director
        // from Varnish before dropping inner. This ensures Varnish won't call
        // director methods after the inner implementation is freed.
        unsafe {
            ffi::VRT_DelDirector(&raw mut self.ptr);
            // Now safe to drop inner - Varnish has finished with the director
            ManuallyDrop::drop(&mut self.inner);
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
        // Fall back to UNIX_EPOCH (0.0) if conversion fails (e.g., time before epoch)
        *changed = when.try_into().unwrap_or(VCL_TIME(vtim_real(0.0)));
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
