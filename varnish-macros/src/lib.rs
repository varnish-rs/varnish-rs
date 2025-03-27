// Uncomment the following line to disable warnings for the entire crate, e.g. during debugging.
// #![allow(warnings)]

use errors::Errors;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, ItemMod};
use {proc_macro as pm, proc_macro2 as pm2};

use crate::gen_docs::generate_docs;
use crate::generator::render_model;
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
///   - `pub fn new(...)` is treated as the object constructor.
///   - `#[vcl_name]` attribute on an object constructor's argument will set it to the VCL name.
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
#[proc_macro_derive(VscMetric, attributes(counter, gauge))]
pub fn vsc_metric(input: pm::TokenStream) -> pm::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    if !metrics::has_repr_c(&input) {
        return syn::Error::new(
            name.span(),
            "VSC structs must be marked with #[repr(C)] for correct memory layout",
        )
        .into_compile_error()
        .into();
    }

    let fields = metrics::get_struct_fields(&input.data);
    metrics::validate_fields(fields);

    let metadata_json = metrics::generate_metadata_json(&name.to_string(), fields);

    quote! {
        unsafe impl varnish::VscMetric for #name {
            fn get_metadata() -> &'static str {
                #metadata_json
            }
        }
    }
    .into()
}
