use std::ffi::CStr;
use std::ptr::null;
use std::time::SystemTime;

use crate::ffi;
use crate::ffi::{VCL_BACKEND, VCL_TIME};

/// Result from a probe health check
///
/// Contains both the health status and when it last changed.
#[derive(Debug, Clone, Copy)]
pub struct ProbeResult {
    pub healthy: bool,
    pub last_changed: SystemTime,
}

#[derive(Debug)]
pub struct BackendRef {
    refcounted: bool,
    bep: VCL_BACKEND,
}

impl BackendRef {
    pub unsafe fn new(bep: VCL_BACKEND) -> Option<Self> {
        if bep.0.is_null() {
            return None;
        }
        unsafe {
            let dir = bep.0.as_ref()?;
            assert_eq!(dir.magic, ffi::DIRECTOR_MAGIC);
            let vdir = dir.vdir.as_mut().expect("vdir can't be null");
            assert_eq!(vdir.magic, ffi::VCLDIR_MAGIC);
            if vdir.flags & ffi::VDIR_FLG_NOREFCNT == 0 {
                ffi::Lck__Lock(
                    &raw mut vdir.dlck,
                    c"BackendRef::new".as_ptr(),
                    line!() as i32,
                );
                assert!(vdir.refcnt > 0);
                vdir.refcnt += 1;
                ffi::Lck__Unlock(
                    &raw mut vdir.dlck,
                    c"BackendRef::new".as_ptr(),
                    line!() as i32,
                );
            }
        }
        Some(BackendRef {
            bep,
            refcounted: true,
        })
    }

    // this doesn't grab a reference and is only used to
    // create the backend_ref fields of Backend, NativeBackend and Director
    pub(super) unsafe fn new_withouth_refcount(bep: VCL_BACKEND) -> Option<Self> {
        if bep.0.is_null() {
            return None;
        }
        Some(BackendRef {
            bep,
            refcounted: false,
        })
    }

    pub fn probe(&self, ctx: &crate::vcl::Ctx) -> ProbeResult {
        let mut changed = VCL_TIME::default();
        let healthy = unsafe { ffi::VRT_Healthy(ctx.raw, self.bep, &raw mut changed).into() };
        let last_changed = changed.try_into().unwrap_or(SystemTime::UNIX_EPOCH);
        ProbeResult {
            healthy,
            last_changed,
        }
    }

    pub fn name(&self) -> &CStr {
        assert!(!self.bep.0.is_null());
        unsafe {
            let dir = *self.bep.0;
            assert_eq!(dir.magic, ffi::DIRECTOR_MAGIC);
            CStr::from_ptr(dir.vcl_name)
        }
    }

    pub fn vcl_ptr(&self) -> VCL_BACKEND {
        // need to clone, we'll clear bep on drop
        self.bep
    }
}

impl Clone for BackendRef {
    fn clone(&self) -> BackendRef {
        // self.vcl_ptr() will never be null
        unsafe { BackendRef::new(self.vcl_ptr()).unwrap() }
    }
}

impl Drop for BackendRef {
    fn drop(&mut self) {
        assert!(!self.bep.0.is_null());
        if self.refcounted {
            unsafe {
                ffi::VRT_Assign_Backend(&raw mut self.bep, VCL_BACKEND(null()));
            }
        }
    }
}
