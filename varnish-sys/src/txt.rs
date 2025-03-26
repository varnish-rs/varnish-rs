use std::ffi::{c_char, CStr};
use std::slice::from_raw_parts;
use std::str::from_utf8;

use crate::ffi::txt;
use crate::vcl::StrOrBytes;

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
    pub fn to_slice(&self) -> Option<&[u8]> {
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

    /// Convert the `txt` struct to a `StrOrBytes` enum.
    #[expect(clippy::wrong_self_convention)] // TODO: drop Copy trait for txt?
    pub fn to_str(&self) -> Option<StrOrBytes<'_>> {
        self.to_slice().map(StrOrBytes::from)
    }

    /// Parse the `txt` struct as a header, returning a tuple with the key and value,
    /// trimming the value of leading whitespace.
    pub fn parse_header(&self) -> Option<(&str, StrOrBytes<'_>)> {
        // We expect varnishd to always given us a string with a ':' in it
        // If it's not the case, blow up as it might be a sign of a bigger problem.
        let slice = self.to_slice()?;
        let index = slice
            .iter()
            .position(|c| *c == b':')
            .expect("headers should always have a :");

        let (key_slice, value_slice) = slice.split_at(index);

        Some((
            from_utf8(key_slice).expect("header names must be UTF-8"),
            value_slice[1..].trim_ascii_start().into(),
        ))
    }
}
