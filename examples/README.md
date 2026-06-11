# Examples

This is a small collection of vmods, written using the [varnish crate](https://crates.io/crates/varnish), each focusing on a different aspect of the API.

- [vmod_example](vmod_example): start with this one for a tour of the different files needed
- [vmod_error](vmod_error): various ways to convey an error back to VCL when the vmod fails
- [vmod_object](vmod_object): how to map a vmod object into a rust equivalent
- [vmod_timestamp](vmod_timestamp): use of a `PRIV_TASK`
- [vmod_infiniteloop](vmod_infiniteloop): access regular C structures
- [vmod_native_backend](vmod_native_backend): define your own backend
- [vmod_simple_backend](vmod_simple_backend): a backend that sends a fixed response string
- [vmod_round_robin](vmod_round_robin): a director that picks backends in round-robin order
- [vmod_event](vmod_event): be notified when the vmod is loaded/discarded and store information
- [vmod_subcall](vmod_subcall): call VCL subroutines from a vmod, with per-task context storage
- [vmod_restricted_callsites](vmod_restricted_callsites): limit which VCL subroutines can call a function with `#[restrict(...)]`
- [vmod_counters](vmod_counters): expose custom Varnish statistics counters
- [vmod_blobutils](vmod_blobutils): work with `VCL_BLOB` arguments
- [vmod_vdp](vmod_vdp) and [vmod_vfp](vmod_vfp): inject Fetch/Delivery processor to modify response body content
- [stat_tree](stat_tree): display Varnish statistics as a hierarchical tree (standalone binary, not a vmod)

Note that you can also use [vmod-rs-example](https://github.com/varnish-rs/vmod-rs-example) for a stand-alone, out-of-tree vmod starting point.
