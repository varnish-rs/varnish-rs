use crate::ffi::director;
use crate::vcl::{VclBackend, VclResponse};

/// Return the private pointer as a reference to the [`VclBackend`] object
/// FIXME: should it return a `&mut` instead?
pub fn get_backend<S: VclBackend<T>, T: VclResponse>(v: &director) -> &S {
    unsafe { v.priv_.cast::<S>().as_ref().unwrap() }
}
