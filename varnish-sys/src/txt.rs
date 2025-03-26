use std::ffi::{c_char, CStr};
use std::slice::from_raw_parts;
use std::str::from_utf8;

use crate::ffi::txt;

impl txt {
    /// Internal helper to create a `txt` struct from a byte slice.
    /// The entire slice is assumed to not contain any null bytes.
    fn from_bytes(s: &[u8]) -> Self {
        Self {
            b: s.as_ptr().cast::<c_char>(),
            e: unsafe { s.as_ptr().add(s.len()).cast::<c_char>() },
        }
    }

    /// FIXME: This method is only used when calling [`crate::ffi::VSLbt`],
    /// and current implementation creates a string without a null terminator to pass it in.
    /// Going forward, we should probably refactor it to avoid extra string allocation.
    #[expect(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        Self::from_bytes(s.as_bytes())
    }

    pub fn from_cstr(s: &CStr) -> Self {
        Self::from_bytes(s.to_bytes())
    }

    /// Convert the `txt` struct to a `&[u8]`.
    /// We want to explicitly differentiate between empty (`None`) and null (`Some([])`) strings.
    #[expect(clippy::wrong_self_convention)] // TODO: drop Copy trait for txt?
    pub fn to_slice<'a>(&self) -> Option<&'a [u8]> {
        if self.b.is_null() {
            None
        } else {
            // SAFETY: We assume that txt instance was created correctly,
            //         so the pointers are valid and the end is after the beginning.
            //         Txt instances are part of ffi, so inherently unsafe.
            unsafe {
                Some(from_raw_parts(
                    self.b.cast::<u8>(),
                    self.e.offset_from(self.b) as usize,
                ))
            }
        }
    }

    /// Convert the `txt` struct to a `&str`.
    /// Returns None if the string is not valid UTF-8.
    /// Use [Self::to_slice] to handle raw bytes.
    #[expect(clippy::wrong_self_convention)] // TODO: drop Copy trait for txt?
    pub fn to_str<'a>(&self) -> Option<&'a str> {
        self.to_slice().and_then(|s| from_utf8(s).ok())
    }

    /// Parse the `txt` struct as a header, returning a tuple with the key and value,
    /// trimming the value of leading whitespace. Returns None in case of any issues.
    /// Use [Self::to_slice] to handle raw bytes, e.g. if the header is not UTF-8.
    pub fn parse_header<'a>(&self) -> Option<(&'a str, &'a str)> {
        let (key, value) = self.to_str()?.split_once(':')?;
        // FIXME: Consider `.trim_ascii_start()` if unicode is not a concern
        Some((key, value.trim_start()))
    }
}
