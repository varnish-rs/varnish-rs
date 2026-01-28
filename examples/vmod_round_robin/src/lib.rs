use std::sync::Mutex;
use std::time::SystemTime;
use varnish::vcl::{BackendRef, Buffer, Ctx, Director, ProbeResult, VclDirector, VclError};
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
        pub fn add_backend(&self, backend: Option<BackendRef>) -> Result<(), VclError> {
            let backend = backend.ok_or_else(|| {
                VclError::new("round_robin.add_backend() requires a non-null backend".to_string())
            })?;
            self.director.get_inner().state.lock().unwrap().backends.push(backend);
            Ok(())
        }

        /// Get the number of backends in the director
        pub fn count(&self) -> i64 {
            self.director.get_inner().state.lock().unwrap().backends.len() as i64
        }

        /// Get the director as a backend reference
        pub fn backend(&self) -> BackendRef {
            BackendRef::new(self.director.raw())
                .expect("Director should always have a valid backend pointer")
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
            
            if backend.healthy(ctx).healthy {
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

    fn healthy(&self, ctx: &mut Ctx) -> ProbeResult {
        let state = self.state.lock().unwrap();
        
        let (any_healthy, latest_change) = state.backends.iter()
            .map(|backend| backend.healthy(ctx))
            .fold((false, SystemTime::UNIX_EPOCH), |(any_healthy, latest), probe| {
                (
                    any_healthy || probe.healthy,
                    latest.max(probe.last_changed),
                )
            });
        
        ProbeResult {
            healthy: any_healthy,
            last_changed: latest_change,
        }
    }

    fn list(&self, ctx: &mut Ctx, vsb: &mut Buffer, _detailed: bool, json: bool) {
        let state = self.state.lock().unwrap();
        
        if json {
            // Collect probe results to get health status and last change timestamp
            let probe_results: Vec<_> = state.backends.iter()
                .map(|backend| backend.healthy(ctx))
                .collect();
            
            let healthy_count = probe_results.iter()
                .filter(|probe| probe.healthy)
                .count();
            let total_count = state.backends.len();
            let health_status = if healthy_count > 0 { "healthy" } else { "sick" };
            
            // Find the most recent change across all backends
            let last_change = probe_results.iter()
                .map(|probe| probe.last_changed)
                .max()
                .unwrap_or(SystemTime::UNIX_EPOCH)
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs_f64();
            
            let _ = vsb.write(&"{\n");
            let _ = vsb.write(&"  \"type\": \"roundrobin\",\n");
            let _ = vsb.write(&"  \"admin_health\": \"probe\",\n");
            let _ = vsb.write(&format!(
                "  \"probe_message\": [{}, {}, \"{}\"],\n",
                healthy_count, total_count, health_status
            ));
            let _ = vsb.write(&"  \"backends\": [\n");
            for (i, backend) in state.backends.iter().enumerate() {
                if i > 0 {
                    let _ = vsb.write(&",\n");
                }
                let name = backend.name().to_string_lossy();
                let _ = vsb.write(&format!(
                    "    \"{}\"",
                    name
                ));
            }
            let _ = vsb.write(&"\n        ],\n");
            let _ = vsb.write(&format!("  \"last_change\": {:.3}\n", last_change));
            let _ = vsb.write(&"}");
        } else {
            let healthy_count = state.backends.iter()
                .filter(|backend| backend.healthy(ctx).healthy)
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
