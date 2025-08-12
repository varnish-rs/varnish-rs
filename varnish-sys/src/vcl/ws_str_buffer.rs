use std::io::Write;
use std::marker::PhantomData;
use std::num::NonZeroUsize;
use std::ops::{Add, Rem};
use std::slice::from_raw_parts_mut;
use std::{io, mem, ptr};

use crate::ffi;
use crate::ffi::VCL_STRING;
#[cfg(not(varnishsys_6))]
use crate::ffi::{vrt_blob, VCL_BLOB};
use crate::vcl::VclError::WsOutOfMemory;
use crate::vcl::VclResult;

/// The free region of the workspace that functions as a "resizable" vector, up to the end of the workspace.
/// The buffer must be finalized using `finish()` to avoid being reclaimed when dropped.
#[derive(Debug)]
pub struct WsBuffer<'ws, Item, Suffix, Output> {
    /// The workspace pointer, used to release the workspace
    ws: &'ws mut ffi::ws,
    /// The start of the writable buffer, aligned to the content type. Will set to null when finished.
    start_items: *mut Item,
    /// The reserved buffer will move its start as we write to it, thus becoming "used"
    unused: &'ws mut [Item],

    _output: PhantomData<Output>,
    _suffix: PhantomData<Suffix>,
}

pub type WsStrBuffer<'ws> = WsBuffer<'ws, u8, u8, VCL_STRING>;
#[cfg(not(varnishsys_6))]
pub type WsBlobBuffer<'ws> = WsBuffer<'ws, u8, vrt_blob, VCL_BLOB>;
pub type WsTempBuffer<'ws, T> = WsBuffer<'ws, T, (), &'ws [T]>;

impl<Item, Suffix, Output> AsRef<[Item]> for WsBuffer<'_, Item, Suffix, Output> {
    /// Get the written data as a slice
    fn as_ref(&self) -> &[Item] {
        unsafe { std::slice::from_raw_parts(self.start_items, self.len()) }
    }
}

impl<Item, Suffix, Output> AsMut<[Item]> for WsBuffer<'_, Item, Suffix, Output> {
    /// Get the writable buffer as a slice
    fn as_mut(&mut self) -> &mut [Item] {
        unsafe { from_raw_parts_mut(self.start_items, self.len()) }
    }
}

impl<'ws, Item: Copy, Suffix, Output> WsBuffer<'ws, Item, Suffix, Output> {
    /// Internal method to create a new buffer
    pub(crate) unsafe fn new(ws: &'ws mut ffi::ws) -> VclResult<Self> {
        let reserved_space = ws.reserve_all() as usize;
        let raw_start = get_raw_start(ws);

        // Compute the size of the alignment, usually compile-time zero, but just in case
        let items_align = raw_start.align_offset(align_of::<Item>());

        // Computes how many bytes need to be reserved for the suffix
        let end = raw_start.add(reserved_space);
        // last byte where Suffix can start and fit entirely,
        // rounded down to the alignment
        let suffix_ptr = end.sub(size_of::<Suffix>());
        let suffix_ptr = suffix_ptr.sub((suffix_ptr as usize).rem(align_of::<Suffix>()));
        debug_assert!(suffix_ptr.is_aligned(), "suffix_ptr is not aligned");
        let suffix_size = end.offset_from(suffix_ptr);
        let suffix_size = usize::try_from(suffix_size).expect("invalid suffix size");

        let items_start = raw_start.add(items_align).cast::<Item>().cast_mut();
        debug_assert!(items_start.is_aligned(), "WS buffer is not aligned");

        // Minimum space we need to function properly. A zero-length null-terminated C-string is valid.
        let required = if size_of::<Suffix>() > 0 {
            // With the suffix, it's enough to have space for the suffix itself, i.e. `\0`
            items_align + suffix_size
        } else {
            // Without the suffix, require space for at least one item
            items_align + size_of::<Item>()
        };

        if reserved_space < required {
            return Err(WsOutOfMemory(NonZeroUsize::new_unchecked(required)));
        }

        let len = (reserved_space - items_align - suffix_size) / Self::ITEM_SIZE;

        Ok(WsBuffer {
            ws,
            start_items: items_start,
            unused: from_raw_parts_mut(items_start, len),
            _output: PhantomData,
            _suffix: PhantomData,
        })
    }
}

impl<Item, Suffix, Output> WsBuffer<'_, Item, Suffix, Output> {
    const ITEM_SIZE: usize = size_of::<Item>();
    const _ITEM_SIZE_CHECK: () = assert!(
        Self::ITEM_SIZE >= size_of::<u8>(),
        "size_of::<T>() must be at least 1 byte"
    );

    /// Release the workspace, reclaiming the memory except for the written data.
    ///
    /// Safety:
    ///     This must be the last call before dropping. It may be called multiple times.
    unsafe fn release(&mut self, preserve: bool) {
        let start = mem::replace(&mut self.start_items, ptr::null_mut());
        if !start.is_null() {
            let preserve_bytes = if preserve {
                // compute total bytes used by the buffer
                usize::try_from(
                    self.get_suffix_ptr()
                        .cast::<u8>()
                        .offset_from(get_raw_start(self.ws)),
                )
                .expect("used_bytes overflow")
                .add(size_of::<Suffix>())
                .try_into()
                .expect("preserve_bytes overflow")
            } else {
                0
            };

            self.ws.release(preserve_bytes);
        }
    }

    /// Internal method to calculate the length of the written data
    fn calc_len(start: *const Item, buffer: &[Item]) -> usize {
        unsafe {
            let len = buffer.as_ptr().offset_from(start);
            debug_assert!(len >= 0, "len={len} is negative");
            len as usize
        }
    }

    /// Get the length of the written data
    pub fn len(&self) -> usize {
        Self::calc_len(self.start_items, self.unused)
    }

    /// Check if anything has been written to the buffer
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the remaining capacity of the buffer
    pub fn remaining(&self) -> usize {
        self.unused.len()
    }

    pub fn push(&mut self, item: Item) -> VclResult<()> {
        if self.unused.is_empty() {
            return Err(WsOutOfMemory(NonZeroUsize::MIN));
        }
        unsafe {
            let end = self.unused.as_mut_ptr();
            ptr::write(end, item);
            self.unused = from_raw_parts_mut(end.add(1), self.unused.len() - 1);
        }
        Ok(())
    }

    pub fn extend_from_slice(&mut self, slice: &[Item]) -> VclResult<()> {
        if self.unused.len() < slice.len() {
            return Err(WsOutOfMemory(NonZeroUsize::new(slice.len()).unwrap()));
        }
        unsafe {
            let end = self.unused.as_mut_ptr();
            ptr::copy_nonoverlapping(slice.as_ptr(), end, slice.len());
            self.unused = from_raw_parts_mut(end.add(slice.len()), self.unused.len() - slice.len());
        }
        Ok(())
    }

    /// Get the pointer to the allowed suffix location right after currently used data.
    unsafe fn get_suffix_ptr(&self) -> *mut Suffix {
        let ptr_unused = self.unused.as_ptr();
        let offset = ptr_unused.align_offset(align_of::<Suffix>());
        ptr_unused.add(offset).cast::<Suffix>().cast_mut()
    }
}

impl<Output, Suffix> Write for WsBuffer<'_, u8, Suffix, Output> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.unused.write(data)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<Item, Suffix, Output> Drop for WsBuffer<'_, Item, Suffix, Output> {
    /// Ignore all the write commands, reclaiming the workspace memory
    fn drop(&mut self) {
        unsafe { self.release(false) };
    }
}

impl WsStrBuffer<'_> {
    /// Finish writing to the [`WsBuffer`], returning the allocated [`VCL_STRING`].
    pub fn finish(mut self) -> VCL_STRING {
        unsafe {
            // SAFETY:
            // Since we reserved one extra byte for the NUL terminator,
            // we can force write the NUL terminator even past the end of the slice.
            self.unused.as_mut_ptr().write(b'\0');

            // Must get the result before releasing the workspace, as it updates the pointer
            let result = get_raw_start(self.ws).cast();

            // Reserve written data including the NUL terminator, and release the rest
            self.release(true);

            VCL_STRING(result)
        }
    }
}

#[cfg(not(varnishsys_6))]
impl WsBlobBuffer<'_> {
    /// Finish writing to the [`WsBlobBuffer`], returning the allocated [`VCL_BLOB`].
    pub fn finish(mut self) -> VCL_BLOB {
        unsafe {
            let data = self.as_ref();
            // Create vrt_blob suffix right after the data
            let suffix_ptr = self.get_suffix_ptr();
            suffix_ptr.write(vrt_blob {
                blob: data.as_ptr().cast(),
                len: data.len(),
                ..Default::default()
            });

            self.release(true);

            VCL_BLOB(suffix_ptr)
        }
    }
}

impl<'ws, T> WsTempBuffer<'ws, T> {
    /// Finish writing to the [`WsTempBuffer`], returning the allocated `&'ws [T]`.
    pub fn finish(mut self) -> &'ws [T] {
        unsafe {
            // force lifetime to be 'ws because we are dropping self
            let data = mem::transmute::<&[T], &'ws [T]>(self.as_ref());
            self.release(true);
            data
        }
    }
}

fn get_raw_start(ws: &ffi::ws) -> *const u8 {
    ws.f.cast::<u8>()
}

#[cfg(test)]
mod tests {
    use std::ffi::{CStr, CString};

    use super::*;
    use crate::vcl::TestWS;

    fn get_cstr(s: &VCL_STRING) -> &CStr {
        unsafe { CStr::from_ptr(s.0) }
    }

    fn round_up_to_usize(number: usize) -> usize {
        number.div_ceil(size_of::<usize>()) * size_of::<usize>()
    }

    #[cfg(not(varnishsys_6))]
    fn buf_to_vec(buf: WsBlobBuffer<'_>) -> &'_ [u8] {
        let data = buf.finish();
        let vrt_blob { blob, len, .. } = unsafe { *(data.0) };
        unsafe { std::slice::from_raw_parts(blob.cast::<u8>(), len) }
    }

    #[test]
    fn str_buffer() {
        let mut test_ws = TestWS::new(160);
        let mut ws = test_ws.workspace();

        // first buffer call gets all available space
        let mut buf = ws.vcl_string_builder().unwrap();
        assert_eq!(buf.remaining(), 159);
        buf.write_all(b"0123456789").unwrap();
        assert_eq!(buf.remaining(), 149);
        // saving 10 bytes + nul
        assert_eq!(get_cstr(&buf.finish()), c"0123456789");

        let mut buf = ws.vcl_string_builder().unwrap();
        assert_eq!(buf.remaining(), 160 - round_up_to_usize(10 + 1) - 1);
        write!(buf, "this data is ignored").unwrap();
        // the ReservedBuf goes out of scope without a call to .finish()
        // so now data is fully allocated
        drop(buf);

        let mut buf = ws.vcl_string_builder().unwrap();
        assert_eq!(buf.remaining(), 160 - round_up_to_usize(10 + 1) - 1);
        let fill = vec![b'x'; buf.remaining() - 1];
        buf.write_all(&fill).unwrap();
        assert_eq!(buf.remaining(), 1);
        assert_eq!(
            get_cstr(&buf.finish()),
            CString::new(fill).unwrap().as_c_str()
        );

        assert!(matches!(ws.vcl_string_builder(), Err(WsOutOfMemory(_))));

        // Will to the end of the buffer
        let mut test_ws = TestWS::new(160);
        let mut ws = test_ws.workspace();
        let mut buf = ws.vcl_string_builder().unwrap();
        assert_eq!(buf.remaining(), 159);
        let fill = vec![b'x'; buf.remaining()];
        buf.write_all(&fill).unwrap();
        assert_eq!(buf.remaining(), 0);
        assert_eq!(
            get_cstr(&buf.finish()),
            CString::new(fill).unwrap().as_c_str()
        );

        assert!(matches!(ws.vcl_string_builder(), Err(WsOutOfMemory(_))));
    }

    #[test]
    #[cfg(not(varnishsys_6))]
    fn blob_buffer() {
        assert_eq!(size_of::<vrt_blob>(), 24);
        assert_eq!(align_of::<vrt_blob>(), 8);

        // init a workspace
        let mut test_ws = TestWS::new(160);
        let mut ws = test_ws.workspace();

        // first buffer call gets all available space
        let mut buf = ws.vcl_blob_builder().unwrap();
        assert_eq!(buf.remaining(), 160 - 24);
        buf.write_all(b"0123456789").unwrap();
        let used = round_up_to_usize(24 + 10);
        let data = buf_to_vec(buf);
        assert_eq!(data, b"0123456789");

        // second reservation without (header + )
        let mut buf = ws.vcl_blob_builder().unwrap();
        assert_eq!(buf.remaining(), 160 - used - 24);
        write!(buf, "this data is ignored").unwrap();
        drop(buf);

        // validate no data corruption
        assert_eq!(data, b"0123456789");

        // the ReservedBuf goes out of scope without a call to .finish()
        // so now data is fully allocated
        let mut buf = ws.vcl_blob_builder().unwrap();
        assert_eq!(buf.remaining(), 160 - used - 24);
        write!(buf, "this data is ignored").unwrap();
    }

    #[repr(C)]
    #[derive(Debug, PartialEq, Clone, Copy)]
    struct TestStruct(u16, u8);

    #[test]
    fn temp_buffer() {
        assert_eq!(4, size_of::<TestStruct>());
        let mut test_ws = TestWS::new(160);
        let mut ws = test_ws.workspace();

        // first buffer call gets all available space
        let mut buf = ws.slice_builder::<TestStruct>().unwrap();
        assert_eq!(buf.remaining(), 160 / 4);
        buf.push(TestStruct(1, 2)).unwrap();
        let used = round_up_to_usize(4);
        let data = buf.finish();
        assert_eq!(data, [TestStruct(1, 2)]);

        // second reservation without (header + )
        let mut buf = ws.slice_builder().unwrap();
        assert_eq!(buf.remaining(), 160 - used);
        write!(buf, "this data is ignored").unwrap();
        drop(buf);

        // validate no data corruption
        assert_eq!(data, [TestStruct(1, 2)]);

        // buf went out of scope without a call to .finish(), discarding it
        let mut buf = ws.slice_builder().unwrap();
        assert_eq!(buf.remaining(), 160 - used);
        buf.extend_from_slice(b"0123456789").unwrap();
        assert_eq!(buf.finish(), b"0123456789");
    }
}
