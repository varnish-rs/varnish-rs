//! Store data in a task-centric store to share with the C layers
//!
//! The workspace is a memory allocator with a simple API that allows Varnish to store data that
//! needs only to live for the lifetime of a task (handling a client or backend request for example).
//! At the end of the task, the workspace is wiped, simplifying memory management.
//!
//! Rust handles its own memory, but some data must be shared/returned to the C caller, and the
//! workspace is usually the easiest store available.
//!
//! **Note:** unless you know what you are doing, you should probably just use the automatic type
//! conversion provided by [`crate::vcl::convert`], or store things in
//! [`crate::vcl::vpriv::VPriv`].

use std::any::type_name;
use std::ffi::{c_char, c_void, CStr};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::mem::{align_of, size_of, transmute, MaybeUninit};
use std::num::NonZeroUsize;
use std::ptr;
use std::slice::from_raw_parts_mut;

use memchr::memchr;

#[cfg(varnishsys_6)]
use crate::ffi::WS_Inside;
use crate::ffi::{txt, VCL_STRING};
#[cfg(not(varnishsys_6))]
use crate::ffi::{vrt_blob, WS_Allocated, VCL_BLOB};
#[cfg(not(varnishsys_6))]
pub use crate::vcl::ws_str_buffer::WsBlobBuffer;
pub use crate::vcl::ws_str_buffer::{WsBuffer, WsStrBuffer, WsTempBuffer};
use crate::vcl::{VclError, VclResult};
use crate::{ffi, validate_ws};

#[cfg(not(test))]
impl ffi::ws {
    pub(crate) unsafe fn alloc(&mut self, size: u32) -> *mut c_void {
        assert!(size > 0);
        ffi::WS_Alloc(self, size)
    }
    pub(crate) unsafe fn reserve_all(&mut self) -> u32 {
        ffi::WS_ReserveAll(self)
    }
    pub(crate) unsafe fn release(&mut self, len: u32) {
        ffi::WS_Release(self, len);
    }
}

#[cfg(test)]
impl ffi::ws {
    const ALIGN: usize = align_of::<*const c_void>();
    pub(crate) unsafe fn alloc(&mut self, size: u32) -> *mut c_void {
        // `WS_Alloc` is a private part of `varnishd`, not the Varnish library,
        // so it is only available if the output is a `cdylib`.
        // When testing, VMOD is a lib or a bin,
        // so we have to fake our own allocator.
        let ws = validate_ws(self);
        assert!(size > 0);
        let aligned_sz = (size as usize).div_ceil(Self::ALIGN) * Self::ALIGN;
        if ws.e.offset_from(ws.f) < aligned_sz as isize {
            ptr::null_mut()
        } else {
            let p = ws.f.cast::<c_void>();
            ws.f = ws.f.add(aligned_sz);
            assert!(p.is_aligned());
            p
        }
    }

    #[allow(clippy::unused_self)]
    pub(crate) unsafe fn reserve_all(&mut self) -> u32 {
        let ws = validate_ws(self);
        assert!(ws.r.is_null());
        ws.r = ws.e;
        ws.e.offset_from(ws.f).try_into().unwrap()
    }

    #[allow(clippy::unused_self)]
    pub(crate) unsafe fn release(&mut self, size: u32) {
        let ws = validate_ws(self);
        assert!(isize::try_from(size).unwrap() <= ws.e.offset_from(ws.f));
        assert!(isize::try_from(size).unwrap() <= ws.r.offset_from(ws.f));
        assert!(!ws.r.is_null());
        let aligned_sz = usize::try_from(size).unwrap().div_ceil(Self::ALIGN) * Self::ALIGN;
        ws.f = ws.f.add(aligned_sz);
        assert!(ws.f.is_aligned());
        ws.r = ptr::null_mut::<c_char>();
    }
}

/// A workspace object
///
/// Used to allocate memory in an efficient manner, data will live there until the end of the
/// transaction and the workspace is wiped, so there's no need to free the objects living in it.
///
/// The workspace is usually a few tens of kilobytes large, don't be greedy. If you need more
/// space, consider storing your data in a `#[shared_per_task]` or `#[shared_per_vcl]` objects.
#[derive(Debug)]
pub struct Workspace<'ctx> {
    /// Raw pointer to the C struct
    pub raw: *mut ffi::ws,
    _phantom: PhantomData<&'ctx ()>,
}

impl<'ctx> Workspace<'ctx> {
    /// Wrap a raw pointer into an object we can use.
    pub(crate) fn from_ptr(raw: *mut ffi::ws) -> Self {
        assert!(!raw.is_null(), "raw pointer was null");
        Self {
            raw,
            _phantom: PhantomData,
        }
    }

    /// Allocate a buffer of a given size.
    ///
    /// # Safety
    /// Allocated memory is not initialized.
    pub unsafe fn alloc(&mut self, size: NonZeroUsize) -> *mut c_void {
        validate_ws(self.raw).alloc(size.get() as u32)
    }

    /// Check if a pointer is part of the current workspace
    pub fn contains(&self, data: &[u8]) -> bool {
        #[cfg(varnishsys_6)]
        {
            let last = match data.last() {
                None => data.as_ptr(),
                Some(p) => p as *const _,
            };
            unsafe { WS_Inside(self.raw, data.as_ptr().cast(), last.cast()) == 1 }
        }
        #[cfg(not(varnishsys_6))]
        {
            unsafe { WS_Allocated(self.raw, data.as_ptr().cast(), data.len() as isize) == 1 }
        }
    }

    /// Allocate `[u8; size]` array on Workspace.
    /// Returns a reference to uninitialized buffer, or an out of memory error.
    pub fn allocate(
        &mut self,
        size: NonZeroUsize,
    ) -> Result<&'ctx mut [MaybeUninit<u8>], VclError> {
        let ptr = unsafe { self.alloc(size) };
        if ptr.is_null() {
            Err(VclError::WsOutOfMemory(size))
        } else {
            Ok(unsafe { from_raw_parts_mut(ptr.cast(), size.get()) })
        }
    }

    /// Allocate `[u8; size]` array on Workspace, and zero it.
    pub fn allocate_zeroed(&mut self, size: NonZeroUsize) -> Result<&'ctx mut [u8], VclError> {
        let buf = self.allocate(size)?;
        unsafe {
            buf.as_mut_ptr().write_bytes(0, buf.len());
            Ok(slice_assume_init_mut(buf))
        }
    }

    /// Allocate memory on Workspace, and move a value into it.
    /// The value will be dropped in case of out of memory error.
    pub(crate) fn copy_value<T>(&mut self, value: T) -> Result<&'ctx mut T, VclError> {
        let size = NonZeroUsize::new(size_of::<T>())
            .unwrap_or_else(|| panic!("Type {} has sizeof=0", type_name::<T>()));

        let val = unsafe { self.alloc(size).cast::<T>().as_mut() };
        let val = val.ok_or(VclError::WsOutOfMemory(size))?;
        *val = value;
        Ok(val)
    }

    /// Copy any `AsRef<[u8]>` into the workspace
    fn copy_bytes(&mut self, src: impl AsRef<[u8]>) -> Result<&'ctx [u8], VclError> {
        // Re-implement unstable `maybe_uninit_write_slice` and `maybe_uninit_slice`
        // See https://github.com/rust-lang/rust/issues/79995
        // See https://github.com/rust-lang/rust/issues/63569
        let src = src.as_ref();
        let Some(len) = NonZeroUsize::new(src.len()) else {
            Err(VclError::CStr(c"Unable to allocate 0 bytes in a Workspace"))?
        };
        let dest = self.allocate(len)?;
        dest.copy_from_slice(maybe_uninit(src));
        Ok(unsafe { slice_assume_init_mut(dest) })
    }

    /// Copy any `AsRef<[u8]>` into a new [`VCL_BLOB`] stored in the workspace
    #[cfg(not(varnishsys_6))]
    pub fn copy_blob(&mut self, value: impl AsRef<[u8]>) -> Result<VCL_BLOB, VclError> {
        let buf = self.copy_bytes(value)?;
        let blob = self.copy_value(vrt_blob {
            blob: ptr::from_ref(buf).cast::<c_void>(),
            len: buf.len(),
            ..Default::default()
        })?;
        Ok(VCL_BLOB(ptr::from_ref(blob)))
    }

    /// Copy any `AsRef<CStr>` into a new [`txt`] stored in the workspace
    pub fn copy_txt(&mut self, value: impl AsRef<CStr>) -> Result<txt, VclError> {
        let dest = self.copy_bytes(value.as_ref().to_bytes_with_nul())?;
        Ok(bytes_with_nul_to_txt(dest))
    }

    /// Copy any `AsRef<CStr>` into a new [`VCL_STRING`] stored in the workspace
    pub fn copy_cstr(&mut self, value: impl AsRef<CStr>) -> Result<VCL_STRING, VclError> {
        Ok(VCL_STRING(self.copy_txt(value)?.b))
    }

    /// Same as [`Workspace::copy_blob`], copying bytes into Workspace, but treats bytes
    /// as a string with an optional NULL character at the end.  A `NULL` is added if it is missing.
    /// Returns an error if `src` contain NULL characters in a non-last position.
    pub fn copy_bytes_with_null(&mut self, src: impl AsRef<[u8]>) -> Result<txt, VclError> {
        let src = src.as_ref();
        match memchr(0, src) {
            Some(pos) if pos + 1 == src.len() => {
                // Safe because there is only one NULL at the end of the buffer.
                self.copy_txt(unsafe { CStr::from_bytes_with_nul_unchecked(src) })
            }
            Some(_) => Err(VclError::CStr(c"NULL byte found in the source string")),
            None => {
                // NUL byte not found, add one at the end
                // Similar to copy_bytes above
                let len = src.len();
                let dest = self.allocate(unsafe { NonZeroUsize::new_unchecked(len + 1) })?;
                dest[..len].copy_from_slice(maybe_uninit(src));
                dest[len].write(b'\0');
                let dest = unsafe { slice_assume_init_mut(dest) };
                Ok(bytes_with_nul_to_txt(dest))
            }
        }
    }

    /// Allocate workspace free memory as a string buffer until [`WsStrBuffer::finish()`]
    /// is called, resulting in an unsafe [`VCL_STRING`] that can be returned to Varnish.
    /// Note that it is possible for the returned buf size to be zero, which
    /// would result in a zero-length nul-terminated [`VCL_STRING`] if finished.
    pub fn vcl_string_builder(&mut self) -> VclResult<WsStrBuffer<'ctx>> {
        unsafe { WsStrBuffer::new(validate_ws(self.raw)) }
    }

    /// Allocate workspace free memory as a byte buffer until [`WsBlobBuffer::finish()`]
    /// is called, resulting in an unsafe [`VCL_BLOB`] that can be returned to Varnish.
    #[cfg(not(varnishsys_6))]
    pub fn vcl_blob_builder(&mut self) -> VclResult<WsBlobBuffer<'ctx>> {
        unsafe { WsBlobBuffer::new(validate_ws(self.raw)) }
    }

    /// Allocate workspace free memory as a temporary vector-like buffer
    /// until [`WsTempBuffer::finish()`] is called.  The buffer is not intended
    /// to be returned to Varnish, but may be shared among context users.
    /// The buffer is returned as a `&'ws [T]` to allow mutable access,
    /// while tying the lifetime to the workspace.
    pub fn slice_builder<T: Copy>(&mut self) -> VclResult<WsTempBuffer<'ctx, T>> {
        unsafe { WsTempBuffer::new(validate_ws(self.raw)) }
    }
}

/// Internal helper to convert a `&[u8]` to a `&[MaybeUninit<u8>]`
fn maybe_uninit(value: &[u8]) -> &[MaybeUninit<u8>] {
    // SAFETY: &[T] and &[MaybeUninit<T>] have the same layout
    // This was copied from MaybeUninit::copy_from_slice, ignoring clippy lints
    unsafe {
        #[expect(clippy::transmute_ptr_to_ptr)]
        transmute(value)
    }
}

/// Internal helper to convert a `&mut [MaybeUninit<u8>]` to a `&[u8]`, assuming all elements are initialized
unsafe fn slice_assume_init_mut(value: &mut [MaybeUninit<u8>]) -> &mut [u8] {
    // SAFETY: Valid elements have just been copied into `this` so it is initialized
    // This was copied from MaybeUninit::slice_assume_init_mut, ignoring clippy lints
    #[expect(clippy::ref_as_ptr)]
    &mut *(value as *mut [MaybeUninit<u8>] as *mut [u8])
}

/// Helper to convert a byte slice with a null terminator to a `txt` struct.
fn bytes_with_nul_to_txt(buf: &[u8]) -> txt {
    txt::from_cstr(unsafe { CStr::from_bytes_with_nul_unchecked(buf) })
}

/// A struct holding both a native workspace struct and the space it points to.
///
/// As the name implies, this struct mainly exist to facilitate testing and should probably not be
/// used elsewhere.
#[derive(Debug)]
pub struct TestWS {
    c_ws: ffi::ws,
    #[expect(dead_code)]
    space: Vec<c_char>,
}

impl TestWS {
    /// Instantiate a `C` ws struct and the required space of size `sz`.
    pub fn new(sz: usize) -> Self {
        let al = align_of::<*const c_void>();
        let aligned_sz = (sz / al) * al;
        let mut space: Vec<c_char> = vec![0; sz];
        let s = space.as_mut_ptr();
        assert!(s.is_aligned());
        assert!(unsafe { s.add(aligned_sz).is_aligned() });
        Self {
            c_ws: ffi::ws {
                magic: ffi::WS_MAGIC,
                id: ['t' as c_char, 's' as c_char, 't' as c_char, '\0' as c_char],
                s,
                f: s,
                r: ptr::null_mut(),
                e: unsafe { s.add(aligned_sz) },
            },
            space,
        }
    }

    /// Return a pointer to the underlying C ws struct. As usual, the caller needs to ensure that
    /// self doesn't outlive the returned pointer.
    pub fn as_ptr(&mut self) -> *mut ffi::ws {
        ptr::from_mut::<ffi::ws>(&mut self.c_ws)
    }

    /// build a `Workspace`
    pub fn workspace(&mut self) -> Workspace<'_> {
        Workspace::from_ptr(self.as_ptr())
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use super::*;

    #[test]
    fn ws_test_alloc() {
        let mut test_ws = TestWS::new(160);
        let mut ws = test_ws.workspace();
        for _ in 0..10 {
            unsafe {
                assert!(!ws.alloc(NonZero::new(16).unwrap()).is_null());
            }
        }
        unsafe {
            assert!(ws.alloc(NonZero::new(1).unwrap()).is_null());
        }
    }
}
