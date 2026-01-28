use std::sync::Mutex;
use std::time::SystemTime;
use varnish::vcl::{BackendRef, Buffer, Ctx, Director, VclDirector, VclError};
use varnish::ffi::VCL_BACKEND;

varnish::run_vtc_tests!("tests/*.vtc");

/// A simple round-robin director that distributes requests across multiple backends
#[allow(non_camel_case_types)]
pub struct rr {
    director: Director<RoundRobinDirector>,
}

/// Round-robin director VMOD
#[varnish::vmod(docs = "README.md")]
mod round_robin {
    use super::*;

    impl rr {
        /// Create a new round-robin director
        pub fn new(
            ctx: &mut Ctx,
            #[vcl_name] name: &str,
        ) -> Result<Self, VclError> {
            let director = Director::new(
                ctx,
                "round-robin",
                name,
                RoundRobinDirector {
                    state: Mutex::new(RoundRobinState {
                        backends: Vec::new(),
                        current: 0,
                    }),
                },
            )?;

            Ok(rr { director })
        }

        /// Add a backend to the director
        pub fn add_backend(&self, backend: Option<BackendRef>) {
            if let Some(backend) = backend {
                self.director.get_inner().state.lock().unwrap().backends.push(backend);
            }
        }

        /// Get the number of backends in the director
        pub fn count(&self) -> i64 {
            self.director.get_inner().state.lock().unwrap().backends.len() as i64
        }

        /// Get the director backend pointer
        pub unsafe fn backend(&self) -> VCL_BACKEND {
            self.director.raw()
        }
    }
}

/// State for the round-robin director
struct RoundRobinState {
    backends: Vec<BackendRef>,
    current: usize,
}

/// Implementation of the round-robin director logic
struct RoundRobinDirector {
    state: Mutex<RoundRobinState>,
}

impl VclDirector for RoundRobinDirector {
    fn resolve(&self, ctx: &mut Ctx, _director: VCL_BACKEND) -> Option<BackendRef> {
        let mut state = self.state.lock().unwrap();
        if state.backends.is_empty() {
            return None;
        }

        // Try to find a healthy backend, starting from current position
        let start_idx = state.current % state.backends.len();
        for i in 0..state.backends.len() {
            let idx = (start_idx + i) % state.backends.len();
            let backend = &state.backends[idx];
            
            if backend.healthy(ctx) {
                // Clone before updating state to avoid borrow checker issues
                let result = backend.clone();
                // Update current position for next call
                state.current = idx + 1;
                return Some(result);
            }
        }
        
        // No healthy backends found
        None
    }

    fn healthy(&self, ctx: &mut Ctx, changed: &mut SystemTime) -> bool {
        *changed = SystemTime::UNIX_EPOCH;
        
        let state = self.state.lock().unwrap();
        // Check if at least one backend is healthy
        for backend in state.backends.iter() {
            if backend.healthy(ctx) {
                return true;
            }
        }
        false
    }

    fn list(&self, ctx: &mut Ctx, vsb: &mut Buffer, _detailed: bool, json: bool) {
        let state = self.state.lock().unwrap();
        
        if json {
            let _ = vsb.write(&"[\n");
            for (i, backend) in state.backends.iter().enumerate() {
                if i > 0 {
                    let _ = vsb.write(&",\n");
                }
                let name = backend.name().to_string_lossy();
                let is_healthy = backend.healthy(ctx);
                let _ = vsb.write(&format!(
                    "    {{ \"backend_name\": \"{}\", \"healthy\": {} }}",
                    name, is_healthy
                ));
            }
            let _ = vsb.write(&"\n]");
        } else {
            let healthy_count = state.backends.iter()
                .filter(|backend| backend.healthy(ctx))
                .count();
            let _ = vsb.write(&format!("{}/{}	", healthy_count, state.backends.len()));
            let _ = vsb.write(&(if healthy_count > 0 { "healthy" } else { "sick" }));
        }
    }

    fn release(&self) {
        // BackendRef instances will be automatically dropped when the Vec is dropped
        // This explicit release ensures all backend references are cleaned up
        self.state.lock().unwrap().backends.clear();
    }
}
