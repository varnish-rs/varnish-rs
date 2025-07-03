# Examples

This is a small collection of vmods, written using the [varnish crate](https://crates.io/crates/varnish), each focusing on a different aspect of the API.

- [vmod_example](vmod_example): start with this one for a tour of the different files needed
- [vmod_error](vmod_error): various ways to convey an error back to VCL when the vmod fails
- [vmod_object](vmod_object): how to map a vmod object into a rust equivalent
- [vmod_timestamp](vmod_timestamp): use of a `PRIV_TASK`
- [vmod_infiniteloop](vmod_infiniteloop): access regular C structures
- [vmod_be](vmod_be): define your own backend
- [vmod_event](vmod_event): be notified when the vmod is loaded/discarded and store information
- [vmod_vdp](vmod_vdp) and [vmod_vfp](vmod_vfp): inject Fetch/Delivery processor to modify response body content

Note that you can also use [vmod-rs-example](https://github.com/varnish-rs/vmod-rs-example) for a stand-alone, out-of-tree vmod, with packaging framework.
