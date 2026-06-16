use std::net::SocketAddr;

use crate::ffi;

#[derive(Debug, Clone)]
pub struct Acl {
    pub raw: ffi::VCL_ACL,
}

impl Acl {
    /// Test if the given address matches the ACL.
    pub fn matches(&self, ctx: &crate::vcl::Ctx, addr: SocketAddr) -> bool {
        assert!(!self.raw.0.is_null());

        unsafe {
            let mut sa_buf = vec![0u8; ffi::vsa_suckaddr_len];
            crate::vcl::convert::write_ip_to_buf(addr, &mut sa_buf);
            ffi::VRT_acl_match(ctx.raw, self.raw, ffi::VCL_IP(sa_buf.as_ptr().cast())) == 1
        }
    }

    /// Retun the `C` pointer to the underlying ACL.
    pub unsafe fn vcl_ptr(&self) -> ffi::VCL_ACL {
        self.raw
    }
}
