use std::sync::atomic::AtomicU64;

use varnish::{Vsc, VscMetric};

#[derive(VscMetric)]
#[repr(C)] // required for correct memory layout
pub struct VariousStats {
    /// Some arbitrary counter
    #[counter]
    foo: AtomicU64,

    /// Some arbitrary gauge
    #[gauge]
    temperature: AtomicU64,

    /// An arbitrary gauge with a longer description
    ///
    /// A more detailed description than the above oneliner could go here.
    #[gauge(level = "debug", format = "bytes")]
    memory: AtomicU64,

    /// Some arbitrary bitmap
    #[bitmap]
    flags: AtomicU64,
}

#[allow(non_camel_case_types)]
pub struct test {
    stats: Vsc<VariousStats>,
}

#[varnish::vmod(docs = "README.md")]
mod stats {
    use varnish::Vsc;

    use super::{test, VariousStats};

    impl test {
        #[allow(clippy::new_without_default)]
        pub fn new() -> Self {
            let stats = Vsc::<VariousStats>::new("mystats", "default");
            Self { stats }
        }

        pub fn increment_foo(&self) {
            self.stats
                .foo
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        pub fn get_foo(&self) -> i64 {
            self.stats
                .foo
                .load(std::sync::atomic::Ordering::Relaxed)
                .try_into()
                .unwrap()
        }

        pub fn update_temperature(&self, value: i64) {
            self.stats.temperature.store(
                value.try_into().unwrap(),
                std::sync::atomic::Ordering::Relaxed,
            );
        }

        pub fn get_temperature(&self) -> i64 {
            self.stats
                .temperature
                .load(std::sync::atomic::Ordering::Relaxed)
                .try_into()
                .unwrap()
        }

        pub fn get_memory(&self) -> i64 {
            self.stats
                .memory
                .load(std::sync::atomic::Ordering::Relaxed)
                .try_into()
                .unwrap()
        }

        pub fn update_memory(&self, value: i64) {
            self.stats.memory.store(
                value.try_into().unwrap(),
                std::sync::atomic::Ordering::Relaxed,
            );
        }

        pub fn set_flag(&self, bit: i64) {
            let mask = 1u64 << bit;
            self.stats
                .flags
                .fetch_or(mask, std::sync::atomic::Ordering::Relaxed);
        }

        pub fn clear_flag(&self, bit: i64) {
            let mask = !(1u64 << bit);
            self.stats
                .flags
                .fetch_and(mask, std::sync::atomic::Ordering::Relaxed);
        }

        pub fn get_flags(&self) -> i64 {
            self.stats
                .flags
                .load(std::sync::atomic::Ordering::Relaxed)
                .try_into()
                .unwrap()
        }
    }
}

#[cfg(test)]
mod tests {
    // run all VTC tests
    varnish::run_vtc_tests!("tests/*.vtc");
}
