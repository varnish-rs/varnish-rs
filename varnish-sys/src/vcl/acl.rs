use crate::ffi;

#[derive(Debug, Clone)]
pub struct Acl {
    pub raw: ffi::VCL_ACL,
}

impl Acl {
    /// Retun the `C` pointer to the underlying ACL.
    pub unsafe fn vcl_ptr(&self) -> ffi::VCL_ACL {
        self.raw
    }
}
