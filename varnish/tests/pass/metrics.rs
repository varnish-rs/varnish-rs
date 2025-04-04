use std::sync::atomic::AtomicU64;
use varnish::VscMetric;

fn main() {}

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
}
