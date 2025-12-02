//! Convert Rust types into their VCL_* equivalent, and back
//!
//! # Type conversion
//!
//! The proc macro will generate the wrappers for each user function, relying on
//! the type conversions defined here. The values need to be converted from Varnish's internal types
//! to Rust's types, and vice versa.
//!
//! Most conversions from VCL to Rust are straightforward, using either `From` or `TryFrom` traits.
//! The `IntoVCL` trait take care of converting a Rust type into VCL. It requires a `&mut `[`Workspace`]
//! to possibly store the returned value into the task request. This allows vmod writes to just return
//! easy-to-work-with strings, and let the boilerplate handle the allocation, copy and error handling.
//!
//! If one wants to handle things manually, all `VCL_*` types implement [`IntoVCL`] as a no-op. It
//! can be useful to avoid extra memory allocations by the boilerplate, if that is a worry.
//!
//! Here's a table of the type correspondences:
//!
//! | Rust | direction | VCL |
//! | :--: | :-------: | :-:
//! | `()` | -> | `VCL_VOID` |
//! | `f64`  | <-> | `VCL_REAL` |
//! | `i64`  | <-> | `VCL_INT` |
//! | `bool` | <-> | `VCL_BOOL` |
//! | `std::time::Duration` | <-> | `VCL_DURATION` |
//! | `&str` | <-> | `VCL_STRING` |
//! | `String` | -> | `VCL_STRING` |
//! | `&[u8]` | <- | `VCL_BLOB` |
//! | `Option<CowProbe>` | <-> | `VCL_PROBE` |
//! | `Option<Probe>` | <-> | `VCL_PROBE` |
//! | `Option<std::net::SockAdd>` | -> | `VCL_IP` |
//!
//! For all the other types, which are pointers, you will need to use the native types.
//!
//! *Note:* It is possible to simply return a `VCL_*` type (or a Result<VCL_*, _>), in which case
//! the boilerplate will just skip the conversion.
//!
//! # Result
//!
//! It's possible for a vmod writer to return a bare value, or a `Result<_, E: AsRef<str>>` to
//! potentially abort VCL processing in case the vmod hit an unrecoverable error.
//!
//! If a vmod function returns `Err(msg)`, the boilerplate will log `msg`, mark the current task as
//! failed and will return a default value to the VCL. In turn, the VCL will stop its processing
//! and will create a synthetic error object.

use std::borrow::Cow;
use std::ffi::{c_char, CStr};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::ptr::{null, null_mut};
use std::time::{Duration, SystemTime};

use crate::ffi::{
    http, vtim_dur, vtim_real, VSA_GetPtr, VSA_Port, PF_INET, PF_INET6, VCL_ACL, VCL_BACKEND,
    VCL_BLOB, VCL_BODY, VCL_BOOL, VCL_DURATION, VCL_ENUM, VCL_HEADER, VCL_HTTP, VCL_INT, VCL_IP,
    VCL_PROBE, VCL_REAL, VCL_STEVEDORE, VCL_STRANDS, VCL_STRING, VCL_TIME, VCL_VCL,
};
use crate::vcl::{from_vcl_probe, into_vcl_probe, CowProbe, Probe, VclError, Workspace};

/// Convert a Rust type into a VCL one
///
/// It will use the [`Workspace`] to persist the data during the VCL task if necessary
pub trait IntoVCL<T> {
    fn into_vcl(self, ws: &mut Workspace) -> Result<T, VclError>;
}

macro_rules! default_null_ptr {
    ($ident:ident) => {
        default_null_ptr!($ident, null);
    };
    (mut $ident:ident) => {
        default_null_ptr!($ident, null_mut);
    };
    ($ident:ident, $func:ident) => {
        impl Default for $ident {
            fn default() -> Self {
                $ident($func())
            }
        }
    };
}

macro_rules! into_vcl_using_from {
    ($rust_ty:ty, $vcl_ty:ident) => {
        impl IntoVCL<$vcl_ty> for $rust_ty {
            fn into_vcl(self, _: &mut Workspace) -> Result<$vcl_ty, VclError> {
                Ok(self.into())
            }
        }
    };
}

macro_rules! from_rust_to_vcl {
    ($rust_ty:ty, $vcl_ty:ident) => {
        impl From<$rust_ty> for $vcl_ty {
            fn from(b: $rust_ty) -> Self {
                Self(b.into())
            }
        }
    };
}

macro_rules! from_vcl_to_opt_rust {
    ($vcl_ty:ident, $rust_ty:ty) => {
        impl From<$vcl_ty> for Option<$rust_ty> {
            fn from(b: $vcl_ty) -> Self {
                Some(b.into())
            }
        }
    };
}

// VCL_ACL
default_null_ptr!(VCL_ACL);

// VCL_BACKEND
default_null_ptr!(VCL_BACKEND);

// VCL_BLOB
default_null_ptr!(VCL_BLOB);
impl From<VCL_BLOB> for &[u8] {
    fn from(value: VCL_BLOB) -> Self {
        if value.0.is_null() {
            return &[]
        }

        // after 6.0
        #[cfg(not(varnishsys_6))]
        unsafe {
            let blob = &*value.0;
            if blob.blob.is_null() || blob.len == 0 {
                &[]
            } else {
                    std::slice::from_raw_parts(
                            blob.blob.cast::<u8>(),
                            blob.len,
                    )
            }
        }
        #[cfg(varnishsys_6)]
        unsafe {
            let blob = &*value.0;
            if blob.priv_.is_null() || blob.len <= 0 {
                &[]
            } else {
                    std::slice::from_raw_parts(
                            blob.priv_.cast::<u8>(),
                            blob.len as usize,
                    )
            }
        }
    }
}
from_vcl_to_opt_rust!(VCL_BLOB, &[u8]);

// VCL_BODY
default_null_ptr!(VCL_BODY);

//
// VCL_BOOL
//
into_vcl_using_from!(bool, VCL_BOOL);
from_rust_to_vcl!(bool, VCL_BOOL);
from_vcl_to_opt_rust!(VCL_BOOL, bool);
impl From<VCL_BOOL> for bool {
    fn from(b: VCL_BOOL) -> Self {
        b.0 != 0
    }
}

//
// VCL_DURATION
//
into_vcl_using_from!(Duration, VCL_DURATION);
from_vcl_to_opt_rust!(VCL_DURATION, Duration);
impl From<VCL_DURATION> for Duration {
    fn from(value: VCL_DURATION) -> Self {
        value.0.into()
    }
}
impl From<Duration> for VCL_DURATION {
    fn from(value: Duration) -> Self {
        Self(value.into())
    }
}

//
// vtim_dur -- this is a sub-structure of VCL_DURATION, equal to f64
//
impl From<vtim_dur> for Duration {
    fn from(value: vtim_dur) -> Self {
        Self::from_secs_f64(value.0)
    }
}
impl From<Duration> for vtim_dur {
    fn from(value: Duration) -> Self {
        Self(value.as_secs_f64())
    }
}

// VCL_ENUM
default_null_ptr!(VCL_ENUM);
// VCL_HEADER
default_null_ptr!(VCL_HEADER);
// VCL_HTTP
default_null_ptr!(mut VCL_HTTP);
impl From<*mut http> for VCL_HTTP {
    // This is needed because pre-v7 vrt_ctx used http instead of VCL_HTTP
    fn from(value: *mut http) -> Self {
        Self(value)
    }
}

//
// VCL_INT
//
into_vcl_using_from!(i64, VCL_INT);
from_rust_to_vcl!(i64, VCL_INT);
from_vcl_to_opt_rust!(VCL_INT, i64);
impl From<VCL_INT> for i64 {
    fn from(b: VCL_INT) -> Self {
        b.0
    }
}

//
// VCL_IP
//
default_null_ptr!(VCL_IP);
impl From<VCL_IP> for Option<SocketAddr> {
    fn from(value: VCL_IP) -> Self {
        let value = value.0;
        if value.is_null() {
            return None;
        }
        unsafe {
            let mut ptr = null();
            let fam = VSA_GetPtr(value, &raw mut ptr) as u32;
            let port = VSA_Port(value) as u16;

            match fam {
                PF_INET => {
                    let buf: &[u8; 4] = std::slice::from_raw_parts(ptr.cast::<u8>(), 4)
                        .try_into()
                        .unwrap();
                    Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(*buf)), port))
                }
                PF_INET6 => {
                    let buf: &[u8; 16] = std::slice::from_raw_parts(ptr.cast::<u8>(), 16)
                        .try_into()
                        .unwrap();
                    Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(*buf)), port))
                }
                _ => None,
            }
        }
    }
}

//
// VCL_PROBE
//
default_null_ptr!(VCL_PROBE);
impl IntoVCL<VCL_PROBE> for CowProbe<'_> {
    fn into_vcl(self, ws: &mut Workspace) -> Result<VCL_PROBE, VclError> {
        into_vcl_probe(self, ws)
    }
}
impl IntoVCL<VCL_PROBE> for Probe {
    fn into_vcl(self, ws: &mut Workspace) -> Result<VCL_PROBE, VclError> {
        into_vcl_probe(self, ws)
    }
}
impl From<VCL_PROBE> for Option<CowProbe<'_>> {
    fn from(value: VCL_PROBE) -> Self {
        from_vcl_probe(value)
    }
}
impl From<VCL_PROBE> for Option<Probe> {
    fn from(value: VCL_PROBE) -> Self {
        from_vcl_probe(value)
    }
}

//
// VCL_REAL
//
into_vcl_using_from!(f64, VCL_REAL);
from_rust_to_vcl!(f64, VCL_REAL);
from_vcl_to_opt_rust!(VCL_REAL, f64);
impl From<VCL_REAL> for f64 {
    fn from(b: VCL_REAL) -> Self {
        b.0
    }
}

//
// VCL_STRING
//
default_null_ptr!(VCL_STRING);
impl IntoVCL<VCL_STRING> for &[u8] {
    fn into_vcl(self, ws: &mut Workspace) -> Result<VCL_STRING, VclError> {
        // Try to save some work if the buffer is already in the workspace.
        // We assume that &[u8] has always been readonly, so workspace data is valid.
        // However, we need to make sure that the buffer is null-terminated, either by its own last
        // byte, or by the one after it
        let with_extra_byte = unsafe { std::slice::from_raw_parts(self.as_ptr(), self.len() + 1) };
        if (ws.contains(self) && self.ends_with(b"\0"))
            || (ws.contains(with_extra_byte) && with_extra_byte.last() == Some(&b'\0'))
        {
            Ok(VCL_STRING(self.as_ptr().cast::<c_char>()))
        } else {
            Ok(VCL_STRING(ws.copy_bytes_with_null(self)?.b))
        }
    }
}

impl IntoVCL<VCL_STRING> for &str {
    fn into_vcl(self, ws: &mut Workspace) -> Result<VCL_STRING, VclError> {
        self.as_bytes().into_vcl(ws)
    }
}
impl IntoVCL<VCL_STRING> for &CStr {
    fn into_vcl(self, ws: &mut Workspace) -> Result<VCL_STRING, VclError> {
        ws.copy_cstr(self)
    }
}
impl IntoVCL<VCL_STRING> for &Cow<'_, str> {
    fn into_vcl(self, ws: &mut Workspace) -> Result<VCL_STRING, VclError> {
        self.as_bytes().into_vcl(ws)
    }
}
impl IntoVCL<VCL_STRING> for String {
    fn into_vcl(self, ws: &mut Workspace) -> Result<VCL_STRING, VclError> {
        self.as_str().into_vcl(ws)
    }
}
impl<T: IntoVCL<VCL_STRING> + AsRef<[u8]>> IntoVCL<VCL_STRING> for Option<T> {
    fn into_vcl(self, ws: &mut Workspace) -> Result<VCL_STRING, VclError> {
        match self {
            None => Ok(VCL_STRING(null())),
            Some(t) => t.as_ref().into_vcl(ws),
        }
    }
}
impl From<VCL_STRING> for Option<&CStr> {
    fn from(value: VCL_STRING) -> Self {
        if value.0.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(value.0) })
        }
    }
}
impl From<VCL_STRING> for &CStr {
    fn from(value: VCL_STRING) -> Self {
        // Treat a null pointer as an empty string
        <Option<&CStr>>::from(value).unwrap_or_default()
    }
}
impl TryFrom<VCL_STRING> for Option<&str> {
    type Error = VclError;
    fn try_from(value: VCL_STRING) -> Result<Self, Self::Error> {
        Ok(<Option<&CStr>>::from(value).map(CStr::to_str).transpose()?)
    }
}
impl<'a> TryFrom<VCL_STRING> for &'a str {
    type Error = VclError;
    fn try_from(value: VCL_STRING) -> Result<Self, Self::Error> {
        Ok(<Option<&'a str>>::try_from(value)?.unwrap_or(""))
    }
}

// VCL_STEVEDORE
default_null_ptr!(VCL_STEVEDORE);
// VCL_STRANDS
default_null_ptr!(VCL_STRANDS);

//
// VCL_TIME
//
impl IntoVCL<VCL_TIME> for SystemTime {
    fn into_vcl(self, _: &mut Workspace) -> Result<VCL_TIME, VclError> {
        self.try_into()
    }
}
impl TryFrom<SystemTime> for VCL_TIME {
    type Error = VclError;

    fn try_from(value: SystemTime) -> Result<Self, Self::Error> {
        Ok(VCL_TIME(vtim_real(
            value
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_err(|e| VclError::new(e.to_string()))?
                .as_secs_f64(),
        )))
    }
}

// VCL_VCL
default_null_ptr!(mut VCL_VCL);

#[cfg(not(varnishsys_6))]
mod version_after_v6 {
    use std::ffi::c_void;
    use std::net::SocketAddr;
    use std::num::NonZeroUsize;
    use std::ptr;
    use std::ptr::null;

    use super::IntoVCL;
    use crate::ffi::{
        sa_family_t, vsa_suckaddr_len, VSA_BuildFAP, PF_INET, PF_INET6, VCL_IP, VCL_REGEX, VCL_SUB,
    };
    use crate::vcl::{VclError, Workspace};
    default_null_ptr!(VCL_SUB);

    default_null_ptr!(VCL_REGEX);

    impl IntoVCL<VCL_IP> for SocketAddr {
        fn into_vcl(self, ws: &mut Workspace) -> Result<VCL_IP, VclError> {
            unsafe {
                // We cannot use sizeof::<suckaddr>() because suckaddr is a zero-sized
                // struct from Rust's perspective
                let size = NonZeroUsize::new(vsa_suckaddr_len).unwrap();
                let p = ws.alloc(size);
                if p.is_null() {
                    Err(VclError::WsOutOfMemory(size))?;
                }
                match self {
                    SocketAddr::V4(sa) => {
                        assert!(!VSA_BuildFAP(
                            p,
                            PF_INET as sa_family_t,
                            sa.ip().octets().as_slice().as_ptr().cast::<c_void>(),
                            4,
                            ptr::from_ref::<u16>(&sa.port().to_be()).cast::<c_void>(),
                            2
                        )
                        .is_null());
                    }
                    SocketAddr::V6(sa) => {
                        assert!(!VSA_BuildFAP(
                            p,
                            PF_INET6 as sa_family_t,
                            sa.ip().octets().as_slice().as_ptr().cast::<c_void>(),
                            16,
                            ptr::from_ref::<u16>(&sa.port().to_be()).cast::<c_void>(),
                            2
                        )
                        .is_null());
                    }
                }
                Ok(VCL_IP(p.cast()))
            }
        }
    }
}
