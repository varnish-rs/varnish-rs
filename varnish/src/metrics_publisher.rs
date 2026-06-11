//! Expose custom Varnish statistics counters (VSC) from a VMOD.
//!
//! The VSC (Varnish Shared Counter) publisher side lets a VMOD define its own counters that are
//! visible to `varnishstat` and other monitoring tools. Define a struct, derive [`VscMetric`],
//! wrap it in [`Vsc`], and access the fields directly via `Deref`.
//!
//! For the read-only counterpart used by external tools, see [`metrics_reader`](crate::metrics_reader).

use std::ffi::{CStr, CString};
use std::mem::size_of;
use std::ops::{Deref, DerefMut};
use std::ptr::null_mut;

use varnish_sys::ffi::{vsc_seg, VRT_VSC_Alloc, VRT_VSC_Destroy};

/// Trait implemented by structs derived with `#[derive(VscMetric)]`.
///
/// Do not implement this trait manually — use the derive macro instead, which generates the
/// correct JSON metadata that Varnish requires.
pub unsafe trait VscMetric {
    fn get_metadata() -> &'static CStr;
}

/// Owns a live Varnish statistics (VSC) segment for a `T: VscMetric` struct.
///
/// Create one with [`Vsc::new`] and keep it alive for as long as the metrics should be
/// visible. `Deref`s to `T`, so you can access fields directly.
///
/// ```rust,ignore
/// let vsc = Vsc::<MyMetrics>::new("mymod", "mymod");
/// vsc.requests.fetch_add(1, Ordering::Relaxed);
/// ```
pub struct Vsc<T: VscMetric> {
    metric: *mut T,
    seg: *mut vsc_seg,
    name: CString,
}

impl<T: VscMetric> Vsc<T> {
    /// Allocate a new VSC segment.
    ///
    /// - `module_name`: unique instance name shown in `varnishstat` (e.g. `"mymod"`)
    /// - `module_prefix`: prefix for stat names in the output (e.g. `"mymod"`)
    ///
    /// Panics if either string contains an interior nul byte, or if Varnish fails to allocate
    /// the segment.
    pub fn new(module_name: &str, module_prefix: &str) -> Self {
        let mut seg = null_mut();
        let name = CString::new(module_name).expect("module_name contained interior nul byte");
        let format =
            CString::new(module_prefix).expect("module_prefix contained interior nul byte");

        let metadata_json = T::get_metadata();

        let metric = unsafe {
            VRT_VSC_Alloc(
                null_mut(),
                &raw mut seg,
                name.as_ptr(),
                size_of::<T>(),
                metadata_json.as_ptr().cast::<std::os::raw::c_uchar>(),
                metadata_json.to_bytes().len(),
                format.as_ptr(),
            )
            .cast::<T>()
        };

        assert!(
            !metric.is_null(),
            "VSC segment allocation failed for {module_name}"
        );

        Self { metric, seg, name }
    }
}

impl<T: VscMetric> Drop for Vsc<T> {
    fn drop(&mut self) {
        unsafe {
            VRT_VSC_Destroy(self.name.as_ptr(), self.seg);
        }
    }
}

impl<T: VscMetric> Deref for Vsc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.metric }
    }
}

impl<T: VscMetric> DerefMut for Vsc<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.metric }
    }
}
