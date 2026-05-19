use varnish::vmod;

fn main() {}

pub struct PerTask<'a> {
    pub data: &'a str,
}

#[vmod]
mod tuple {
    use super::PerTask;

    pub fn ref_to_slice_lifetime<'a>(
        #[shared_per_task] tsk_vals: &mut Option<Box<PerTask<'a>>>,
    ) -> Option<&'a str> {
        tsk_vals.as_ref().as_deref().map(|v| v.data)
    }
}
