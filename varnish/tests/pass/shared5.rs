use varnish::vmod;

fn main() {}

#[vmod]
mod refcell_task_map {
    use std::cell::RefCell;
    use std::collections::HashMap;

    pub fn with_map(#[shared_per_task] tsk: &RefCell<Option<HashMap<String, String>>>) {
        let mut borrow = tsk.borrow_mut();
        let map = borrow.get_or_insert_with(HashMap::new);
        map.insert("key".to_string(), "val".to_string());
    }
}
