//! Thread-safe collection of backends for directors
//!
//! [`BackendSet`] provides a thread-safe way to manage a collection of backends
//! with proper reference counting via `VRT_Assign_Backend()`. It includes helpers
//! for health checking and weighted backend selection.
//!
//! # Example
//!
//! ```ignore
//! use varnish::vcl::{BackendSet, Ctx, VclDirector, VCL_BACKEND};
//! use std::sync::atomic::{AtomicUsize, Ordering};
//!
//! struct RoundRobin {
//!     backends: BackendSet,
//!     next: AtomicUsize,
//! }
//!
//! impl VclDirector for RoundRobin {
//!     fn resolve(&self, ctx: &mut Ctx) -> Option<VCL_BACKEND> {
//!         let healthy = self.backends.healthy_list(ctx);
//!         if healthy.is_empty() {
//!             return None;
//!         }
//!         let idx = self.next.fetch_add(1, Ordering::Relaxed) % healthy.len();
//!         Some(healthy[idx])
//!     }
//!
//!     fn release(&self) {
//!         self.backends.release_all();
//!     }
//! }
//! ```
//!
//! # Thread Safety
//!
//! All methods are thread-safe. Read operations (health checks, listing) can
//! proceed concurrently, while write operations (add, remove) are serialized.
//!
//! # Lifetime Management
//!
//! [`BackendSet`] properly manages backend reference counts:
//! - [`BackendSet::add`] increments the reference count
//! - [`BackendSet::remove`] decrements the reference count
//! - [`BackendSet::release_all`] releases all references (call when VCL goes cold)
//! - [`Drop`] ensures all references are released

use std::ptr;
use std::sync::RwLock;
use std::time::SystemTime;

use crate::ffi::{self, vtim_real, VCL_BACKEND, VCL_TIME};
use crate::vcl::Ctx;

/// Thread-safe collection of backends for directors
///
/// Handles proper refcounting via `VRT_Assign_Backend()` and provides
/// helpers for health checking and backend selection.
///
/// # Thread Safety
///
/// Uses internal [`RwLock`] to allow concurrent read access while serializing
/// modifications. All public methods are safe to call from multiple threads.
#[derive(Debug)]
pub struct BackendSet {
    inner: RwLock<BackendSetInner>,
}

#[derive(Debug)]
struct BackendSetInner {
    /// Backend pointers (properly refcounted)
    backends: Vec<VCL_BACKEND>,
    /// Weights for weighted selection (parallel to backends)
    weights: Vec<f64>,
    /// Sum of all weights
    total_weight: f64,
}

impl BackendSet {
    /// Create an empty backend set
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(BackendSetInner {
                backends: Vec::new(),
                weights: Vec::new(),
                total_weight: 0.0,
            }),
        }
    }

    /// Create a backend set with pre-allocated capacity
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: RwLock::new(BackendSetInner {
                backends: Vec::with_capacity(capacity),
                weights: Vec::with_capacity(capacity),
                total_weight: 0.0,
            }),
        }
    }

    /// Add a backend with weight 1.0
    ///
    /// This properly increments the backend's reference count.
    ///
    /// # Thread Safety
    /// This method acquires a write lock.
    pub fn add(&self, be: VCL_BACKEND) {
        self.add_weighted(be, 1.0);
    }

    /// Add a backend with a custom weight
    ///
    /// Weight affects selection probability in [`BackendSet::pick_by_weight`].
    /// A weight of 0.0 or negative means the backend won't be selected by weight,
    /// but will still be included in [`BackendSet::healthy_list`].
    ///
    /// This properly increments the backend's reference count.
    ///
    /// # Thread Safety
    /// This method acquires a write lock.
    pub fn add_weighted(&self, be: VCL_BACKEND, weight: f64) {
        let mut inner = self.inner.write().unwrap();

        // Create a slot and properly increment refcount
        let mut slot = VCL_BACKEND(ptr::null_mut());
        unsafe {
            ffi::VRT_Assign_Backend(&mut slot, be);
        }

        inner.backends.push(slot);
        let effective_weight = weight.max(0.0);
        inner.weights.push(effective_weight);
        inner.total_weight += effective_weight;
    }

    /// Remove a backend from the set
    ///
    /// Returns `true` if the backend was found and removed, `false` otherwise.
    /// This properly decrements the backend's reference count.
    ///
    /// # Thread Safety
    /// This method acquires a write lock.
    pub fn remove(&self, be: VCL_BACKEND) -> bool {
        let mut inner = self.inner.write().unwrap();

        for i in 0..inner.backends.len() {
            // Compare inner pointers since VCL_BACKEND doesn't impl PartialEq
            if inner.backends[i].0 == be.0 {
                // Decrement refcount by assigning null
                unsafe {
                    ffi::VRT_Assign_Backend(&mut inner.backends[i], VCL_BACKEND(ptr::null_mut()));
                }

                inner.total_weight -= inner.weights[i];
                inner.backends.remove(i);
                inner.weights.remove(i);
                return true;
            }
        }
        false
    }

    /// Get the number of backends in the set
    ///
    /// # Thread Safety
    /// This method acquires a read lock.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().unwrap().backends.len()
    }

    /// Check if the set is empty
    ///
    /// # Thread Safety
    /// This method acquires a read lock.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Check if any backend in the set is healthy
    ///
    /// Returns `(is_healthy, last_change_time)` where `last_change_time` is
    /// the most recent health state change across all backends.
    ///
    /// # Thread Safety
    /// This method acquires a read lock.
    pub fn any_healthy(&self, ctx: &Ctx) -> (bool, SystemTime) {
        let inner = self.inner.read().unwrap();
        let mut latest_change = VCL_TIME(vtim_real(0.0));

        for &be in &inner.backends {
            let mut changed = VCL_TIME(vtim_real(0.0));
            let healthy: bool = unsafe { ffi::VRT_Healthy(ctx.raw, be, &mut changed).into() };
            // Compare inner f64 values
            if (changed.0).0 > (latest_change.0).0 {
                latest_change = changed;
            }
            if healthy {
                let time = SystemTime::UNIX_EPOCH
                    + std::time::Duration::from_secs_f64((latest_change.0).0);
                return (true, time);
            }
        }

        let time = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs_f64((latest_change.0).0);
        (false, time)
    }

    /// Get a list of all healthy backends
    ///
    /// Returns a new `Vec` containing only the backends that are currently healthy.
    /// The order matches the insertion order.
    ///
    /// # Thread Safety
    /// This method acquires a read lock.
    #[must_use]
    pub fn healthy_list(&self, ctx: &Ctx) -> Vec<VCL_BACKEND> {
        let inner = self.inner.read().unwrap();
        inner
            .backends
            .iter()
            .copied()
            .filter(|&be| {
                let healthy: bool =
                    unsafe { ffi::VRT_Healthy(ctx.raw, be, ptr::null_mut()).into() };
                healthy
            })
            .collect()
    }

    /// Get a list of all backends (healthy or not)
    ///
    /// Returns a new `Vec` containing all backends in insertion order.
    ///
    /// # Thread Safety
    /// This method acquires a read lock.
    #[must_use]
    pub fn all(&self) -> Vec<VCL_BACKEND> {
        self.inner.read().unwrap().backends.clone()
    }

    /// Pick a backend by weighted random selection
    ///
    /// Takes a value `w` in range `[0.0, 1.0)` and returns a healthy backend
    /// based on weight distribution. Backends with higher weights are more
    /// likely to be selected.
    ///
    /// Returns `None` if no healthy backends are available.
    ///
    /// # Arguments
    /// * `ctx` - VCL context for health checks
    /// * `w` - Random value in `[0.0, 1.0)`, typically from `rand::random::<f64>()`
    ///
    /// # Thread Safety
    /// This method acquires a read lock.
    #[must_use]
    pub fn pick_by_weight(&self, ctx: &Ctx, w: f64) -> Option<VCL_BACKEND> {
        let inner = self.inner.read().unwrap();

        // Calculate total weight of healthy backends
        let mut healthy_weight = 0.0;
        for (i, &be) in inner.backends.iter().enumerate() {
            let healthy: bool = unsafe { ffi::VRT_Healthy(ctx.raw, be, ptr::null_mut()).into() };
            if healthy {
                healthy_weight += inner.weights[i];
            }
        }

        if healthy_weight <= 0.0 {
            return None;
        }

        // Clamp w to valid range
        let w = w.clamp(0.0, 0.999_999_999);
        let target = w * healthy_weight;
        let mut acc = 0.0;

        for (i, &be) in inner.backends.iter().enumerate() {
            let healthy: bool = unsafe { ffi::VRT_Healthy(ctx.raw, be, ptr::null_mut()).into() };
            if healthy {
                acc += inner.weights[i];
                if target < acc {
                    return Some(be);
                }
            }
        }

        // Fallback: return last healthy backend (shouldn't normally reach here)
        for &be in inner.backends.iter().rev() {
            let healthy: bool = unsafe { ffi::VRT_Healthy(ctx.raw, be, ptr::null_mut()).into() };
            if healthy {
                return Some(be);
            }
        }

        None
    }

    /// Pick a backend by index (round-robin helper)
    ///
    /// Returns the backend at position `idx % len` from the healthy backends.
    /// Returns `None` if no healthy backends are available.
    ///
    /// # Thread Safety
    /// This method acquires a read lock.
    #[must_use]
    pub fn pick_by_index(&self, ctx: &Ctx, idx: usize) -> Option<VCL_BACKEND> {
        let healthy = self.healthy_list(ctx);
        if healthy.is_empty() {
            None
        } else {
            Some(healthy[idx % healthy.len()])
        }
    }

    /// Pick the first healthy backend (fallback helper)
    ///
    /// Returns the first backend in insertion order that is healthy.
    /// Useful for fallback/priority directors.
    ///
    /// # Thread Safety
    /// This method acquires a read lock.
    #[must_use]
    pub fn pick_first_healthy(&self, ctx: &Ctx) -> Option<VCL_BACKEND> {
        let inner = self.inner.read().unwrap();

        for &be in &inner.backends {
            let healthy: bool = unsafe { ffi::VRT_Healthy(ctx.raw, be, ptr::null_mut()).into() };
            if healthy {
                return Some(be);
            }
        }

        None
    }

    /// Release all backend references
    ///
    /// **MUST** be called when the director's VCL goes cold (in [`VclDirector::release`]).
    /// After this call, the set is empty.
    ///
    /// Failure to call this method will cause resource leaks.
    ///
    /// # Thread Safety
    /// This method acquires a write lock.
    ///
    /// [`VclDirector::release`]: super::VclDirector::release
    pub fn release_all(&self) {
        let mut inner = self.inner.write().unwrap();

        for be in &mut inner.backends {
            unsafe {
                ffi::VRT_Assign_Backend(be, VCL_BACKEND(ptr::null_mut()));
            }
        }

        inner.backends.clear();
        inner.weights.clear();
        inner.total_weight = 0.0;
    }

    /// Get the total weight of all backends
    ///
    /// This is the sum of weights for all backends, regardless of health status.
    ///
    /// # Thread Safety
    /// This method acquires a read lock.
    #[must_use]
    pub fn total_weight(&self) -> f64 {
        self.inner.read().unwrap().total_weight
    }

    /// Get the weight of healthy backends
    ///
    /// # Thread Safety
    /// This method acquires a read lock.
    #[must_use]
    pub fn healthy_weight(&self, ctx: &Ctx) -> f64 {
        let inner = self.inner.read().unwrap();
        let mut weight = 0.0;

        for (i, &be) in inner.backends.iter().enumerate() {
            let healthy: bool = unsafe { ffi::VRT_Healthy(ctx.raw, be, ptr::null_mut()).into() };
            if healthy {
                weight += inner.weights[i];
            }
        }

        weight
    }

    /// Count healthy backends
    ///
    /// # Thread Safety
    /// This method acquires a read lock.
    #[must_use]
    pub fn healthy_count(&self, ctx: &Ctx) -> usize {
        let inner = self.inner.read().unwrap();
        let mut count = 0;

        for &be in &inner.backends {
            let healthy: bool = unsafe { ffi::VRT_Healthy(ctx.raw, be, ptr::null_mut()).into() };
            if healthy {
                count += 1;
            }
        }

        count
    }
}

impl Default for BackendSet {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BackendSet {
    fn drop(&mut self) {
        // Ensure all backends are released
        // Note: This should have been done via release_all() already,
        // but we do it here as a safety net to prevent leaks.
        self.release_all();
    }
}

// BackendSet is Send + Sync because:
// - All mutations are protected by RwLock
// - VCL_BACKEND is just a pointer, and we use VRT_Assign_Backend for proper refcounting
// - The FFI functions we call are thread-safe per Varnish documentation
unsafe impl Send for BackendSet {}
unsafe impl Sync for BackendSet {}
