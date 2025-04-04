//! Generates functions, methods, and events code for the Varnish VMOD.

use std::fmt::Write as _;

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use serde_json::{json, Value};
use syn::Type;

use crate::model::FuncType::{Constructor, Destructor, Event, Function, Method};
use crate::model::{FuncInfo, OutputTy, ParamKind, ParamTy, ParamType, ParamTypeInfo, SharedTypes};
use crate::names::{Names, ToIdent};

#[derive(Debug, Default)]
pub struct FuncProcessor {
    names: Names,

    /// For fn with optional args, the name of the struct that holds all arguments, i.e. `arg_simple_void_to_void`
    opt_args_ty_name: String,

    /// Rust wrapper steps before calling user fn, e.g. create temp vars from C values. `[ {let ctx = &mut ctx; }, {let __var1 = args.foo;} ]`
    func_pre_call: Vec<TokenStream>,
    /// Arguments as passed to the user function from the Rust wrapper: `[ {&ctx}, {__var1;} ]`
    func_call_vars: Vec<TokenStream>,
    /// Rust wrapper steps to be executed after the user function call even if it fails, e.g. releasing ownership of shared objects.
    func_always_after_call: Vec<TokenStream>,
    /// Rust wrapper requires `__ctx`
    func_needs_ctx: bool,

    /// C function list of arguments for funcs with no optional args, e.g. `["VCL_INT", "VCL_STRING"]`
    cproto_wrapper_args: Vec<&'static str>,
    /// For optional arguments, params to go into the C header, i.e. `[ {c_char valid_arg0}, {VCL_INT arg1} ]`
    cproto_opt_arg_decl: Vec<String>,
    /// For optional arguments, params to go into `args` struct, i.e. `[ {valid_arg0: c_char}, {arg1: VCL_INT} ]`
    opt_args_arg_decl: Vec<TokenStream>,
    /// Args to the export "C" function. Similar to `opt_args_arg_decl`, i.e. `[ {ctx: *mut vrt_ctx}, {arg0: VCL_INT} ]`
    wrap_fn_arg_decl: Vec<TokenStream>,
    /// Corresponding strings to be added to C declaration, matching `wrap_fn_arg_decl`
    cproto_fn_arg_decl: Vec<String>,

    /// List of arguments as published in the JSON, with up to five values each e.g. `[[INT, ...], [STRING]]`
    /// Order:  `[VCC_type, arg_name, default_value, spec(?), is_optional]`
    /// Any arg after the first one is optional - `NULL`, and all trailing `NULLs` should be removed.
    args_json: Vec<Value>,

    /// `-> c_output_type` or empty if the export "C" function returns nothing
    wrap_fn_output: TokenStream,
    /// VCL types as used in the .c and .h files, e.g. `VCL_INT`, `VCL_STRING`, `VCL_VOID`, ...
    output_hdr: String,
    /// VCC types as used in the .vcc file, e.g. `INT`, `STRING`, `VOID`, ...
    output_vcc: String,

    /// `typedef VCL_VOID td_simple_void_to_void(VRT_CTX, VCL_STRING, ...);` C code
    pub cproto_typedef_decl: String,
    /// `td_simple_void_to_void *f_void_to_void;` - part of the `struct Vmod_something { ... }` C code
    pub cproto_typedef_init: String,

    /// `rust_fn_name: Option< unsafe extern "C" fn name_c_fn(c_args) -> c_output_type >`
    pub export_decl: TokenStream,
    /// `rust_fn_name: Some(name_c_fn)`
    pub export_init: TokenStream,
    /// Full body of the export "C" function
    pub wrapper_function_body: TokenStream,
    /// JSON blob for the function
    pub json: Value,
}

impl FuncProcessor {
    pub fn from_info(names: Names, info: &FuncInfo, shared_types: &SharedTypes) -> Self {
        let mut obj = Self {
            opt_args_ty_name: if info.has_optional_args {
                names.arg_struct_name()
            } else {
                String::new()
            },
            names,
            ..Default::default()
        };
        obj.init(info, shared_types);
        obj
    }

    fn init(&mut self, info: &FuncInfo, shared_types: &SharedTypes) {
        if matches!(info.func_type, Destructor) {
            self.func_pre_call
                .push(quote! { drop(Box::from_raw(*__objp)); *__objp = ::std::ptr::null_mut(); });
        } else {
            self.wrap_fn_arg_decl.push(quote! { __ctx: *mut vrt_ctx });
            self.cproto_fn_arg_decl.push("VRT_CTX".to_string());
        }
        if matches!(info.func_type, Constructor | Destructor) {
            let obj_name = self.names.obj_name().to_ident();
            self.wrap_fn_arg_decl
                .push(quote! { __objp: *mut *mut #obj_name });
            self.cproto_fn_arg_decl
                .push(format!("{} **", self.names.struct_obj_name()));
        }
        if matches!(info.func_type, Constructor) {
            self.wrap_fn_arg_decl
                .push(quote! { __vcl_name: *const c_char });
            self.cproto_fn_arg_decl.push("const char *".to_string());
        }
        if matches!(info.func_type, Method) {
            let obj_name = self.names.obj_access();
            self.wrap_fn_arg_decl
                .push(quote! { __obj: *const #obj_name });
            self.cproto_fn_arg_decl
                .push(format!("{} *", self.names.struct_obj_name()));
        }
        if matches!(info.func_type, Event) {
            self.wrap_fn_arg_decl.push(quote! { __vp: *mut vmod_priv });
            self.wrap_fn_arg_decl.push(quote! { __ev: VclEvent });
        }
        if info.has_optional_args {
            self.func_pre_call
                .push(quote! { let __args = __args.as_ref().unwrap(); });
            let ty = self.opt_args_ty_name.to_ident();
            self.wrap_fn_arg_decl.push(quote! { __args: *const #ty });
        }
        if info.use_shared_per_vcl() {
            let arg_name = "__vp".to_ident();
            let arg_value = Self::get_arg_value(info, &arg_name);
            let shared_ty = shared_types.get_per_vcl_ty();
            let shared_ty = syn::parse_str::<Type>(shared_ty).expect("Unable to parse second time");
            self.add_wrapper_arg(info, quote! { #arg_name: *mut vmod_priv });
            self.func_pre_call.push(
                quote! { let mut __obj_per_vcl = (* #arg_value).take_per_vcl::<#shared_ty>(); },
            );
            let meth = if cfg!(varnishsys_6_priv_free_f) {
                quote!(PRIV_VCL_METHODS)
            } else {
                quote!(&PRIV_VCL_METHODS)
            };
            self.func_always_after_call.push(quote! {
                // Release ownership back to Varnish
                (* #arg_value).put(__obj_per_vcl, #meth);
            });
            let json = Self::arg_to_json("__vp".to_string(), false, "PRIV_VCL", Value::Null);
            self.args_json.push(json);
            self.add_cproto_arg(info, "struct vmod_priv *", "__vp");
        }

        for arg in &info.args {
            self.do_fn_param(info, arg);
        }
        self.do_fn_return(info);

        let wrapper_fn_name = self.names.wrapper_fn_name().to_ident();
        let signature = self.get_wrapper_fn_sig(false);

        self.export_decl = quote! { #wrapper_fn_name: Option< #signature > };
        self.export_init = quote! { #wrapper_fn_name: Some(#wrapper_fn_name) };
        self.wrapper_function_body = self.gen_callback_fn(info);
        (self.cproto_typedef_init, self.cproto_typedef_decl) = self.gen_cproto(info);
        self.json = self.json_func(info);
    }

    /// per-function part of $CPROTO - returns typedef init (part of common struct) and declaration code
    fn gen_cproto(&self, info: &FuncInfo) -> (String, String) {
        let (td_name, decl) = match &info.func_type {
            Function | Constructor | Destructor | Method => {
                let mut decl = "\n".to_string();
                if info.has_optional_args {
                    // This corresponds to the Rust declaration created in `gen_callback_fn`
                    // struct arg_vmod_example_captain_obvious {
                    //    char    valid_n;
                    //    VCL_INT n;
                    // };
                    // typedef VCL_STRING td_vmod_example_captain_obvious(VRT_CTX, struct arg_vmod_example_captain_obvious*);
                    let _ = writeln!(decl, "struct {} {{", self.opt_args_ty_name);
                    for arg in &self.cproto_opt_arg_decl {
                        let _ = writeln!(decl, "  {arg};");
                    }
                    let _ = writeln!(decl, "}};\n");
                }

                let _ = write!(
                    decl,
                    "typedef {} {}(",
                    self.output_hdr,
                    self.names.typedef_name()
                );

                for (idx, arg) in self.cproto_fn_arg_decl.iter().enumerate() {
                    if idx != 0 {
                        decl.push(',');
                    }
                    decl.push_str("\n    ");
                    decl.push_str(arg);
                }

                if info.has_optional_args {
                    let _ = write!(decl, ",\n    struct {} *", self.opt_args_ty_name);
                } else {
                    for arg in &self.cproto_wrapper_args {
                        let _ = write!(decl, ",\n    {arg}");
                    }
                }
                let _ = writeln!(decl, "\n);");

                (self.names.typedef_name(), decl)
            }
            Event => ("vmod_event_f".to_string(), String::new()),
        };

        (format!("  {td_name} *{};\n", self.names.f_fn_name()), decl)
    }

    #[expect(clippy::too_many_lines)]
    fn do_fn_param(&mut self, func_info: &FuncInfo, arg_info: &ParamTypeInfo) {
        let arg_name_ident = arg_info.ident.to_ident();
        let arg_value = Self::get_arg_value(func_info, &arg_name_ident);

        match &arg_info.ty {
            ParamType::Context { is_mut } => {
                self.func_needs_ctx = true;
                self.func_call_vars.push(if *is_mut {
                    quote! { &mut __ctx }
                } else {
                    quote! { &__ctx }
                });
            }
            ParamType::Workspace { is_mut } => {
                self.func_needs_ctx = true;
                self.func_call_vars.push(if *is_mut {
                    quote! { &mut __ctx.ws }
                } else {
                    quote! { &__ctx.ws }
                });
            }
            ParamType::SelfType => {
                self.func_pre_call
                    .push(quote! { let __obj = __obj.as_ref().unwrap(); });
            }
            ParamType::Event => {
                self.func_call_vars.push(quote! { __ev });
                let json = Self::arg_to_json(arg_info.ident.clone(), false, "EVENT", Value::Null);
                self.args_json.push(json);
            }
            ParamType::VclName(pi) => {
                let arg_value = quote! { VCL_STRING(__vcl_name) };
                let input_expr = if pi.ty_info.use_try_from() {
                    quote! { #arg_value.try_into()? }
                } else {
                    quote! { #arg_value.into() }
                };
                self.func_call_vars.push(quote! { #input_expr });
            }
            ParamType::SharedPerTask => {
                self.add_wrapper_arg(func_info, quote! { #arg_name_ident: *mut vmod_priv });
                let temp_var = format_ident!("__obj_per_task");
                self.func_pre_call
                    .push(quote! { let mut #temp_var = (* #arg_value).take(); });
                self.func_call_vars.push(quote! { &mut #temp_var });
                let meth = if cfg!(varnishsys_6_priv_free_f) {
                    quote!(PRIV_TASK_METHODS)
                } else {
                    quote!(&PRIV_TASK_METHODS)
                };
                self.func_always_after_call.push(quote! {
                    // Release ownership back to Varnish
                    if let Some(obj) = #temp_var {
                        (* #arg_value).put(obj, #meth);
                    }
                });

                let json =
                    Self::arg_to_json(arg_info.ident.clone(), false, "PRIV_TASK", Value::Null);
                self.args_json.push(json);
                self.add_cproto_arg(func_info, "struct vmod_priv *", &arg_info.ident);
            }
            ParamType::SharedPerVclRef => {
                self.add_wrapper_arg(func_info, quote! { #arg_name_ident: *const vmod_priv });
                // defensive programming: *vmod_priv should never be NULL,
                // but might as well just treat it as None rather than crashing - its readonly anyway
                self.func_call_vars.push(quote! {
                    #arg_value
                        .as_ref()
                        .and_then::<&PerVclState<_>, _>(|v| v.get_ref())
                        .and_then(|v| v.get_user_data())
                });
                let json =
                    Self::arg_to_json(arg_info.ident.clone(), false, "PRIV_VCL", Value::Null);
                self.args_json.push(json);
                self.add_cproto_arg(func_info, "struct vmod_priv *", &arg_info.ident);
            }
            ParamType::SharedPerVclMut => {
                self.func_call_vars
                    .push(quote! { &mut __obj_per_vcl.user_data });
            }
            ParamType::DeliveryFilters => {
                self.func_needs_ctx = true;
                self.func_call_vars.push(
                    quote! { &mut __ctx.raw.delivery_filters(&mut __obj_per_vcl.delivery_filters) },
                );
            }
            ParamType::FetchFilters => {
                self.func_needs_ctx = true;
                self.func_call_vars.push(
                    quote! { &mut __ctx.raw.fetch_filters(&mut __obj_per_vcl.fetch_filters) },
                );
            }
            ParamType::Value(pi) => {
                // Convert all other C arg types into a Rust arg, and pass it to the user's function
                let mut input_expr = if pi.ty_info.use_try_from() {
                    quote! { #arg_value.try_into()? }
                } else {
                    quote! { #arg_value.into() }
                };
                if matches!(pi.kind, ParamKind::Optional) {
                    let arg_valid = format_ident!("valid_{}", arg_info.ident);
                    let is_arg_valid = quote! { __args.#arg_valid != 0 };
                    input_expr = quote! { if #is_arg_valid { #input_expr } else { None } };
                    self.add_wrapper_arg(func_info, quote! { #arg_valid: c_char });
                    self.cproto_opt_arg_decl.push(format!("char {arg_valid}"));
                }

                let c_type = pi.ty_info.to_c_type().to_ident();
                self.add_wrapper_arg(func_info, quote! { #arg_name_ident: #c_type });
                self.func_call_vars.push(quote! { #input_expr });

                let json = Self::arg_to_json(
                    arg_info.ident.clone(),
                    matches!(pi.kind, ParamKind::Optional),
                    pi.ty_info.to_vcc_type(),
                    pi.default.clone(),
                );
                self.args_json.push(json);
                self.add_cproto_arg(func_info, pi.ty_info.to_c_type(), &arg_info.ident);
            }
        }
    }

    /// Access to the input value, either from the args struct or directly
    fn get_arg_value(func_info: &FuncInfo, arg_name_ident: &Ident) -> TokenStream {
        if func_info.has_optional_args {
            quote! { __args.#arg_name_ident }
        } else {
            quote! { #arg_name_ident }
        }
    }

    fn add_wrapper_arg(&mut self, func_info: &FuncInfo, code: TokenStream) {
        // Events do not modify their signature
        if matches!(func_info.func_type, Function | Constructor | Method) {
            if func_info.has_optional_args {
                self.opt_args_arg_decl.push(code);
            } else {
                self.wrap_fn_arg_decl.push(code);
            }
        }
    }

    fn add_cproto_arg(&mut self, func_info: &FuncInfo, ctype: &'static str, arg_name: &str) {
        if func_info.has_optional_args {
            self.cproto_opt_arg_decl.push(format!("{ctype} {arg_name}"));
        } else {
            self.cproto_wrapper_args.push(ctype);
        }
    }

    fn arg_to_json(
        arg_name: String,
        is_optional_arg: bool,
        vcc_type: &str,
        mut default: Value,
    ) -> Value {
        // JSON data for each argument:
        //   [VCC_type, arg_name, default_value, spec(?), is_optional]
        if !default.is_null() {
            // The default value must be a string containing C code (?)
            // This ensures the string is properly escaped and surrounded by quotes
            default = default.to_string().into();
        }
        let mut json_arg: Vec<Value> = vec![
            vcc_type.into(),
            arg_name.into(),
            default,
            Value::Null, // spec param is not used at this point
        ];

        if is_optional_arg {
            json_arg.push(true.into());
        } else {
            // trim all NULLs from the end of json_arg list
            while let Some(Value::Null) = json_arg.last() {
                json_arg.pop();
            }
        }

        json_arg.into()
    }

    fn do_fn_return(&mut self, info: &FuncInfo) {
        let ty = if matches!(info.func_type, Event) {
            // Rust event functions do not return value, but their C wrappers must return an int
            &OutputTy::ParamType(ParamTy::I64)
        } else {
            &info.output_ty
        };
        self.output_hdr = ty.to_c_type();
        self.wrap_fn_output = if self.output_hdr == "VCL_VOID" {
            quote! {}
        } else {
            let ident = self.output_hdr.to_ident();
            quote! { -> #ident }
        };
        self.output_vcc = ty.to_vcc_type();
    }

    fn json_func(&self, info: &FuncInfo) -> Value {
        let callback_fn = format!(
            "{}.{}",
            self.names.func_struct_name(),
            self.names.f_fn_name()
        );
        let args_struct_cproto = if info.has_optional_args {
            format!("struct {}", self.opt_args_ty_name)
        } else {
            String::new()
        };
        let mut decl: Vec<Value> = vec![
            vec![self.output_vcc.clone()].into(),
            callback_fn.clone().into(),
            args_struct_cproto.into(),
        ];
        decl.extend(self.args_json.iter().cloned());

        match info.func_type {
            Function | Method => {
                json! { [ info.func_type.to_vcc_type(), self.names.fn_name().to_string(), decl ] }
            }
            Constructor | Destructor => {
                json! { [ info.func_type.to_vcc_type(), decl ] }
            }
            Event => {
                json! { [ info.func_type.to_vcc_type(), callback_fn ] }
            }
        }
    }

    /// Generate an extern "C" wrapper function that calls user's Rust function
    fn gen_callback_fn(&self, info: &FuncInfo) -> TokenStream {
        let opt_param_struct = self.gen_opt_param_struct(info);
        let signature = self.get_wrapper_fn_sig(true);
        let func_pre_call = &self.func_pre_call;
        let func_always_after_call = &self.func_always_after_call;
        let mut needs_ctx = self.func_needs_ctx;

        let is_void = self.output_hdr == "VCL_VOID";
        let mut func_steps = Vec::new();

        let mut result_stmt = if matches!(info.func_type, Destructor) {
            quote! {}
        } else {
            let user_fn_name = self.names.fn_callable_name(info.func_type);
            let var_args = &self.func_call_vars;

            let mut func_call = quote! { #user_fn_name(#(#var_args),*) };
            if info.out_result {
                func_call.extend(quote! { ? });
            }

            if matches!(info.func_type, Event) {
                // Ignore the result of the event function, override it with 0
                func_steps.push(quote! { #func_call; });
                func_call = quote! { VCL_INT(0) }
            } else if !is_void && !matches!(info.output_ty, OutputTy::VclType(_)) {
                needs_ctx = true;
                func_call = quote! { #func_call.into_vcl(&mut __ctx.ws)? };
            }

            if matches!(info.func_type, Constructor) {
                func_steps.push(quote! {
                    let __result = Box::new( #func_call );
                    *__objp = Box::into_raw(__result);
                });
                func_call = quote! {};
            }

            func_call
        };

        let result = if self.func_may_fail(info) {
            let error_value = if self.output_hdr == "VCL_VOID" {
                quote! {}
            } else if matches!(info.func_type, Event) {
                // Events require special handling - convert errors into 1, otherwise 0
                quote! { VCL_INT(1) }
            } else {
                quote! { Default::default() }
            };

            if result_stmt.is_empty() {
                result_stmt = quote! { () };
            }
            let lambda = quote! {
                let mut __call_user_func = || -> Result<_, ::varnish::vcl::VclError> {
                    #(#func_steps)*
                    Ok( #result_stmt )
                }
            };
            let res = if func_always_after_call.is_empty() {
                quote! { #lambda; __call_user_func() }
            } else {
                quote! {
                    #lambda;
                    let __result = __call_user_func();
                    #(#func_always_after_call)*
                    __result
                }
            };
            needs_ctx = true;
            quote! {
                #res.unwrap_or_else(|err| {
                    __ctx.fail(err);
                    #error_value
                })
            }
        } else if func_always_after_call.is_empty() {
            quote! {
                #(#func_steps)*
                #result_stmt
            }
        } else if result_stmt.is_empty() {
            quote! {
                #(#func_steps)*
                #(#func_always_after_call)*
            }
        } else {
            quote! {
                #(#func_steps)*
                let __result = #result_stmt;
                #(#func_always_after_call)*
                __result
            }
        };
        let create_ctx = if needs_ctx {
            quote! { let mut __ctx = Ctx::from_ptr(__ctx); }
        } else {
            quote! {}
        };

        quote! {
            #opt_param_struct
            #signature {
                #create_ctx
                #(#func_pre_call)*
                #result
            }
        }
    }

    /// Will be true if the wrapper uses `try_from`, or the user function returns a `Result<T, E>`, or the output may fail conversion to a VCL type
    fn func_may_fail(&self, info: &FuncInfo) -> bool {
        info.args.iter().any(|arg| matches!(&arg.ty, ParamType::VclName(p) | ParamType::Value(p) if p.ty_info.use_try_from()))
            || info.out_result
            || (self.output_hdr != "VCL_VOID"
                && !matches!(info.output_ty, OutputTy::Default | OutputTy::VclType(_)))
    }

    /// Get the signature of the wrapper function:  `unsafe extern "C" fn name(ctx: *mut vrt_ctx, ...) -> VCL_STRING`
    /// If `with_fn_name` is true, the function name is included in the signature
    fn get_wrapper_fn_sig(&self, with_fn_name: bool) -> TokenStream {
        let fn_name = with_fn_name.then(|| self.names.wrapper_fn_name().to_ident());
        let fn_args = &self.wrap_fn_arg_decl;
        let fn_output = &self.wrap_fn_output;
        quote! { unsafe extern "C" fn #fn_name(#(#fn_args),*) #fn_output }
    }

    /// Get the Rust args struct in case optional arguments are being used
    fn gen_opt_param_struct(&self, info: &FuncInfo) -> TokenStream {
        if info.has_optional_args {
            let opt_args_ty_name = self.opt_args_ty_name.to_ident();
            let opt_args_arg_decl = &self.opt_args_arg_decl;
            quote! {
                #[repr(C)]
                struct #opt_args_ty_name {
                    #(#opt_args_arg_decl,)*
                }
            }
        } else {
            quote! {}
        }
    }
}
