use crate::ffi::{VAS_Fail, Vas};
use std::panic;
use std::sync::Once;

static SET_HOOK: Once = Once::new();

pub fn set_vas_hook_once() {
    SET_HOOK.call_once(|| panic::set_hook(Box::new(vas_hook)));
}

fn vas_hook(info: &panic::PanicHookInfo<'_>) {
    let loc = info.location();
    let line = loc.map_or(-1, |l| l.line().try_into().unwrap_or(-1));
    unsafe {
        VAS_Fail(
            c"???".as_ptr(), // XXX: extract fn name as &Cstr?
            c"???".as_ptr(), // XXX: ideally, should be file name
            line,
            c"rust panic".as_ptr(), // XXX: extract message as &Cstr?
            Vas::Wrong,
        )
    }
}
