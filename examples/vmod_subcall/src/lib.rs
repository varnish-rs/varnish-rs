varnish::run_vtc_tests!("tests/*.vtc");

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static VARS: RefCell<Option<HashMap<String, String>>> = const { RefCell::new(None) };
}

/// Per-task sentinel. Varnish drops this at task end, clearing the thread-local map.
pub struct TaskVars;

impl Drop for TaskVars {
    fn drop(&mut self) {
        VARS.with(|v| v.borrow_mut().take());
    }
}

/// Demonstrate calling VCL subroutines from a VMOD, with per-task context storage.
#[varnish::vmod(docs = "README.md")]
mod subcall {
    use std::collections::HashMap;
    use std::ffi::CStr;

    use varnish::ffi::{VRT_call, VCL_SUB};
    use varnish::vcl::{Ctx, Subroutine, VclError};

    use super::TaskVars;

    /// Call a VCL subroutine using the raw [`VCL_SUB`] pointer directly.
    ///
    /// Demonstrates bypassing the [`Subroutine`] wrapper and calling `VRT_call`
    /// through the C FFI. Prefer [`call_for_each`] or [`Ctx::call_sub`] for new code.
    pub fn call_unsafe(ctx: &mut Ctx, sub: VCL_SUB) {
        // SAFETY: VCC guarantees sub is a valid non-null pointer when it reaches this call.
        unsafe { VRT_call(ctx.raw, sub) };
    }

    /// Split `array` on whitespace and call `sub` once per word.
    ///
    /// Each word is stored as `var_name` in the per-task variable map for the
    /// duration of that iteration, accessible via [`var`].
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
            eprintln!("call_for_each before {:p}", ctx.raw);
            if ctx.call_sub(sub) {
                break;
            }
            eprintln!("call_for_each after {:p}", ctx.raw);
        }
        crate::VARS.with(|v| {
            v.borrow_mut().as_mut().map(|m| m.remove(&key));
        });
        eprintln!("call_for_each delete {:p}", ctx.raw);

        Ok(())
    }

    /// Set `var_name` to `value` in the per-task variable map.
    ///
    /// Overwrites any existing value. To read the value back, use [`var`].
    /// The per-task [`TaskVars`] sentinel ensures the map is cleared when the
    /// Varnish task ends.
    pub fn set_var(
        ctx: &mut Ctx,
        #[shared_per_task] state: &mut Option<Box<TaskVars>>,
        var_name: &CStr,
        value: &CStr,
    ) {
        let key = var_name.to_string_lossy().into_owned();
        let val = value.to_string_lossy().into_owned();
        state.get_or_insert_with(|| Box::new(TaskVars));
        crate::VARS.with(|v| {
            v.borrow_mut()
                .get_or_insert_with(HashMap::new)
                .insert(key, val)
        });
        eprintln!("set_var: {:p}", ctx.raw);
    }

    /// Return the current value of `var_name` as set by an enclosing [`call_for_each`].
    ///
    /// Returns an error if `var_name` is not set.
    pub fn var(ctx: &mut Ctx, var_name: &CStr) -> Result<String, VclError> {
        let name = var_name.to_string_lossy();
        eprintln!("var: {:p}", ctx.raw);
        crate::VARS
            .with(|v| {
                v.borrow()
                    .as_ref()
                    .and_then(|m| m.get(name.as_ref()).cloned())
            })
            .ok_or_else(|| VclError::new(format!("variable {name:?} is not set")))
    }
}
