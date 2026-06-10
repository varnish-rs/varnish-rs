// Uncomment the following line to disable warnings for the entire crate, e.g. during debugging.
// #![allow(warnings)]

use errors::Errors;
use syn::{parse_macro_input, DeriveInput, ItemMod};
use {proc_macro as pm, proc_macro2 as pm2};

use crate::gen_docs::generate_docs;
use crate::generator::render_model;
use crate::metrics::derive_vsc_metric;
use crate::parser::tokens_to_model;

mod errors;
mod gen_docs;
mod gen_func;
mod gen_objects;
mod generator;
mod metrics;
mod model;
mod names;
mod parser;
mod parser_args;
mod parser_utils;
mod vtc_tests;

pub(crate) type ProcResult<T> = Result<T, Errors>;

/// All tests for the proc-macro crate must be part of the crate itself
/// because the tests must call functions not tagged with the `#[proc_macro_attribute]`,
/// but the current proc-macro limitation does not allow these functions to be exported.
/// The only real shortcoming of this approach is that we must add each test file to `tests/mod.rs`
#[cfg(test)]
mod tests;

/// Handle the `#[vmod]` attribute.  This attribute can only be applied to a module.
/// Inside the module, it handles the following items:
/// - Public functions are exported as VMOD functions.
///   - `#[event]` attribute on a function will export it as an event function.
///   - `#[shared_per_task]` attribute on a function argument will treat it as a `PRIV_TASK` object.
///   - `#[shared_per_vcl]` attribute on a function argument will treat it as a `PRIV_VCL` object.
/// - `impl` blocks' public methods are exported as VMOD object methods. The object itself may reside outside the module.
///   - A public method returning `Self` or `Result<Self, _>` is treated as the object constructor.
///   - `#[vcl_name]` attribute on an object constructor's argument will set it to the VCL name.
/// The `#[vmod]` attribute can only be applied to a module. All `pub` functions in that module
/// are exported as VMOD functions. `impl` blocks export their `pub` methods as VMOD object
/// methods; `pub fn new(...)` is treated as the object constructor.
///
/// # Special function attributes
///
/// ## `#[event]`
///
/// Marks a function as a VCL event handler. Called by Varnish on `Load`, `Warm`, `Cold`, and
/// `Discard` events. The function must accept `(evt: Event, ctx: &mut Ctx)` as its first two
/// arguments (plus any `#[shared_per_vcl]` arguments).
///
/// ## `#[restrict(scope, ...)]`
///
/// Restricts which VCL subroutines may call this function. A violation is a **VCL compile
/// error**, not a runtime panic. Cannot be used on `#[event]` functions.
///
/// Valid individual scopes: `vcl_recv`, `vcl_pass`, `vcl_hash`, `vcl_purge`, `vcl_miss`,
/// `vcl_hit`, `vcl_deliver`, `vcl_synth`, `vcl_pipe`, `vcl_backend_fetch`,
/// `vcl_backend_refresh`, `vcl_backend_response`, `vcl_backend_error`, `vcl_init`, `vcl_fini`
///
/// Valid grouped scopes: `client` (all client-side subs), `backend` (all backend-side subs),
/// `housekeeping` (`vcl_init` + `vcl_fini`)
///
/// ```rust
/// # use varnish::vmod;
/// #[vmod]
/// mod example {
///     /// Only callable from client-side subs (vcl_recv, vcl_pass, vcl_hash, …)
///     #[restrict(client)]
///     pub fn client_only() -> i64 { 1 }
///
///     /// Only callable from vcl_recv and vcl_hash
///     #[restrict(vcl_recv, vcl_hash)]
///     pub fn recv_or_hash() -> i64 { 2 }
///
///     /// Callable from both client and backend contexts
///     #[restrict(client, backend)]
///     pub fn client_or_backend() -> i64 { 3 }
/// }
/// ```
///
/// ## `#[shared_per_task]`
///
/// Applied to a function argument to share state across all calls within a single
/// request/task. The value is created on first use and dropped when the task ends — it does
/// not outlive the request. This maps to `PRIV_TASK` in C VMODs.
///
/// The type must be `&mut Option<Box<T>>` for mutable access, or `Option<&T>` for read-only
/// access. Only one `T` type is allowed per VMOD.
///
/// ```rust
/// # use varnish::vmod;
/// #[vmod]
/// mod example {
///     use std::time::{Duration, Instant};
///
///     /// Returns elapsed time since first call in this request, or 0 on first call.
///     pub fn elapsed(#[shared_per_task] start: &mut Option<Box<Instant>>) -> Duration {
///         let now = Instant::now();
///         match start {
///             None => { *start = Some(Box::new(now)); Duration::ZERO }
///             Some(t) => now.duration_since(**t),
///         }
///     }
/// }
/// ```
///
/// ## `#[shared_per_vcl]`
///
/// Applied to a function argument to share state for the entire lifetime of a VCL load.
/// The value lives from when the VCL is loaded until it is discarded — it survives across
/// many requests. This maps to `PRIV_VCL` in C VMODs.
///
/// The type must be `&mut Option<Box<T>>` for mutable access, or `Option<&T>` for read-only
/// access. Only one `T` type is allowed per VMOD. Mutable access is only available in
/// `#[event]` handlers and object constructors; everywhere else use read-only `Option<&T>`.
///
/// ```rust
/// # use varnish::vmod;
/// # use varnish::vcl::{Ctx, Event};
/// #[vmod]
/// mod example {
///     use varnish::vcl::{Ctx, Event};
///
///     pub struct Config { pub threshold: i64 }
///
///     /// Set up the VCL-lifetime config when VCL loads.
///     #[event]
///     pub fn on_event(
///         evt: Event,
///         _ctx: &mut Ctx,
///         #[shared_per_vcl] config: &mut Option<Box<Config>>,
///     ) {
///         if evt == Event::Load {
///             *config = Some(Box::new(Config { threshold: 100 }));
///         }
///     }
///
///     /// Read the config on every request.
///     pub fn above_threshold(#[shared_per_vcl] config: Option<&Config>, n: i64) -> bool {
///         config.map_or(false, |c| n > c.threshold)
///     }
/// }
/// ```
///
/// # Special constructor attributes
///
/// ## `#[vcl_name]`
///
/// Marks a constructor parameter that will receive the name given to the object in VCL
/// (e.g. `new myobj = myvmod.mytype()` passes `"myobj"`). Useful for logging and
/// identification. Only valid on object constructors (`pub fn new`), at most once per
/// constructor. The type must be `&str` or `&CStr`.
///
/// ```rust
/// # use varnish::vmod;
/// # pub struct MyObj { name: String }
/// #[vmod]
/// mod example {
///     use super::MyObj;
///
///     impl MyObj {
///         pub fn new(#[vcl_name] name: &str) -> Self {
///             MyObj { name: name.to_owned() }
///         }
///     }
/// }
/// ```
///
/// # `docs` attribute
///
/// Use `#[vmod(docs = "README.md")]` to generate a markdown documentation file at build time.
/// The path is relative to the crate root. The file is overwritten on each build.
#[proc_macro_attribute]
pub fn vmod(args: pm::TokenStream, input: pm::TokenStream) -> pm::TokenStream {
    // parse the module code into a data model.
    // Most error checking is done here.
    // Magical attributes like `#[event]` are removed from the user's code.
    // let args = parse_macro_input!(args);
    // let args = parse_macro_input!(args);
    // let input = parse_macro_input!(input);
    let args = pm2::TokenStream::from(args);
    let mut item_mod = parse_macro_input!(input as ItemMod);

    let info = match tokens_to_model(args, &mut item_mod) {
        Ok(v) => v,
        Err(err) => return err.into_compile_error().into(),
    };

    // generate the code for the VMOD.
    // This will output the slightly modified original user code,
    // plus generate the FFI code as a submodule.
    let result = render_model(item_mod, &info);

    // generate documentation file if needed
    generate_docs(&info);

    result.into()
}

/// Handle the `#[derive(VscMetric)]` macro. This can only be applied to a struct.
/// The struct must have only fields of type `AtomicU64`.
/// - `#[counter]` attribute on a field will export it as a counter.
/// - `#[gauge]` attribute on a field will export it as a gauge.
/// - `#[bitmap]` attribute on a field will export it as a bitmap.
#[proc_macro_derive(VscMetric, attributes(counter, gauge, bitmap))]
pub fn vsc_metric(input: pm::TokenStream) -> pm::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    derive_vsc_metric(&input)
        .unwrap_or_else(Errors::into_compile_error)
        .into()
}

/// Generate one `#[test]` function per VTC file matching the glob pattern.
///
/// Globs are resolved at compile time, relative to `CARGO_MANIFEST_DIR`. Each
/// matched file produces a test named `vtc_<sanitized_stem>`. An optional
/// second argument (`true`) enables verbose `varnishtest` output.
///
/// Use it from your VMOD crate's `lib.rs`:
///
/// ```ignore
/// varnish::run_vtc_tests!("tests/*.vtc");
/// ```
#[proc_macro]
pub fn run_vtc_tests(input: pm::TokenStream) -> pm::TokenStream {
    vtc_tests::generate(input.into()).into()
}
