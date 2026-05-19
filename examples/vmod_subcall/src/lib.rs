varnish::run_vtc_tests!("tests/*.vtc");

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static VARS: RefCell<Option<HashMap<String, String>>> = const { RefCell::new(None) };
}

/// Demonstrate calling VCL subroutines from a VMOD, with per-task context storage.
#[varnish::vmod(docs = "README.md")]
mod subcall {
    use std::collections::HashMap;
    use std::ffi::CStr;

    use varnish::ffi::{VRT_call, VCL_SUB};
    use varnish::vcl::{Ctx, subroutine::Subroutine, VclError};

    /// Call a VCL subroutine using the raw [`VCL_SUB`] pointer directly.
    ///
    /// Demonstrates bypassing the `Subroutine` wrapper and calling `VRT_call`
    /// through the C FFI. Prefer using `Ctx::call_sub` in real code, as it handles error checking and is safer to use. 
    pub fn call_unsafe(ctx: &mut Ctx, sub: VCL_SUB) {
        // SAFETY: VCC guarantees sub is a valid non-null pointer when it reaches this call.
        unsafe { VRT_call(ctx.raw, sub) };
    }

    /// Split `array` on whitespace and call `sub` once per word.
    ///
    /// Each word is stored as `var_name` in the per-task variable map for the
    /// duration of that iteration, accessible via `var`.
    ///
    /// Returns an error if `var_name` is already set — this prevents accidental
    /// shadowing when nesting two `call_for_each` calls with the same name.
    pub fn call_for_each(
        ctx: &mut Ctx,
        array: &CStr,
        var_name: &CStr,
        sub: Subroutine,
    ) -> Result<(), VclError> {
        let key = var_name.to_string_lossy().into_owned();

        crate::VARS.with(|v| {
            if v.borrow().as_ref().is_some_and(|m| m.contains_key(&key)) {
                return Err(VclError::new(format!(
                    "variable {key:?} is already set by an enclosing call_for_each"
                )));
            }
            Ok(())
        })?;

        let words: Vec<String> = array
            .to_string_lossy()
            .split_whitespace()
            .map(String::from)
            .collect();
        for word in words {
            crate::VARS.with(|v| {
                v.borrow_mut()
                    .get_or_insert_with(HashMap::new)
                    .insert(key.clone(), word.clone())
            });
            if ctx.call_sub(sub)? {
                break;
            }
        }
        crate::VARS.with(|v| {
            if let Some(m) = v.borrow_mut().as_mut() {
                 m.remove(&key);
            }
        });

        Ok(())
    }

    /// Return the current value of `var_name` as set by an enclosing `call_for_each`.
    ///
    /// Returns an error if `var_name` is not set.
    pub fn var(_ctx: &mut Ctx, var_name: &CStr) -> Result<String, VclError> {
        let name = var_name.to_string_lossy();
        crate::VARS
            .with(|v| {
                v.borrow()
                    .as_ref()
                    .and_then(|m| m.get(name.as_ref()).cloned())
            })
            .ok_or_else(|| VclError::new(format!("variable {name:?} is not set")))
    }
}
