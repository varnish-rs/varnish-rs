use std::ffi::c_void;

use crate::ffi;

/// A wrapper for scalable/growable buffer (VSB) managed by Varnish
#[derive(Debug)]
pub struct Buffer<'a> {
    /// Raw pointer to the C struct
    pub raw: &'a mut ffi::vsb,
}

impl Buffer<'_> {
    /// Create a `Vsb` from a C pointer
    #[expect(clippy::not_unsafe_ptr_arg_deref)]
    pub fn from_ptr(raw: *mut ffi::vsb) -> Self {
        let raw = unsafe { raw.as_mut().expect("VSB pointer must not be null") };
        assert_eq!(raw.magic, ffi::VSB_MAGIC);
        Self { raw }
    }

    /// Push a buffer into the buffer
    #[expect(clippy::result_unit_err)] // FIXME: what should the error be? VclError?
    pub fn write<T: AsRef<[u8]>>(&mut self, src: &T) -> Result<(), ()> {
        let buf = src.as_ref().as_ptr().cast::<c_void>();
        let l = src.as_ref().len();

        match unsafe { ffi::VSB_bcat(self.raw, buf, l as isize) } {
            0 => Ok(()),
            _ => Err(()),
        }
    }
}
