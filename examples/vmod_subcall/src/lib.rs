varnish::run_vtc_tests!("tests/*.vtc");

/// Demonstrate calling VCL subroutines from a VMOD, with per-task context storage.
#[varnish::vmod(docs = "README.md")]
mod subcall {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ffi::CStr;

    use varnish::ffi::{VRT_call, VCL_SUB};
    use varnish::vcl::{subroutine::Subroutine, Ctx, VclError};

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
        #[shared_per_task] vars: &RefCell<Option<HashMap<String, String>>>,
    ) -> Result<(), VclError> {
        let mut res = Ok(true);
        let key = var_name.to_string_lossy().into_owned();

        if vars.borrow().as_ref().is_some_and(|m| m.contains_key(&key)) {
            return Err(VclError::new(format!(
                "variable {key:?} is already set by an enclosing call_for_each"
            )));
        }

        let words: Vec<String> = array
            .to_string_lossy()
            .split_whitespace()
            .map(String::from)
            .collect();
        for word in words {
            // Drop the borrow_mut before calling the sub so nested calls (var, call_for_each)
            // can borrow vars during sub execution.
            vars.borrow_mut()
                .get_or_insert_with(HashMap::new)
                .insert(key.clone(), word);
            res = ctx.call_sub(sub);
            if res.is_err() {
                break;
            }
        }
        if let Some(m) = vars.borrow_mut().as_mut() {
            m.remove(&key);
        }

        res.map(|_| ())
    }

    /// Return the current value of `var_name` as set by an enclosing `call_for_each`.
    ///
    /// Returns an error if `var_name` is not set.
    pub fn var(
        _ctx: &mut Ctx,
        var_name: &CStr,
        #[shared_per_task] vars: &RefCell<Option<HashMap<String, String>>>,
    ) -> Result<String, VclError> {
        let name = var_name.to_string_lossy();
        vars.borrow()
            .as_ref()
            .and_then(|m| m.get(name.as_ref()).cloned())
            .ok_or_else(|| VclError::new(format!("variable {name:?} is not set")))
    }
}
