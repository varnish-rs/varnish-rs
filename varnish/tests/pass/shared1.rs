#![expect(unused_variables)]

use varnish::vmod;

fn main() {}

pub struct PerTask;
pub struct PerVcl;

#[vmod]
mod task {
    use super::{PerTask, PerVcl};
    use varnish::vcl::{Ctx, Event};

    #[event]
    pub fn on_event(evt: Event, ctx: &mut Ctx, #[shared_per_vcl] vcl: &mut Option<Box<PerVcl>>) {}

    pub fn per_vcl_val(
        /// This comment is ignored
        #[shared_per_vcl]
        vcl: Option<&PerVcl>,
    ) {
    }

    pub fn per_vcl_opt(#[shared_per_vcl] vcl: Option<&PerVcl>, op: Option<i64>) {}

    impl PerVcl {
        pub fn new(#[shared_per_vcl] vcl: &mut Option<Box<PerVcl>>) -> Self {
            Self
        }

        pub fn both(
            &self,
            /// this comment is ignored too
            #[shared_per_task]
            tsk: &mut Option<Box<PerTask>>,
            #[shared_per_vcl] vcl: Option<&PerVcl>,
        ) {
        }

        pub fn both_pos(
            &self,
            #[shared_per_task] tsk: &mut Option<Box<PerTask>>,
            #[shared_per_vcl] vcl: Option<&PerVcl>,
            val: i64,
        ) {
        }

        pub fn both_opt(
            &self,
            #[shared_per_task] tsk: &mut Option<Box<PerTask>>,
            #[shared_per_vcl] vcl: Option<&PerVcl>,
            opt: Option<i64>,
        ) {
        }
    }

    pub fn per_tsk_val(#[shared_per_task] tsk: &mut Option<Box<PerTask>>) {}

    pub fn per_tsk_opt(#[shared_per_task] tsk: &mut Option<Box<PerTask>>, op: Option<i64>) {}

    pub fn per_tsk_ref(#[shared_per_task] tsk: Option<&PerTask>) {}

    pub fn per_tsk_ref_opt(#[shared_per_task] tsk: Option<&PerTask>, op: Option<i64>) {}
}
