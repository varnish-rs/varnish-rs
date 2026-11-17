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

#[cfg(not(feature = "full"))]
use std::ffi::c_char;
use std::ffi::CStr;
use std::fmt::Debug;
use std::marker::PhantomData;
#[cfg(not(feature = "full"))]
use std::num::NonZeroUsize;

use memchr::memchr;

use crate::ffi;
#[cfg(not(feature = "full"))]
use crate::ffi::VCL_STRANDS;
use crate::ffi::{txt, VCL_STRING};
use crate::vcl::VclError;

#[cfg(feature = "full")]
pub use crate::vcl::ws_str_buffer::{WsBlobBuffer, WsBuffer, WsStrBuffer, WsTempBuffer};
#[cfg(feature = "full")]
pub use full::TestWS;

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

impl Workspace<'_> {
    /// Wrap a raw pointer into an object we can use.
    pub(crate) fn from_ptr(raw: *mut ffi::ws) -> Self {
        assert!(!raw.is_null(), "raw pointer was null");
        Self {
            raw,
            _phantom: PhantomData,
        }
    }
}

// String copying: `full` mode copies with `WS_Alloc` (cache.h-only). vrt mode only has
// `VRT_StrandsWS`, which builds the string with `WS_VSB_new` and therefore fails once the
// workspace overflow flag is set, even if space is left (the same as VCL's own string handling).
// `VRT_StrandsWS` only resolves inside varnishd, so vrt mode copies can't be exercised by a
// plain `cargo test` binary the way the `full` ones are (via the `#[cfg(test)]` fake `ffi::ws`
// impl in the `full` module below). Verify via a `.vtc` test instead.
impl Workspace<'_> {
    /// Copy any `AsRef<CStr>` into a new [`txt`] stored in the workspace
    ///
    /// Without the `full` feature, this fails once the workspace has overflowed (e.g. after a
    /// failed allocation), even if enough space is left.
    pub fn copy_txt(&mut self, value: impl AsRef<CStr>) -> Result<txt, VclError> {
        #[cfg(feature = "full")]
        return self.copy_cstr_ws_alloc(value.as_ref());
        #[cfg(not(feature = "full"))]
        return self.copy_cstr_strands(value.as_ref());
    }

    /// Copy any `AsRef<CStr>` into a new [`VCL_STRING`] stored in the workspace
    pub fn copy_cstr(&mut self, value: impl AsRef<CStr>) -> Result<VCL_STRING, VclError> {
        Ok(VCL_STRING(self.copy_txt(value)?.b))
    }

    /// Copy bytes into the workspace as a NUL-terminated string, treating bytes as already
    /// having an optional NULL character at the end. A `NULL` is added if it is missing.
    /// Returns an error if `src` contains NULL characters in a non-last position.
    pub fn copy_bytes_with_null(&mut self, src: impl AsRef<[u8]>) -> Result<txt, VclError> {
        let src = src.as_ref();
        match memchr(0, src) {
            Some(pos) if pos + 1 == src.len() => {
                // Safe because there is only one NULL at the end of the buffer.
                self.copy_txt(unsafe { CStr::from_bytes_with_nul_unchecked(src) })
            }
            Some(_) => Err(VclError::CStr(c"NULL byte found in the source string")),
            #[cfg(feature = "full")]
            None => self.copy_bytes_add_null(src),
            #[cfg(not(feature = "full"))]
            None => {
                // NUL byte not found: stage a temporary (non-workspace) copy with one
                // appended, since `VRT_StrandsWS` needs a NUL-terminated input string.
                let mut staged = Vec::with_capacity(src.len() + 1);
                staged.extend_from_slice(src);
                staged.push(0);
                self.copy_cstr_strands(unsafe { CStr::from_bytes_with_nul_unchecked(&staged) })
            }
        }
    }

    #[cfg(not(feature = "full"))]
    fn copy_cstr_strands(&mut self, cstr: &CStr) -> Result<txt, VclError> {
        // `VRT_StrandsWS` only reads `strands` for the duration of the call — it never
        // retains the descriptor itself, only copies the string data into the workspace —
        // so the descriptor can live on the Rust stack instead of costing a permanent
        // `VRT_AllocStrandsWS` workspace allocation.
        let mut p: *const c_char = cstr.as_ptr();
        let strands = ffi::strands {
            magic: ffi::STRANDS_MAGIC,
            n: 1,
            p: &raw mut p,
        };
        // `h` must be non-NULL: with `h == NULL` and a single non-empty strand,
        // `VRT_StrandsWS` may return that strand's pointer as-is instead of copying it
        // (Varnish 9.1 does so unconditionally; 8.0/9.0 only when it is already in the
        // workspace), leaving the caller with a pointer into Rust-owned memory. An empty
        // `h` adds nothing to the result but always forces a copy of a non-empty string;
        // an empty one yields `h` itself (a static) or a workspace copy, both valid.
        unsafe {
            let vcl_str =
                ffi::VRT_StrandsWS(self.raw, c"".as_ptr(), VCL_STRANDS(&raw const strands));
            if vcl_str.0.is_null() {
                return Err(VclError::WsOutOfMemory(out_of_memory_size(cstr)));
            }
            Ok(txt::from_cstr(CStr::from_ptr(vcl_str.0)))
        }
    }
}

#[cfg(not(feature = "full"))]
fn out_of_memory_size(cstr: &CStr) -> NonZeroUsize {
    NonZeroUsize::new(cstr.to_bytes().len() + 1).expect("len + 1 is never zero")
}

// Everything below needs the concrete (cache.h-only) `struct ws` layout — `WS_Alloc`,
// `WS_ReserveAll`, `WS_Release`, `WS_Allocated` — unavailable under the vrt-only surface.
// Grouped into one module so the `full` gate isn't repeated on every item.
#[cfg(feature = "full")]
mod full {
    use std::any::type_name;
    use std::ffi::{c_char, c_void, CStr};
    use std::mem::{align_of, size_of, transmute, MaybeUninit};
    use std::num::NonZeroUsize;
    use std::ptr;
    use std::slice::from_raw_parts_mut;

    #[allow(unused_imports)] // only used by the `vcl_string_builder` intra-doc link below
    use crate::ffi::{self, txt, vrt_blob, WS_Allocated, VCL_BLOB, VCL_STRING};
    use crate::validate_ws;
    use crate::vcl::{VclError, VclResult};

    use super::{WsBlobBuffer, WsStrBuffer, WsTempBuffer};

    use super::Workspace;

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
            ws.e.offset_from(ws.f)
                .try_into()
                .expect("workspace free space must fit in u32")
        }

        #[allow(clippy::unused_self)]
        pub(crate) unsafe fn release(&mut self, size: u32) {
            let ws = validate_ws(self);
            assert!(
                isize::try_from(size).expect("workspace size must fit in isize")
                    <= ws.e.offset_from(ws.f)
            );
            assert!(
                isize::try_from(size).expect("workspace size must fit in isize")
                    <= ws.r.offset_from(ws.f)
            );
            assert!(!ws.r.is_null());
            let aligned_sz = usize::try_from(size)
                .expect("workspace size must fit in usize")
                .div_ceil(Self::ALIGN)
                * Self::ALIGN;
            ws.f = ws.f.add(aligned_sz);
            assert!(ws.f.is_aligned());
            ws.r = ptr::null_mut::<c_char>();
        }
    }

    impl<'ctx> Workspace<'ctx> {
        /// Allocate a buffer of a given size.
        ///
        /// # Safety
        /// Allocated memory is not initialized.
        pub unsafe fn alloc(&mut self, size: NonZeroUsize) -> *mut c_void {
            validate_ws(self.raw).alloc(size.get() as u32)
        }

        /// Check if a pointer is part of the current workspace
        pub fn contains(&self, data: &[u8]) -> bool {
            unsafe { WS_Allocated(self.raw, data.as_ptr().cast(), data.len() as isize) == 1 }
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

        /// `full` implementation of [`Workspace::copy_txt`]: a plain `WS_Alloc` copy, which,
        /// unlike `VRT_StrandsWS`, still works after an earlier allocation overflowed.
        pub(super) fn copy_cstr_ws_alloc(&mut self, value: &CStr) -> Result<txt, VclError> {
            let dest = self.copy_bytes(value.to_bytes_with_nul())?;
            Ok(bytes_with_nul_to_txt(dest))
        }

        /// `full` implementation of [`Workspace::copy_bytes_with_null`] for `src` without NUL.
        pub(super) fn copy_bytes_add_null(&mut self, src: &[u8]) -> Result<txt, VclError> {
            let len = src.len();
            let dest = self.allocate(unsafe { NonZeroUsize::new_unchecked(len + 1) })?;
            dest[..len].copy_from_slice(maybe_uninit(src));
            dest[len].write(b'\0');
            let dest = unsafe { slice_assume_init_mut(dest) };
            Ok(bytes_with_nul_to_txt(dest))
        }

        /// Copy any `AsRef<[u8]>` into a new [`VCL_BLOB`] stored in the workspace
        ///
        /// Only available under `full`: the vrt-native `VRT_blob()` needs a `VRT_CTX`, not just a
        /// `Workspace`, so it doesn't fit this method's signature — no vrt-mode equivalent yet.
        pub fn copy_blob(&mut self, value: impl AsRef<[u8]>) -> Result<VCL_BLOB, VclError> {
            let buf = self.copy_bytes(value)?;
            let blob = self.copy_value(vrt_blob {
                magic: ffi::VRT_BLOB_MAGIC,
                blob: ptr::from_ref(buf).cast::<c_void>(),
                len: buf.len(),
                ..Default::default()
            })?;
            Ok(VCL_BLOB(ptr::from_ref(blob)))
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

    fn bytes_with_nul_to_txt(buf: &[u8]) -> txt {
        txt::from_cstr(unsafe { CStr::from_bytes_with_nul_unchecked(buf) })
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
        &mut *(ptr::from_mut::<[MaybeUninit<u8>]>(value) as *mut [u8])
    }

    /// A struct holding both a native workspace struct and the space it points to.
    ///
    /// As the name implies, this struct mainly exist to facilitate testing and should probably not be
    /// used elsewhere.
    ///
    /// Only available under `full`: it needs a concrete `struct ws` layout, unavailable under the
    /// vrt-only surface.
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
                    assert!(!ws
                        .alloc(NonZero::new(16).expect("16 is non-zero"))
                        .is_null());
                }
            }
            unsafe {
                assert!(ws.alloc(NonZero::new(1).expect("1 is non-zero")).is_null());
            }
        }

        #[test]
        fn ws_test_copy_after_failed_alloc() {
            let mut test_ws = TestWS::new(160);
            let mut ws = test_ws.workspace();
            // A failed allocation must not prevent later, smaller copies.
            assert!(ws.allocate(NonZero::new(1000).expect("non-zero")).is_err());
            let txt = ws.copy_txt(c"hello").expect("copy_txt");
            assert_eq!(txt.to_slice(), Some(b"hello".as_slice()));
            let txt = ws
                .copy_bytes_with_null(b"abc")
                .expect("copy_bytes_with_null");
            assert_eq!(txt.to_slice(), Some(b"abc".as_slice()));
            let txt = ws
                .copy_bytes_with_null(b"def\0")
                .expect("copy_bytes_with_null");
            assert_eq!(txt.to_slice(), Some(b"def".as_slice()));
            assert!(ws.copy_bytes_with_null(b"a\0b").is_err());
        }
    }
}
