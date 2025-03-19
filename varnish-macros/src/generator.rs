//! The main generator for the varnish vmod.

use std::ffi::CString;
use std::fmt::Write as _;

use proc_macro2::TokenStream;
use quote::{quote, TokenStreamExt};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use syn::{Item, ItemMod, Type};

use crate::gen_func::FuncProcessor;
use crate::gen_objects::ObjProcessor;
use crate::model::{FuncInfo, ParamType, VmodInfo};
use crate::names::{ForceCstr, Names, ToIdent};

pub fn render_model(mut item_mod: ItemMod, info: &VmodInfo) -> TokenStream {
    let output = Generator::render(info);
    item_mod
        .content
        .as_mut()
        .unwrap()
        .1
        .insert(0, Item::Verbatim(output));

    quote! { #item_mod }
}

#[derive(Debug, Default)]
pub struct Generator {
    pub names: Names,
    pub file_id: CString,
    pub functions: Vec<FuncProcessor>,
    pub objects: Vec<ObjProcessor>,
}

/// See also <https://varnish-cache.org/docs/7.6/reference/vmod.html>
impl Generator {
    pub fn render(vmod: &VmodInfo) -> TokenStream {
        let mut obj = Self {
            names: Names::new(&vmod.ident),
            file_id: Self::calc_file_id(vmod).force_cstr(),
            ..Self::default()
        };
        for info in &vmod.funcs {
            obj.functions.push(FuncProcessor::from_info(
                obj.names.to_func(info.func_type, &info.ident),
                info,
                &vmod.shared_types,
            ));
        }
        for info in &vmod.objects {
            obj.objects.push(ObjProcessor::from_info(
                obj.names.to_obj(&info.ident),
                info,
                &vmod.shared_types,
            ));
        }
        obj.render_generated_mod(vmod)
    }

    /// Use the entire data model parsed from sources to generate a hash.
    /// Should be somewhat consistent and unique for each set of functions.
    fn calc_file_id(info: &VmodInfo) -> String {
        Sha256::digest(format!("{info:?}").as_bytes())
            .into_iter()
            .fold(String::new(), |mut output, b| {
                let _ = write!(output, "{b:02x}");
                output
            })
    }

    fn gen_per_vcl_priv_struct(priv_structs: &mut Vec<TokenStream>, vmod: &VmodInfo) {
        if vmod.use_shared_per_vcl() {
            let ty = vmod.shared_types.get_per_vcl_ty();
            Self::gen_priv_struct(priv_structs, "PRIV_VCL_METHODS", ty, true);
        }
    }

    fn gen_priv_struct(
        tokens: &mut Vec<TokenStream>,
        name: &str,
        type_name: &str,
        is_vcl_state: bool,
    ) {
        let name = name.to_ident();
        // The type name is stored as a string, but we already validated we can parse it during the `parse` phase.
        let ty_ident = syn::parse_str::<Type>(type_name).expect("Unable to parse second time");
        let on_fini = if is_vcl_state {
            "on_fini_per_vcl".to_ident()
        } else {
            "on_fini".to_ident()
        };
        // Static methods to clean up the `vmod_priv` object's `T`
        if cfg!(varnishsys_6_priv_free_f) {
            tokens.push(quote! {
                static #name: vmod_priv_free_f = Some(vmod_priv::#on_fini::<#ty_ident>);
            });
        } else {
            let ty_name = type_name.force_cstr();
            tokens.push(quote! {
                static #name: vmod_priv_methods = vmod_priv_methods {
                    magic: VMOD_PRIV_METHODS_MAGIC,
                    type_: #ty_name.as_ptr(),
                    fini: Some(vmod_priv::#on_fini::<#ty_ident>),
                };
            });
        }
    }

    fn iter_all_funcs(&self) -> impl Iterator<Item = &FuncProcessor> {
        self.functions
            .iter()
            .chain(self.objects.iter().flat_map(|o| o.funcs.iter()))
    }

    fn gen_json(&self) -> String {
        let mut header: Vec<Value> = vec!["$VMOD".into(), "1.0".into()];
        if !cfg!(varnishsys_6) {
            header.extend(vec![
                self.names.mod_name().into(),
                self.names.func_struct_name().into(),
                self.file_id.to_str().unwrap().into(),
                // Ohh the irony, this string from VMOD_ABI_Version is the reason
                // why `varnish-sys` must exist. Without it, we could run bindgen
                // from the `varnish` crate directly.  Ohh well.
                //
                // FIXME: figure out a way to assert that the version string used by varnish_macro is the same
                //        as the value accessible by generated code from varnish::ffi::VMOD_ABI_Version.
                //        Currently it seems not possible to do a constant assert at compile time on b-str/c-str equality.
                varnish_sys::ffi::VMOD_ABI_Version.to_str().unwrap().into(),
                "0".into(),
                "0".into(),
            ]);
        }

        let mut json: Vec<Value> = vec![header.into()];

        if !cfg!(varnishsys_6) {
            json.push(json! {[ "$CPROTO", self.generate_proto() ]});
        }

        for func in &self.functions {
            json.push(func.json.clone());
        }

        for obj in &self.objects {
            json.push(obj.json.clone());
        }

        let mut json = serde_json::to_string_pretty(&json! {json}).unwrap();

        if !cfg!(varnishsys_6) {
            // 7.0+ wrap the JSON in a special format
            json = format!("VMOD_JSON_SPEC\u{2}\n{json}\n\u{3}");
        }

        json
    }

    fn generate_proto(&self) -> String {
        let mut cproto = String::new();
        for obj in &self.objects {
            cproto.push_str(&obj.cproto_typedef_decl);
        }
        for func in self.iter_all_funcs() {
            cproto.push_str(&func.cproto_typedef_decl);
        }
        let _ = write!(cproto, "\nstruct {} {{\n", self.names.func_struct_name());
        for func in self.iter_all_funcs() {
            cproto.push_str(&func.cproto_typedef_init);
        }
        let _ = write!(
            cproto,
            "}};\n\nstatic struct {struct_name} {struct_name};",
            struct_name = self.names.func_struct_name()
        );
        cproto
    }

    fn render_generated_mod(&self, vmod: &VmodInfo) -> TokenStream {
        let cproto = self.generate_proto().force_cstr();
        let vmod_name_data = self.names.data_struct_name().to_ident();
        let c_name = self.names.mod_name().force_cstr();
        let file_id = &self.file_id;
        let mut priv_structs = Vec::new();
        if let Some(s) = vmod.shared_types.shared_per_task_ty.as_ref() {
            Self::gen_priv_struct(&mut priv_structs, "PRIV_TASK_METHODS", s, false);
        }
        Self::gen_per_vcl_priv_struct(&mut priv_structs, vmod);

        let functions = self.iter_all_funcs().map(|f| &f.wrapper_function_body);
        let json = &self.gen_json().force_cstr();
        let export_decls: Vec<_> = self.iter_all_funcs().map(|f| &f.export_decl).collect();
        let export_inits: Vec<_> = self.iter_all_funcs().map(|f| &f.export_init).collect();

        // WARNING: This list must match the list in varnish-macros/src/lib.rs
        let mut use_ffi_items = quote![
            VCL_BACKEND,
            VCL_BOOL,
            VCL_DURATION,
            VCL_INT,
            VCL_IP,
            VCL_PROBE,
            VCL_REAL,
            VCL_STRING,
            VCL_VOID,
            VMOD_ABI_Version,
            VclEvent,
            vmod_data,
            vmod_priv,
            vrt_ctx,
        ];
        if cfg!(varnishsys_6_priv_free_f) {
            use_ffi_items.append_all(quote![vmod_priv_free_f]);
        } else {
            use_ffi_items.append_all(quote![VMOD_PRIV_METHODS_MAGIC, vmod_priv_methods]);
        }
        // WARNING: This list must match the list in varnish-macros/src/lib.rs

        let func_name;
        let cproto_ptr;
        let cproto_def;
        if cfg!(varnishsys_6) {
            func_name = quote! {};
            cproto_ptr = quote! { cproto.as_ptr() };
            cproto_def = quote! { const cproto: &CStr = #cproto; };
        } else {
            let c_func_name = self.names.func_struct_name().force_cstr();
            func_name = quote! { func_name: #c_func_name.as_ptr(), };
            cproto_ptr = quote! { null() };
            cproto_def = quote! {};
        }

        let vmod_data_extras = if cfg!(varnishsys_77_vmod_data) {
            quote! {
                vcs: c"".as_ptr(),  // FIXME: value?
                version: c"".as_ptr(),  // FIXME: value?
            }
        } else {
            quote! {}
        };

        quote!(
            #[allow(
                non_snake_case,
                unused_imports,
                unused_qualifications,
                unused_variables,
            )]
            #[allow(
                clippy::needless_question_mark,
                clippy::new_without_default,
            )]
            #[automatically_derived]
            mod varnish_generated {
                use std::ffi::{c_char, c_int, c_uint, c_void, CStr};
                use std::ptr::null;
                use varnish::ffi::{#use_ffi_items};
                use varnish::vcl::{Ctx, IntoVCL, PerVclState, Workspace};
                use super::*;

                #( #priv_structs )*
                #( #functions )*

                #[repr(C)]
                pub struct VmodExports {
                    #(#export_decls,)*
                }

                pub static VMOD_EXPORTS: VmodExports = VmodExports {
                    #(#export_inits,)*
                };

                // This name must be in the format `Vmod_{modulename}_Data`.
                #[allow(non_upper_case_globals)]
                #[no_mangle]
                pub static #vmod_name_data: vmod_data = vmod_data {
                    vrt_major: 0,
                    vrt_minor: 0,
                    file_id: #file_id.as_ptr(),
                    name: #c_name.as_ptr(),
                    #func_name
                    func_len: ::std::mem::size_of::<VmodExports>() as c_int,
                    func: &VMOD_EXPORTS as *const _ as *const c_void,
                    abi: VMOD_ABI_Version.as_ptr(),
                    json: JSON.as_ptr(),
                    proto: #cproto_ptr,
                    #vmod_data_extras
                };

                const JSON: &CStr = #json;
                #cproto_def
            }
        )
    }
}

impl FuncInfo {
    pub fn use_shared_per_vcl(&self) -> bool {
        self.count_args(|v| {
            matches!(
                v.ty,
                ParamType::SharedPerVclMut | ParamType::FetchFilters | ParamType::DeliveryFilters
            )
        }) > 0
    }
}

impl VmodInfo {
    pub fn use_shared_per_vcl(&self) -> bool {
        self.count_funcs(|v| v.use_shared_per_vcl()) > 0
    }
}
