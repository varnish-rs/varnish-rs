#[varnish::vmod]
mod err_fn_args_logic {
    pub fn string(s: String) {}
    pub fn task_arg_non_mut(#[shared_per_task] a: Option<i64>) {}
    pub fn task_refcell_mut(#[shared_per_task] a: &mut RefCell<Option<i64>>) {}
    pub fn vcl_arg_non_ref(#[shared_per_vcl] a: Option<i64>) {}
    pub fn on_non_event(a: Event) {}
    #[event]
    pub fn on_event_arg(a: i64) {}
    #[event]
    pub fn on_event_arg_task(#[shared_per_task] a: Option<Box<i64>>) {}
    #[event]
    pub fn on_event_arg_vcl(#[shared_per_vcl] a: Option<&i64>) {}
    pub fn socket_addr_non_opt(_v: SocketAddr) {}
    #[event]
    pub fn vcl_name(#[vcl_name] a: &str) {}
}

fn main() {}
