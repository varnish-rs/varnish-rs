//! Parser takes in the token stream and generates the model structure.
//! The model describes the functions, objects, and events that should be exposed to Varnish.
//! The model should be thoroughly validated before generating the output, and is treated as the source of truth.

use darling::ast::NestedMeta;
use darling::FromMeta;
use proc_macro2::TokenStream;
use syn::{Attribute, ImplItem, Item, ItemImpl, ItemMod, ReturnType, Signature, Visibility};

use crate::errors::Errors;
use crate::model::{
    FuncInfo, FuncType, ObjInfo, OutputTy, ParamKind, ParamType, ParamTypeInfo, SharedTypes,
    VmodInfo, VmodParams,
};
use crate::parser_args::FuncStatus;
use crate::{parser_utils, ProcResult};

pub fn tokens_to_model(args: TokenStream, item_mod: &mut ItemMod) -> ProcResult<VmodInfo> {
    let args = NestedMeta::parse_meta_list(args)?;
    let args = VmodParams::from_list(&args)?;
    let info = VmodInfo::parse(args, item_mod)?;
    Ok(info)
}

impl VmodInfo {
    /// Parse the `mod` item and generate the model of everything
    fn parse(params: VmodParams, item: &mut ItemMod) -> ProcResult<Self> {
        let mut errors = Errors::new();
        let mut funcs = Vec::<FuncInfo>::new();
        let mut objects = Vec::<ObjInfo>::new();
        let mut shared_types = SharedTypes::default();

        if let Some((_, content)) = &mut item.content {
            for item in content {
                match item {
                    Item::Fn(fn_item) => {
                        // a function or an event handler
                        let func = FuncInfo::parse(
                            &mut shared_types,
                            &mut fn_item.sig,
                            &fn_item.vis,
                            &mut fn_item.attrs,
                            false,
                        );
                        if let Some(func) = errors.on_err(func) {
                            funcs.push(func);
                        }
                    }
                    Item::Impl(impl_item) => {
                        // an object
                        if let Some(obj) =
                            errors.on_err(ObjInfo::parse(impl_item, &mut shared_types))
                        {
                            objects.push(obj);
                        }
                    }
                    Item::Use(_) => { /* ignore */ }
                    Item::Struct { .. } => {
                        errors.add(item, &err_msg_item_not_allowed("Structs"));
                    }
                    Item::Enum { .. } => {
                        errors.add(item, &err_msg_item_not_allowed("Enums"));
                    }
                    Item::Const(_) => {
                        errors.add(
                            item,
                            "Constants are not allowed in a `mod` tagged with `#[varnish::vmod]",
                        );
                    }
                    Item::Macro(_) => {
                        errors.add(
                            item,
                            "Macros are not allowed in a `mod` tagged with `#[varnish::vmod]",
                        );
                    }
                    Item::Mod(_) => {
                        errors.add(item, "Nested modules are not allowed in a `mod` tagged with `#[varnish::vmod]");
                    }
                    Item::Static(_) => {
                        errors.add(item, "Static variables are not allowed in a `mod` tagged with `#[varnish::vmod]");
                    }
                    Item::Trait(_) => {
                        errors.add(
                            item,
                            "Traits are not allowed in a `mod` tagged with `#[varnish::vmod]",
                        );
                    }
                    Item::TraitAlias(_) => {
                        errors.add(item, "Trait aliases are not allowed in a `mod` tagged with `#[varnish::vmod]");
                    }
                    Item::Type(_) => {
                        errors.add(
                            item,
                            "Type aliases are not allowed in a `mod` tagged with `#[varnish::vmod]",
                        );
                    }
                    Item::Union(_) => {
                        errors.add(
                            item,
                            "Unions are not allowed in a `mod` tagged with `#[varnish::vmod]",
                        );
                    }
                    _ => {
                        errors.add(item, "Only functions and impl blocks are allowed inside a `mod` tagged with `#[varnish::vmod]`");
                    }
                }
            }
        }
        let info = Self {
            params,
            ident: item.ident.to_string(),
            docs: parser_utils::parse_doc_str(&item.attrs),
            shared_types,
            funcs,
            objects,
        };
        info.validate(item, &mut errors);
        errors.into_result()?;
        Ok(info)
    }

    pub fn validate(&self, item: &ItemMod, errors: &mut Errors) {
        if self.count_funcs(|v| matches!(v.func_type, FuncType::Event)) > 1 {
            errors.add(
                &item,
                "More than one event handler found. Only one event handler is allowed",
            );
        }
        let per_vcl_mut = self.count_args(|v| matches!(v.ty, ParamType::SharedPerVclMut));
        let per_vcl_ref = self.count_args(|v| matches!(v.ty, ParamType::SharedPerVclRef));
        if per_vcl_ref > 0 && per_vcl_mut == 0 {
            errors.add(
                &item,
                "#[shared_per_vcl] value has not been initialized. Add a `&mut Option<Box<...>>` param to an event handler or an object new() function",
            );
        }
        if self.funcs.is_empty() && self.objects.is_empty() && errors.is_empty() {
            // If another error is reported, most likely it was not added to funcs or objects, so we don't need to report this one
            errors.add(&self.ident, "No functions or objects found in this module");
        }
    }
}

fn err_msg_item_not_allowed(typ: &str) -> String {
    format!("{typ} are not allowed inside a `mod` tagged with `#[varnish::vmod]`.  Move it to an outer scope and keep just the `impl` block. More than one `impl` blocks are allowed.")
}

impl ObjInfo {
    /// Parse an `impl` block and treat all public functions as object methods
    fn parse(item_impl: &mut ItemImpl, shared_types: &mut SharedTypes) -> ProcResult<Self> {
        let mut errors = Errors::new();
        let ident = parser_utils::as_simple_ty(item_impl.self_ty.as_ref()).map(ToString::to_string);

        // Add only one error per object impl declaration
        if item_impl.trait_.as_ref().is_some() {
            errors.add(&item_impl, "Trait impls are not supported for object impls");
        } else if !item_impl.generics.params.is_empty() {
            errors.add(
                &item_impl.generics.params,
                "Generics are not supported for object impls",
            );
        } else if ident.is_none() {
            errors.add(
                &item_impl.self_ty,
                "Expected a simple type for object. If the object is defined elsewhere, use `use` to import it.",
            );
        }

        let mut funcs = Vec::new();
        let mut constructor = None;
        for item in &mut item_impl.items {
            if let ImplItem::Fn(fn_item) = item {
                let Some(func) = errors.on_err(FuncInfo::parse(
                    shared_types,
                    &mut fn_item.sig,
                    &fn_item.vis,
                    &mut fn_item.attrs,
                    true,
                )) else {
                    continue;
                };
                if func.ident == "new" {
                    constructor = Some(func);
                } else {
                    funcs.push(func);
                }
            }
        }

        if constructor.is_none() {
            errors.add(
                &item_impl.self_ty,
                "Object must have a constructor called `new`",
            );
        }

        errors.into_result()?;
        Ok(Self {
            ident: ident.expect("ident err already reported"),
            docs: parser_utils::parse_doc_str(&item_impl.attrs),
            constructor: constructor.expect("ctor err already reported"),
            destructor: FuncInfo {
                func_type: FuncType::Destructor,
                ident: "_fini".to_string(),
                docs: String::new(),
                has_optional_args: false,
                args: Vec::new(),
                output_ty: OutputTy::Default,
                out_result: false,
            },
            funcs,
        })
    }
}

impl FuncInfo {
    /// Parse a function or a method signature
    fn parse(
        shared_types: &mut SharedTypes,
        signature: &mut Signature,
        vis: &Visibility,
        attrs: &mut Vec<Attribute>,
        is_object: bool,
    ) -> ProcResult<Self> {
        let mut errors = Errors::new();

        if !matches!(vis, Visibility::Public(..)) {
            errors.add(
                signature, // cannot use `vis` because it might be `Inherited`
                "Only public functions and impl blocks are allowed inside a `mod` tagged with `#[varnish::vmod]`. Add `pub` or move this function outside of this mod.",
            );
        } else if signature.asyncness.is_some() {
            errors.add(signature, "async functions are not supported");
        }

        let func_type = if let Some(attr) = parser_utils::remove_attr(attrs, "event") {
            if is_object {
                errors.add(
                    &attr.meta,
                    "Event functions are not supported for object methods",
                );
            }
            FuncType::Event
        } else if is_object {
            if signature.ident == "new" {
                FuncType::Constructor
            } else {
                FuncType::Method
            }
        } else {
            FuncType::Function
        };

        let (output_ty, out_result) = match &signature.output {
            ReturnType::Default => (OutputTy::Default, false),
            ReturnType::Type(_, ty) => {
                if let Some(ty) = parser_utils::as_result_type(ty.as_ref()) {
                    (OutputTy::parse(ty, func_type)?, true)
                } else {
                    (OutputTy::parse(ty.as_ref(), func_type)?, false)
                }
            }
        };

        let mut status = FuncStatus::new(func_type);
        let mut args = Vec::new();

        for (idx, arg) in signature.inputs.iter_mut().enumerate() {
            let arg = ParamTypeInfo::parse(shared_types, &mut status, idx, arg);
            if let Some(arg) = errors.on_err(arg) {
                args.push(arg);
            }
        }

        let has_optional_args = args.iter().any(
            |arg| matches!(&arg.ty, ParamType::Value(v) if matches!(v.kind, ParamKind::Optional)),
        );

        let is_unsafe = signature.unsafety.is_some();
        let out_vcl = output_ty.requires_unsafe();
        if is_unsafe && !out_vcl {
            errors.add(signature, "functions and methods must not be tagged as `unsafe` unless they return a VCL_* type");
        } else if out_vcl && !is_unsafe {
            errors.add(
                signature,
                "functions and methods that return a VCL_* type must be tagged as `unsafe`",
            );
        }

        errors.into_result()?;
        Ok(Self {
            func_type,
            ident: signature.ident.to_string(),
            docs: parser_utils::parse_doc_str(attrs),
            has_optional_args,
            output_ty,
            out_result,
            args,
        })
    }
}
