//! Varnish has the ability to modify the body of object leaving its cache using delivery
//! processors, named `VDP` in the C API, and implemented here using the [`DeliveryProcessor`] trait.
//! Processors are linked together and will read, modify and push data down the delivery pipeline.
//!
//! *Note:* The rust wrapper here is pretty thin and the vmod writer will most probably need to have to
//! deal with the raw Varnish internals.

use std::ffi::{c_int, c_void, CStr};
use std::ptr;

use crate::ffi::{vdp_ctx, vfp_ctx, vfp_entry, vrt_ctx, VdpAction, VfpStatus};
use crate::vcl::{Ctx, VclError};
use crate::{ffi, validate_vfp_ctx, validate_vfp_entry};

/// The return type for [`DeliveryProcessor::push`]
#[derive(Debug, Copy, Clone)]
pub enum PushResult {
    /// Indicates a failure, the pipeline will be stopped with an error
    Err,
    /// Nothing special, processing should continue
    Ok,
    /// Stop early, without error
    End,
}

/// The return type for [`FetchProcessor::pull`]
#[derive(Debug, Copy, Clone)]
pub enum PullResult {
    /// Indicates a failure, the pipeline will be stopped with an error
    Err,
    /// Specify how many bytes were written to the buffer, and that the processor is ready for the
    /// next call
    Ok(usize),
    /// The processor is done, and returns how many bytes were treated
    End(usize),
}

/// The return type for [`DeliveryProcessor::new`] and [`FetchProcessor::new`]
#[derive(Debug)]
pub enum InitResult<T> {
    Err(VclError),
    Ok(T),
    Pass,
}

/// Describes a Varnish Delivery Processor (VDP)
pub trait DeliveryProcessor: Sized {
    /// The name of the processor.
    fn name() -> &'static CStr;
    /// Create a new processor, possibly using knowledge from the pipeline, or from the current
    /// request.
    fn new(vrt_ctx: &mut Ctx, vdp_ctx: &mut DeliveryProcCtx) -> InitResult<Self>;
    /// Handle the data buffer from the previous processor. This function generally uses
    /// [`DeliveryProcCtx::push`] to push data to the next processor.
    fn push(&mut self, ctx: &mut DeliveryProcCtx, act: VdpAction, buf: &[u8]) -> PushResult;
}

pub unsafe extern "C" fn gen_vdp_init<T: DeliveryProcessor>(
    vrt_ctx: *const vrt_ctx,
    ctx_raw: *mut vdp_ctx,
    priv_: *mut *mut c_void,
) -> c_int {
    assert_ne!(priv_, ptr::null_mut());
    assert_eq!(*priv_, ptr::null_mut());
    match T::new(
        &mut Ctx::from_ptr(vrt_ctx),
        &mut DeliveryProcCtx::from_ptr(ctx_raw),
    ) {
        InitResult::Ok(proc) => {
            *priv_ = Box::into_raw(Box::new(proc)).cast::<c_void>();
            0
        }
        InitResult::Err(_) => -1, // TODO: log error
        InitResult::Pass => 1,
    }
}

pub unsafe extern "C" fn gen_vdp_fini<T: DeliveryProcessor>(
    _: *mut vdp_ctx,
    priv_: *mut *mut c_void,
) -> c_int {
    if !priv_.is_null() {
        assert_ne!(*priv_, ptr::null_mut());
        drop(Box::from_raw((*priv_).cast::<T>()));
        *priv_ = ptr::null_mut();
    }

    0
}

pub unsafe extern "C" fn gen_vdp_push<T: DeliveryProcessor>(
    ctx_raw: *mut vdp_ctx,
    act: VdpAction,
    priv_: *mut *mut c_void,
    ptr: *const c_void,
    len: isize,
) -> c_int {
    assert_ne!(priv_, ptr::null_mut());
    assert_ne!(*priv_, ptr::null_mut());
    if !matches!(act, VdpAction::Null | VdpAction::Flush | VdpAction::End) {
        return 1; /* TODO: log */
    }

    let buf = if ptr.is_null() {
        &[0; 0]
    } else {
        std::slice::from_raw_parts(ptr.cast::<u8>(), len as usize)
    };

    match (*(*priv_).cast::<T>()).push(&mut DeliveryProcCtx::from_ptr(ctx_raw), act, buf) {
        PushResult::Err => -1, // TODO: log error
        PushResult::Ok => 0,
        PushResult::End => 1,
    }
}

/// Create a `ffi::vdp` that can be fed to `ffi::VRT_AddFilter`
pub fn new_vdp<T: DeliveryProcessor>() -> ffi::vdp {
    ffi::vdp {
        name: T::name().as_ptr(),
        init: Some(gen_vdp_init::<T>),
        bytes: Some(gen_vdp_push::<T>),
        fini: Some(gen_vdp_fini::<T>),
        priv1: ptr::null(),
        #[cfg(varnishsys_80_io_vdp)]
        io_init: None,
        #[cfg(varnishsys_80_io_vdp)]
        io_upgrade: None,
        #[cfg(varnishsys_80_io_vdp)]
        io_lease: None,
        #[cfg(varnishsys_80_io_vdp)]
        io_fini: None,
    }
}

/// A thin wrapper around a `*mut ffi::vdp_ctx`
#[derive(Debug)]
pub struct DeliveryProcCtx<'a> {
    pub raw: &'a mut vdp_ctx,
}

impl DeliveryProcCtx<'_> {
    /// Check the pointer validity and returns the rust equivalent.
    ///
    /// # Safety
    ///
    /// The caller is in charge of making sure the structure doesn't outlive the pointer.
    pub(crate) unsafe fn from_ptr(raw: *mut vdp_ctx) -> Self {
        let raw = raw.as_mut().unwrap();
        assert_eq!(raw.magic, ffi::VDP_CTX_MAGIC);
        Self { raw }
    }

    /// Send buffer down the pipeline
    pub fn push(&mut self, act: VdpAction, buf: &[u8]) -> PushResult {
        match unsafe {
            ffi::VDP_bytes(
                self.raw,
                act,
                buf.as_ptr().cast::<c_void>(),
                buf.len() as isize,
            )
        } {
            r if r < 0 => PushResult::Err,
            0 => PushResult::Ok,
            _ => PushResult::End,
        }
    }
}

/// Describes a Varnish Fetch Processor (VFP)
pub trait FetchProcessor: Sized {
    /// The name of the processor.
    fn name() -> &'static CStr;
    /// Create a new processor, possibly using knowledge from the pipeline
    fn new(vrt_ctx: &mut Ctx, vfp_ctx: &mut FetchProcCtx) -> InitResult<Self>;
    /// Write data into `buf`, generally using `VFP_Suck` to collect data from the previous
    /// processor.
    fn pull(&mut self, ctx: &mut FetchProcCtx, buf: &mut [u8]) -> PullResult;
}

unsafe extern "C" fn wrap_vfp_init<T: FetchProcessor>(
    vrt_ctx: *const vrt_ctx,
    ctxp: *mut vfp_ctx,
    vfep: *mut vfp_entry,
) -> VfpStatus {
    let ctx = validate_vfp_ctx(ctxp);
    let vfe = validate_vfp_entry(vfep);
    match T::new(
        &mut Ctx::from_ptr(vrt_ctx),
        &mut FetchProcCtx::from_ptr(ctx),
    ) {
        InitResult::Ok(proc) => {
            vfe.priv1 = Box::into_raw(Box::new(proc)).cast::<c_void>();
            VfpStatus::Ok
        }
        InitResult::Err(_) => VfpStatus::Error, // TODO: log the error,
        InitResult::Pass => VfpStatus::End,
    }
}

pub unsafe extern "C" fn wrap_vfp_pull<T: FetchProcessor>(
    ctxp: *mut vfp_ctx,
    vfep: *mut vfp_entry,
    ptr: *mut c_void,
    len: *mut isize,
) -> VfpStatus {
    let ctx = validate_vfp_ctx(ctxp);
    let vfe = validate_vfp_entry(vfep);
    let buf = if ptr.is_null() {
        [0; 0].as_mut()
    } else {
        std::slice::from_raw_parts_mut(ptr.cast::<u8>(), *len as usize)
    };
    let obj = vfe.priv1.cast::<T>().as_mut().unwrap();
    match obj.pull(&mut FetchProcCtx::from_ptr(ctx), buf) {
        PullResult::Err => VfpStatus::Error, // TODO: log error
        PullResult::Ok(l) => {
            *len = l as isize;
            VfpStatus::Ok
        }
        PullResult::End(l) => {
            *len = l as isize;
            VfpStatus::End
        }
    }
}

pub unsafe extern "C" fn wrap_vfp_fini<T: FetchProcessor>(
    ctxp: *mut vfp_ctx,
    vfep: *mut vfp_entry,
) {
    validate_vfp_ctx(ctxp);
    let vfe = validate_vfp_entry(vfep);
    if !vfe.priv1.is_null() {
        let p = ptr::replace(&raw mut vfe.priv1, ptr::null_mut());
        drop(Box::from_raw(p.cast::<T>()));
    }
}

/// Create a `ffi::vfp` that can be fed to `ffi::VRT_AddFilter`
pub fn new_vfp<T: FetchProcessor>() -> ffi::vfp {
    ffi::vfp {
        name: T::name().as_ptr(),
        init: Some(wrap_vfp_init::<T>),
        pull: Some(wrap_vfp_pull::<T>),
        fini: Some(wrap_vfp_fini::<T>),
        priv1: ptr::null(),
    }
}

/// A thin wrapper around a `*mut ffi::vfp_ctx`
#[derive(Debug)]
pub struct FetchProcCtx<'a> {
    pub raw: &'a mut vfp_ctx,
}

impl FetchProcCtx<'_> {
    /// Check the pointer validity and returns the rust equivalent.
    ///
    /// # Safety
    ///
    /// The caller is in charge of making sure the structure doesn't outlive the pointer.
    pub(crate) unsafe fn from_ptr(raw: *mut vfp_ctx) -> Self {
        Self {
            raw: validate_vfp_ctx(raw),
        }
    }

    /// Pull data from the pipeline
    pub fn pull(&mut self, buf: &mut [u8]) -> PullResult {
        let mut len = buf.len() as isize;
        let max_len = len;

        match unsafe { ffi::VFP_Suck(self.raw, buf.as_ptr() as *mut c_void, &raw mut len) } {
            VfpStatus::Ok => {
                assert!(len <= max_len);
                assert!(len >= 0);
                PullResult::Ok(len as usize)
            }
            VfpStatus::End => {
                assert!(len <= max_len);
                assert!(len >= 0);
                PullResult::End(len as usize)
            }
            VfpStatus::Error => PullResult::Err,
            VfpStatus::Null => panic!("VFP_Suck() was never supposed to return VFP_NULL!"),
            // In the future, there might be more enum values, so we should ensure it continues
            // to compile, but we do want a warning when developing locally to add the new one.
            #[expect(unreachable_patterns)]
            n => panic!("unknown VfpStatus {n:?}"),
        }
    }
}

#[derive(Debug)]
pub struct FetchFilters<'c, 'f> {
    ctx: &'c vrt_ctx,
    // The pointer to the box content must be stable.
    // Storing values directly in the vector might be moved when the vector grows.
    #[expect(clippy::vec_box)]
    filters: &'f mut Vec<Box<ffi::vfp>>,
}

impl<'c, 'f> FetchFilters<'c, 'f> {
    #[expect(clippy::vec_box)]
    pub(crate) fn new(ctx: &'c vrt_ctx, filters: &'f mut Vec<Box<ffi::vfp>>) -> Self {
        Self { ctx, filters }
    }

    fn find_position<T: FetchProcessor>(&self) -> Option<usize> {
        let name = T::name().as_ptr();
        self.filters.iter().position(|f| f.name == name)
    }

    pub fn register<T: FetchProcessor>(&mut self) -> bool {
        if self.find_position::<T>().is_none() {
            let instance = Box::new(new_vfp::<T>());
            unsafe {
                ffi::VRT_AddFilter(self.ctx, instance.as_ref(), ptr::null());
            }
            self.filters.push(instance);
            true
        } else {
            false
        }
    }

    pub fn unregister<T: FetchProcessor>(&mut self) -> bool {
        if let Some(pos) = self.find_position::<T>() {
            let filter = self.filters.swap_remove(pos);
            unsafe {
                ffi::VRT_RemoveFilter(self.ctx, filter.as_ref(), ptr::null());
            }
            true
        } else {
            false
        }
    }

    pub fn unregister_all(&mut self) {
        for filter in self.filters.drain(..) {
            unsafe { ffi::VRT_RemoveFilter(self.ctx, filter.as_ref(), ptr::null()) }
        }
    }
}

#[derive(Debug)]
pub struct DeliveryFilters<'c, 'f> {
    ctx: &'c vrt_ctx,
    // The pointer to the box content must be stable.
    // Storing values directly in the vector might be moved when the vector grows.
    #[expect(clippy::vec_box)]
    filters: &'f mut Vec<Box<ffi::vdp>>,
}

impl<'c, 'f> DeliveryFilters<'c, 'f> {
    #[expect(clippy::vec_box)]
    pub(crate) fn new(ctx: &'c vrt_ctx, filters: &'f mut Vec<Box<ffi::vdp>>) -> Self {
        Self { ctx, filters }
    }

    fn find_position<T: DeliveryProcessor>(&self) -> Option<usize> {
        let name = T::name().as_ptr();
        self.filters.iter().position(|f| f.name == name)
    }

    pub fn register<T: DeliveryProcessor>(&mut self) -> bool {
        if self.find_position::<T>().is_none() {
            let instance = Box::new(new_vdp::<T>());
            unsafe {
                ffi::VRT_AddFilter(self.ctx, ptr::null(), instance.as_ref());
            }
            self.filters.push(instance);
            true
        } else {
            false
        }
    }

    pub fn unregister<T: DeliveryProcessor>(&mut self) -> bool {
        if let Some(pos) = self.find_position::<T>() {
            let filter = self.filters.swap_remove(pos);
            unsafe {
                ffi::VRT_RemoveFilter(self.ctx, ptr::null(), filter.as_ref());
            }
            true
        } else {
            false
        }
    }

    pub fn unregister_all(&mut self) {
        for filter in self.filters.drain(..) {
            unsafe { ffi::VRT_RemoveFilter(self.ctx, ptr::null(), filter.as_ref()) }
        }
    }
}
