varnish::run_vtc_tests!("tests/*.vtc");

/// Measure time in VCL
#[varnish::vmod(docs = "README.md")]
mod timestamp {
    use std::mem;
    use std::ops::Add;
    use std::time::{Duration, Instant, SystemTime};

    /// Just return the current system time. This will actually return different values if called
    /// multiple times in the same VCL subroutine, contrary to the `now` keyword that always has
    /// the same value for a give subroutine.
    pub fn now() -> SystemTime {
        SystemTime::now()
    }

    /// Add `time` and `duration` and return the resulting time
    pub fn add_duration(time: SystemTime, duration: Duration) -> SystemTime {
        time.add(duration)
    }

    /// Returns the duration since the same function was called for the last time (in the same task).
    /// If it's the first time it's been called, return 0.
    ///
    /// There could be only one type of per-task shared context data type in a Varnish VMOD.
    pub fn timestamp(#[shared_per_task] shared: &mut Option<Box<Instant>>) -> Duration {
        // we will need this either way
        let now = Instant::now();

        match shared {
            None => {
                // This is the first time we're running this function in the task's context
                *shared = Some(Box::new(now));
                Duration::default()
            }
            Some(shared) => {
                // Update box content in-place to the new value, and get the old value
                let old_now = mem::replace(&mut **shared, now);
                // Since Instant implements Copy, we can continue using it and subtract the old value
                now.duration_since(old_now)
            }
        }
    }
}
