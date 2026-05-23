#![expect(unused_variables)]

use varnish::vmod;

fn main() {}

pub struct PerTaskCell;

#[vmod]
mod refcell_task {
    use std::cell::RefCell;
    use super::PerTaskCell;
    use varnish::vcl::Ctx;

    pub fn read(_ctx: &Ctx, #[shared_per_task] tsk: &RefCell<Option<PerTaskCell>>) {}

    pub fn write(_ctx: &mut Ctx, #[shared_per_task] tsk: &RefCell<Option<PerTaskCell>>) {}

    pub fn read_with_opt(
        #[shared_per_task] tsk: &RefCell<Option<PerTaskCell>>,
        op: Option<i64>,
    ) {
    }
}
