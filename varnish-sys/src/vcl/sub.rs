use crate::ffi::VCL_SUB;

/// A wrapper around a [`VCL_SUB`] pointer representing a VCL subroutine.
///
/// Subroutines can be passed as arguments to VMOD functions and invoked via
/// [`Ctx::call_sub`] and [`Ctx::check_call_sub`].
#[derive(Debug, Clone, Copy)]
pub struct Subroutine(pub(crate) VCL_SUB);

impl Subroutine {
    /// Return the underlying [`VCL_SUB`] pointer.
    pub fn vcl_ptr(self) -> VCL_SUB {
        self.0
    }
}
