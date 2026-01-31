use std::sync::Mutex;
use std::time::SystemTime;
use varnish::vcl::{BackendRef, Buffer, Ctx, Director, ProbeResult, VclDirector, VclError};
use varnish_sys::report_details_json;

varnish::run_vtc_tests!("tests/*.vtc");

/// A simple round-robin director that distributes requests across multiple backends
#[allow(non_camel_case_types)]
pub struct rr {
    director: Director<RoundRobinDirector>,
}

/// Round-robin director VMOD
#[varnish::vmod(docs = "README.md")]
mod round_robin {
    use super::{
        rr, BackendRef, Ctx, Director, Mutex, RoundRobinDirector, RoundRobinState, VclError,
    };

    impl rr {
        /// Create a new round-robin director
        pub fn new(ctx: &mut Ctx, #[vcl_name] name: &str) -> Result<Self, VclError> {
            Ok(rr {
                director: Director::new(
                    ctx,
                    "roundrobin",
                    name,
                    RoundRobinDirector {
                        state: Mutex::new(RoundRobinState {
                            backends: Vec::new(),
                            current: 0,
                        }),
                    },
                )?,
            })
        }

        /// Add a backend to the director
        pub fn add_backend(&self, backend: Option<BackendRef>) -> Result<(), VclError> {
            let backend = backend.ok_or_else(|| {
                VclError::new("round_robin.add_backend() requires a non-null backend".to_string())
            })?;
            self.director.get_inner().state().backends.push(backend);
            Ok(())
        }

        /// Get the number of backends in the director
        pub fn count(&self) -> i64 {
            self.director.get_inner().state().backends.len() as i64
        }

        /// Get the director as a backend reference
        pub fn backend(&self) -> BackendRef {
            BackendRef::new(self.director.vcl_ptr())
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

impl RoundRobinDirector {
    /// Helper function to access the locked state
    fn state(&self) -> std::sync::MutexGuard<'_, RoundRobinState> {
        self.state.lock().unwrap()
    }

    /// Get backend health statistics
    fn health_stats(&self, ctx: &mut Ctx) -> (usize, usize, &'static str) {
        let state = self.state();
        let (healthy_count, _) = state
            .backends
            .iter()
            .map(|backend| backend.probe(ctx))
            .fold((0, SystemTime::UNIX_EPOCH), |(count, latest), probe| {
                (
                    count + usize::from(probe.healthy),
                    latest.max(probe.last_changed),
                )
            });
        let total_count = state.backends.len();
        let health_status = if healthy_count > 0 { "healthy" } else { "sick" };
        (healthy_count, total_count, health_status)
    }
}

impl VclDirector for RoundRobinDirector {
    fn resolve(&self, ctx: &mut Ctx) -> Option<BackendRef> {
        let mut state = self.state();
        if state.backends.is_empty() {
            return None;
        }

        // Try to find a healthy backend, starting from current position
        let start_idx = state.current % state.backends.len();
        for i in 0..state.backends.len() {
            let idx = (start_idx + i) % state.backends.len();
            let backend = &state.backends[idx];

            if backend.probe(ctx).healthy {
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

    fn probe(&self, ctx: &mut Ctx) -> ProbeResult {
        let state = self.state();

        let (any_healthy, latest_change) = state
            .backends
            .iter()
            .map(|backend| backend.probe(ctx))
            .fold(
                (false, SystemTime::UNIX_EPOCH),
                |(any_healthy, latest), probe| {
                    (any_healthy || probe.healthy, latest.max(probe.last_changed))
                },
            );

        ProbeResult {
            healthy: any_healthy,
            last_changed: latest_change,
        }
    }

    fn report(&self, ctx: &mut Ctx, vsb: &mut Buffer) {
        let (healthy_count, total_count, health_status) = self.health_stats(ctx);
        let _ = vsb.write(&format!("{healthy_count}/{total_count}\t"));
        let _ = vsb.write(&(health_status));
    }

    fn report_details(&self, ctx: &mut Ctx, vsb: &mut Buffer) {
        let state = self.state();
        let _ = vsb.write(&format!("{:<30}{}\n", "Backend", "Health"));
        for backend in &state.backends {
            let probe = backend.probe(ctx);
            let name = backend.name().to_str().unwrap();
            let health = if probe.healthy { "healthy" } else { "sick" };
            let _ = vsb.write(&format!("{name:<30}{health}\n"));
        }
    }

    fn report_json(&self, ctx: &mut Ctx, vsb: &mut Buffer) {
        let (healthy_count, total_count, health_status) = self.health_stats(ctx);
        let json_array = serde_json::json!([healthy_count, total_count, health_status]);
        let json_str = serde_json::to_string(&json_array)
            .expect("Failed to serialize JSON array");
        let _ = vsb.write(&json_str);
    }

    fn report_details_json(&self, ctx: &mut Ctx, vsb: &mut Buffer) {
        let state = self.state();
        let backend_map: std::collections::HashMap<&str, serde_json::Value> = state
            .backends
            .iter()
            .map(|backend| {
                let probe = backend.probe(ctx);
                let name = backend.name().to_str().unwrap();
                let health_info = serde_json::json!({
                    "healthy": probe.healthy
                });
                (name, health_info)
            })
            .collect();

        report_details_json!(vsb, serde_json::json!({
            "backends": backend_map
        }));
    }

    fn release(&self) {
        // BackendRef instances will be automatically dropped when the Vec is dropped
        // This explicit release ensures all backend references are cleaned up
        self.state().backends.clear();
    }
}
