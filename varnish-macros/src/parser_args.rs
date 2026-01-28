use darling::ast::NestedMeta;
use serde_json::Value;
use syn::Type::Tuple;
use syn::{FnArg, GenericArgument, Lit, Meta, Pat, PatType, Type};

use crate::errors::error;
use crate::model::FuncType::{Constructor, Event, Function, Method};
use crate::model::{
    FuncType, OutputTy, ParamInfo, ParamKind, ParamTy, ParamType, ParamTypeInfo, SharedTypes,
};
use crate::parser_utils::{
    as_one_gen_arg, as_option_type, as_ref_mut_ty, as_ref_ty, as_simple_ty, as_slice_ty,
    parse_and_rm_doc, parse_shared_mut, parse_shared_ref, remove_attr,
};
use crate::ProcResult;

/// Parser state for a function parser. This is not part of the model, but helps with error detection.
#[derive(Debug, Default)]
#[expect(clippy::struct_excessive_bools)]
pub struct FuncStatus {
    func_type: FuncType,
    has_ctx_or_ws: bool,
    has_shared_per_task: bool,
    has_shared_per_vcl: bool,
    has_event: bool,
    has_vcl_name: bool,
    has_fetch_filters: bool,
    has_delivery_filters: bool,
}

impl FuncStatus {
    pub fn new(func_type: FuncType) -> Self {
        Self {
            func_type,
            ..Default::default()
        }
    }
}

// /// Represents function parameter configuration
// #[derive(Debug, FromMeta)]
// struct ArgConfig {
//     #[darling(with = preserve_str_literal)]
//     pub default: Option<Expr>,
//     pub required: Option<bool>,
// }

impl ParamTypeInfo {
    /// Parse an argument of a function, including `&self` for methods.
    /// The actual argument type is parsed by [`ParamType::parse`].
    /// This function should produce only one error per argument.
    pub fn parse(
        shared_types: &mut SharedTypes,
        status: &mut FuncStatus,
        idx: usize,
        arg: &mut FnArg,
    ) -> ProcResult<Self> {
        match arg {
            FnArg::Receiver(recv) => match status.func_type {
                Method => {
                    if idx != 0 || recv.reference.is_none() || recv.mutability.is_some() {
                        Err(error(&recv, "First method arg must be `&self`"))?;
                    }
                    Ok(Self {
                        ident: "self".to_string(),
                        docs: parse_and_rm_doc(&mut recv.attrs),
                        ty: ParamType::SelfType,
                    })
                }
                _ => Err(error(&arg, "`self` is not allowed for this function"))?,
            },
            FnArg::Typed(pat_ty) => {
                let ty = ParamType::parse(shared_types, pat_ty, status)?;
                // compute arg name
                let Pat::Ident(ident) = pat_ty.pat.as_ref() else {
                    Err(error(&pat_ty, "unsupported argument pattern"))?
                };
                Ok(Self {
                    ident: ident.ident.to_string(),
                    docs: parse_and_rm_doc(&mut pat_ty.attrs),
                    ty,
                })
            }
        }
    }
}

impl ParamType {
    /// Parse an argument type, including the magical types like shared types and context.
    #[expect(clippy::too_many_lines)]
    fn parse(
        shared_types: &mut SharedTypes,
        pat_ty: &mut PatType,
        status: &mut FuncStatus,
    ) -> ProcResult<Self> {
        // Make param validation a bit more readable
        macro_rules! error {
            ($msg:expr) => {
                Err(error(pat_ty, $msg))?
            };
        }
        macro_rules! unique {
            ($var:ident, $msg:expr) => {
                if status.$var {
                    error!($msg);
                }
                status.$var = true;
            };
        }
        macro_rules! only_in {
            ($pat:pat, $msg:expr) => {
                if !matches!(status.func_type, $pat) {
                    error!($msg);
                }
            };
        }
        macro_rules! not_in {
            ($pat:pat, $msg:expr) => {
                if matches!(status.func_type, $pat) {
                    error!($msg);
                }
            };
        }

        let attr_count = pat_ty.attrs.len();
        let is_per_task = remove_attr(&mut pat_ty.attrs, "shared_per_task");
        let is_per_vcl = remove_attr(&mut pat_ty.attrs, "shared_per_vcl");
        let is_vcl_name = remove_attr(&mut pat_ty.attrs, "vcl_name");
        if pat_ty.attrs.len() + 1 < attr_count {
            error! { "At most one of `shared_per_task`, `shared_per_vcl`, or `vcl_name` attributes can be used on a parameter" }
        }

        let arg_ty = pat_ty.ty.as_ref();
        Ok(if is_per_task.is_some() {
            parse_shared_mut(&mut shared_types.shared_per_task_ty, arg_ty)?;
            not_in! { Event, "Event functions must not have any #[shared_per_task] arguments." }
            unique! { has_shared_per_task, "#[shared_per_task] param is allowed only once in a function args list" }
            Self::SharedPerTask
        } else if is_per_vcl.is_some() {
            if matches!(status.func_type, Constructor | Event) {
                parse_shared_mut(&mut shared_types.shared_per_vcl_ty, arg_ty)?;
                unique! { has_shared_per_vcl, "#[shared_per_vcl] param is allowed only once in a function args list" }
                Self::SharedPerVclMut
            } else if matches!(status.func_type, Function | Method) {
                parse_shared_ref(&mut shared_types.shared_per_vcl_ty, arg_ty)?;
                unique! { has_shared_per_vcl, "#[shared_per_vcl] param is allowed only once in a function args list" }
                Self::SharedPerVclRef
            } else {
                error! { "#[shared_per_vcl] params can only be used in functions, object constructors, methods, and event handlers" }
            }
        } else if is_vcl_name.is_some() {
            only_in! { Constructor, "#[vcl_name] params are only allowed in object constructors" }
            unique! { has_vcl_name, "#[vcl_name] param is allowed only once in a function args list" }
            let arg_ty = match ParamTy::try_parse(arg_ty) {
                Some(ty) if matches!(ty, ParamTy::Str | ParamTy::CStr) => ty,
                _ => error! { "#[vcl_name] params must be declared as `&str` or `&CStr`" },
            };
            Self::VclName(ParamInfo::new(arg_ty, Value::Null, ParamKind::Regular))
        } else if as_simple_ty(arg_ty)
            .filter(|ident| *ident == "Event")
            .is_some()
        {
            only_in! { Event, "Event parameters are only allowed in event handlers. Try adding `#[event]` to this function." }
            unique! { has_event, "Event param is allowed only once in a function args list" }
            Self::Event
        } else if let Some(arg_ty) = as_ref_ty(arg_ty)
            .and_then(as_simple_ty)
            .filter(|ident| *ident == "Ctx" || *ident == "Workspace")
        {
            unique! { has_ctx_or_ws, "Context or Workspace param is allowed only once in a function args list" }
            if arg_ty == "Ctx" {
                Self::Context { is_mut: false }
            } else {
                Self::Workspace { is_mut: false }
            }
        } else if let Some(arg_ty) = as_ref_mut_ty(arg_ty)
            .and_then(as_simple_ty)
            .filter(|ident| *ident == "Ctx" || *ident == "Workspace")
        {
            unique! { has_ctx_or_ws, "Context or Workspace param is allowed only once in a function args list" }
            if arg_ty == "Ctx" {
                Self::Context { is_mut: true }
            } else {
                Self::Workspace { is_mut: true }
            }
        } else if as_ref_mut_ty(arg_ty)
            .and_then(as_simple_ty)
            .filter(|ident| *ident == "FetchFilters")
            .is_some()
        {
            only_in! { Constructor | Event, if let Function = status.func_type {
                "FetchFilters parameters are only allowed in object constructors and event handlers. Is this function missing `#[event]`?"
            } else {
                "FetchFilters parameters are only allowed in object constructors and event handlers."
            } }
            unique! { has_fetch_filters, "A FetchFilters param is allowed only once in a function args list" }
            Self::FetchFilters
        } else if as_ref_mut_ty(arg_ty)
            .and_then(as_simple_ty)
            .filter(|ident| *ident == "DeliveryFilters")
            .is_some()
        {
            only_in! { Constructor | Event, if let Function = status.func_type {
                "DeliveryFilters parameters are only allowed in object constructors and event handlers. Is this function missing `#[event]`?"
            } else {
                "DeliveryFilters parameters are only allowed in object constructors and event handlers."
            } }
            unique! { has_delivery_filters, "A DeliveryFilters param is allowed only once in a function args list" }
            Self::DeliveryFilters
        } else {
            // Only standard types left, possibly optional
            not_in! { Event, "Event functions can only have `Ctx`, `#[event] Event`, and `#[shared_per_vcl] &mut Option<Box<T>>` arguments." }
            let Some((opt, arg_ty)) = ParamTy::try_parse_or_optional(arg_ty) else {
                error! {"unsupported argument type" }
            };
            if !opt && arg_ty.must_be_optional() {
                error! { "This type of argument must be declared as optional with `Option<...>`" }
            }
            let default = Self::get_arg_opts(pat_ty, arg_ty)?;
            let has_required = Self::get_required_attr(pat_ty)?;
            let opt = if has_required {
                if !opt {
                    error! { "The `required` attribute is only allowed on Option<...> arguments" }
                }
                if !arg_ty.must_be_optional() && !matches!(arg_ty, ParamTy::CStr | ParamTy::Str) {
                    error! { "The `required` attribute is only allowed on CStr, str, Probe, ProbeCow, and SocketAddr arguments" }
                }
                ParamKind::Required
            } else if opt {
                ParamKind::Optional
            } else {
                ParamKind::Regular
            };
            Self::Value(ParamInfo::new(arg_ty, default, opt))
        })
    }

    /// Try to get the default value from the #[default(...)] attribute on an argument
    fn get_arg_opts(pat_ty: &mut PatType, arg_type: ParamTy) -> ProcResult<Value> {
        let Some(arg) = remove_attr(&mut pat_ty.attrs, "default") else {
            return Ok(Value::Null);
        };
        let Meta::List(arg) = arg.meta else {
            Err(error(&pat_ty, "Unexpected #[default(...)] attribute"))?
        };
        let arg = NestedMeta::parse_meta_list(arg.tokens)?;
        let [NestedMeta::Lit(lit)] = arg.as_slice() else {
            Err(error(&pat_ty, "Default value must be a literal value"))?
        };

        macro_rules! only {
            ($pat:pat, $msg:literal) => {
                if !matches!(arg_type, $pat) {
                    Err(error(&pat_ty, $msg))?;
                }
            };
        }

        Ok(match lit {
            Lit::Str(v) => {
                only! { ParamTy::Str | ParamTy::CStr, "Only `&str` and `&CStr` arguments can have a default string value" }
                Value::String(v.value())
            }
            Lit::CStr(v) => {
                only! { ParamTy::Str | ParamTy::CStr, "Only `&str` and `&CStr` arguments can have a default string value" }
                Value::String(v.value().to_str().unwrap().to_string())
            }
            Lit::Int(v) => {
                only! { ParamTy::I64, "Only `i64` arguments can have a default integer value" }
                serde_json::from_str(&v.to_string()).unwrap()
            }
            Lit::Float(v) => {
                only! { ParamTy::F64, "Only `f64` arguments can have a default float value" }
                serde_json::from_str(&v.to_string()).unwrap()
            }
            Lit::Bool(v) => {
                only! { ParamTy::Bool, "Only `bool` arguments can have a default boolean value" }
                Value::Number(i32::from(v.value).into())
            }
            _ => Err(error(&pat_ty, "Unrecognized value in #[default(...)]"))?,
        })
    }

    /// Try to get the #[required] attribute on an argument
    fn get_required_attr(pat_ty: &mut PatType) -> ProcResult<bool> {
        let Some(arg) = remove_attr(&mut pat_ty.attrs, "required") else {
            return Ok(false);
        };
        if let Meta::Path(syn::Path { segments, .. }) = arg.meta {
            if let Some(segment) = segments.last() {
                if segment.arguments.is_empty() {
                    return Ok(true);
                }
            }
        }
        Err(error(&pat_ty, "#[required] attribute must not have params"))?
    }
}

impl ParamInfo {
    fn new(ty_info: ParamTy, default: Value, optional: ParamKind) -> Self {
        Self {
            kind: optional,
            default,
            ty_info,
        }
    }
}

impl ParamTy {
    /// Tries parsing supported VCL types like `i64`, `Probe`, `Duration`, or Option-wrapped like `Option<&str>`.
    pub fn try_parse_or_optional(ty: &Type) -> Option<(bool, Self)> {
        if let Some(ty) = as_option_type(ty).and_then(Self::try_parse) {
            Some((true, ty))
        } else {
            Self::try_parse(ty).map(|ty| (false, ty))
        }
    }

    /// Tries parsing regular VCL types as `i64`, `bool`, `Duration`, `&str`, ...
    pub fn try_parse(ty: &Type) -> Option<Self> {
        if let Some(ident) = as_simple_ty(ty) {
            if ident == "bool" {
                return Some(Self::Bool);
            } else if ident == "BackendRef" {
                return Some(Self::BackendRef);
            } else if ident == "Duration" {
                return Some(Self::Duration);
            } else if ident == "f64" {
                return Some(Self::F64);
            } else if ident == "i64" {
                return Some(Self::I64);
            } else if ident == "Probe" {
                return Some(Self::Probe);
            } else if ident == "SocketAddr" {
                return Some(Self::SocketAddr);
            }
        }

        if let Some(GenericArgument::Lifetime(_)) = as_one_gen_arg(ty, "CowProbe") {
            return Some(Self::ProbeCow);
        }

        if let Some(ident) = as_ref_ty(ty).and_then(as_simple_ty) {
            if ident == "str" {
                return Some(Self::Str);
            } else if ident == "CStr" {
                return Some(Self::CStr);
            }
        }
        if let Some(ty) = as_ref_ty(ty).and_then(as_slice_ty).and_then(as_simple_ty) {
            if ty == "u8" {
                // `&[u8]`
                return Some(Self::Blob);
            }
        }

        None
    }
}

impl OutputTy {
    pub fn parse(ty: &Type, func_type: FuncType) -> ProcResult<Self> {
        let Some(ret_ty) = Self::try_parse(ty) else {
            Err(error(&ty, "This content type is not supported"))?
        };

        if matches!(func_type, Event) && !matches!(ret_ty, Self::Default) {
            Err(error(
                &ty,
                "Event functions must not return a value, or return a Result<(), _>",
            ))?;
        } else if !matches!(func_type, Constructor) && matches!(ret_ty, Self::SelfType) {
            Err(error(
                ty,
                "`Self` return type is only allowed in object constructors named `new`",
            ))?;
        }

        Ok(ret_ty)
    }

    fn try_parse(ty: &Type) -> Option<Self> {
        if let Some(ty) = ParamTy::try_parse(ty) {
            return Some(Self::ParamType(ty));
        }
        if let Some(ident) = as_simple_ty(ty) {
            if ident == "String" {
                return Some(Self::String);
            } else if ident == "Self" {
                return Some(Self::SelfType);
            }
            let ident = ident.to_string();
            if ident.starts_with("VCL_")
                && ident
                    .chars()
                    .all(|v| char::is_ascii_uppercase(&v) || v == '_')
            {
                return Some(Self::VclType(ident));
            }
        }
        if let Some(ty) = as_option_type(ty) {
            if let Some(ident) = as_simple_ty(ty) {
                if ident == "String" {
                    // `Option<String>`
                    return Some(Self::String);
                }
            }
            if let Some(ty) = as_ref_ty(ty).and_then(as_slice_ty).and_then(as_simple_ty) {
                if ty == "u8" {
                    // `&[u8]`
                    return Some(Self::Bytes);
                }
            }
        }
        if let Tuple(v) = ty {
            if v.elems.is_empty() {
                // `()`
                return Some(Self::Default);
            }
        }

        None
    }
}
