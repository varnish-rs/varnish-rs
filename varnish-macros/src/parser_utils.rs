use proc_macro2::Ident;
use quote::quote;
use syn::Expr::Lit;
use syn::Lit::Str;
use syn::Meta::NameValue;
use syn::PathArguments::AngleBracketed;
use syn::Type::{Path, Reference};
use syn::{Attribute, ExprLit, GenericArgument, MetaNameValue, PathSegment, Type, TypePath};

use crate::errors::error;
use crate::model::{FuncInfo, ObjInfo};
use crate::ProcResult;

/// iterator to go over all functions in a [`ObjInfo`], including constructor and destructor
pub struct ObjFuncIter<'a> {
    obj: &'a ObjInfo,
    idx: usize,
}

impl<'a> Iterator for ObjFuncIter<'a> {
    type Item = &'a FuncInfo;

    fn next(&mut self) -> Option<Self::Item> {
        let idx = self.idx;
        self.idx += 1;
        match idx {
            0 => Some(&self.obj.constructor),
            1 => Some(&self.obj.destructor),
            idx => self.obj.funcs.get(idx - 2),
        }
    }
}

impl ObjInfo {
    pub fn iter(&self) -> ObjFuncIter<'_> {
        ObjFuncIter { obj: self, idx: 0 }
    }
}

/// Remove an attribute from a list of attributes, returning the attribute if found.
pub fn remove_attr(attrs: &mut Vec<Attribute>, name: &str) -> Option<Attribute> {
    attrs
        .iter()
        .position(|attr| attr.path().is_ident(name))
        .map(|idx| attrs.swap_remove(idx))
}

/// Check if a field has a specific attribute
pub fn has_attr(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident(name))
}

/// Find an attribute by name
pub fn find_attr<'a>(attrs: &'a [Attribute], name: &str) -> Option<&'a Attribute> {
    attrs.iter().find(|attr| attr.path().is_ident(name))
}

/// Try to get the inner types of the `Result<Ok, Err>` type, or return None if it's not a `Result<Ok, Err>`.
pub fn as_result_type(ty: &Type) -> Option<&Type> {
    if let Path(type_path) = ty {
        if let Some(PathSegment { ident, arguments }) = type_path.path.segments.last() {
            if ident == "Result" {
                if let AngleBracketed(args) = &arguments {
                    if args.args.len() == 2 {
                        if let Some(GenericArgument::Type(ok_ty)) = args.args.first() {
                            // Compiler will check if Err type can be coerced into VclError
                            return Some(ok_ty);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Try to get the inner type of the `Option<T>`, or return None if it's not an `Option<T>`.
pub fn as_option_type(ty: &Type) -> Option<&Type> {
    as_one_gen_type(ty, "Option")
}

/// Try to get the inner type of the `Box<T>`, or return None if it's not a `Box<T>`.
pub fn as_box_type(ty: &Type) -> Option<&Type> {
    as_one_gen_type(ty, "Box")
}

/// Try to get the inner type of `__name__<T>` type with one argument, or return None if it's not a generic type with one argument.
fn as_one_gen_type<'a>(ty: &'a Type, name: &'static str) -> Option<&'a Type> {
    if let Some(GenericArgument::Type(inner_ty)) = as_one_gen_arg(ty, name) {
        Some(inner_ty)
    } else {
        None
    }
}

/// Get the single generic argument of a type, if it's a generic type with one argument.
pub fn as_one_gen_arg<'a>(ty: &'a Type, name: &'static str) -> Option<&'a GenericArgument> {
    if let Path(type_path) = ty {
        if let Some(PathSegment { ident, arguments }) = type_path.path.segments.last() {
            if ident == name {
                if let AngleBracketed(args) = &arguments {
                    if args.args.len() == 1 {
                        return args.args.first();
                    }
                }
            }
        }
    }
    None
}

/// Try to get the inner type of the `&T` reference, or return None if it's not a `&T` reference.
pub fn as_ref_ty(ty: &Type) -> Option<&Type> {
    if let Reference(rf) = ty {
        if rf.mutability.is_none() {
            return Some(&rf.elem);
        }
    }
    None
}

/// Try to get the inner type of the `&mut T` reference, or return None if it's not a `&mut T` reference.
pub fn as_ref_mut_ty(ty: &Type) -> Option<&Type> {
    if let Reference(rf) = ty {
        if rf.mutability.is_some() {
            return Some(&rf.elem);
        }
    }
    None
}

/// Try to get the inner type of the `[T]` slice, or return None if it's not a `[T]` slice.
pub fn as_slice_ty(ty: &Type) -> Option<&Type> {
    if let Type::Slice(slice) = ty {
        return Some(&slice.elem);
    }
    None
}

/// Try to get the ident of a simple type, or return None if it's not a simple type.
pub fn as_simple_ty(ty: &Type) -> Option<&Ident> {
    if let Path(TypePath { qself: None, path }) = ty {
        path.get_ident()
    } else {
        None
    }
}

/// Save/validate shared mut `T` into the store. Must be declared as `&mut Option<Box<T>>`
pub fn parse_shared_mut(store: &mut Option<String>, arg_ty: &Type) -> ProcResult<()> {
    let val = as_ref_mut_ty(arg_ty)
        .and_then(as_option_type)
        .and_then(as_box_type);
    store_shared(store, arg_ty, val, true)
}

/// Save/validate shared ref `T` into the store. Must be declared as `Option<&T>`
pub fn parse_shared_ref(store: &mut Option<String>, arg_ty: &Type) -> ProcResult<()> {
    let val = as_option_type(arg_ty).and_then(as_ref_ty);
    store_shared(store, arg_ty, val, false)
}

use syn::visit_mut::VisitMut;
use syn::Lifetime;

struct AnonymizeLifetimes;

impl VisitMut for AnonymizeLifetimes {
    fn visit_lifetime_mut(&mut self, lifetime: &mut Lifetime) {
        lifetime.ident = Ident::new("_", lifetime.ident.span());
    }
}

/// When processing a fn arg tagged with `#[shared_per_task]` or `#[shared_per_vcl]`,
/// we need to ensure that the shared type is the same everywhere. This function
/// stores the shared type into the `store`, or if it is already non-None, it checks
/// that the type is the same.  This is a helper function for `parse_shared_mut` and `parse_shared_ref`.
fn store_shared(
    store: &mut Option<String>,
    arg_ty: &Type,
    ty: Option<&Type>,
    is_mut: bool,
) -> ProcResult<()> {
    let Some(ty) = ty else {
        let msg = if is_mut {
            "This params must be declared as `&mut Option<Box<...>>`"
        } else {
            "This params must be declared as `Option<&...>`"
        };
        Err(error(arg_ty, msg))?
    };

    // For later usage, we need to anonymize all lifetimes, replacing 'foo with '_
    let mut ty = ty.clone();
    AnonymizeLifetimes.visit_type_mut(&mut ty);
    let ty = quote! { #ty }.to_string();

    if let Some(t) = store {
        if t != &ty {
            let msg = format!(
                "Shared type must be the same everywhere. Another shared param used type `{t}`."
            );
            Err(error(arg_ty, &msg))?;
        }
    } else {
        // Ensure we can parse the types later, but we need to store it as a string to avoid lifetime issues
        if let Err(e) = syn::parse_str::<Type>(&ty) {
            let msg = format!(
                "Internal error, please report: unable to re-parse this from a string '{ty}'"
            );
            Err(syn::Error::new(e.span(), msg))?;
        }

        *store = Some(ty);
    }

    Ok(())
}

/// Parse the doc string from the `#[doc]` attributes, and remove them from the list of attributes.
/// This is required for the argument docs because they are not supported by Rust compiler
pub fn parse_and_rm_doc(attrs: &mut Vec<Attribute>) -> String {
    let docs = parse_doc_str(attrs);
    // there can be more than one doc attribute, so we need to remove all of them
    while remove_attr(attrs, "doc").is_some() {}
    docs
}

/// Parse the doc string from the `#[doc]` attributes, and return it as a string.
pub fn parse_doc_str(attrs: &[Attribute]) -> String {
    // Adapted from https://github.com/hasura/graphql-engine/blob/a29101a3e9fe8c624fb09ec892c21da4b2bdaaba/v3/crates/utils/opendds-derive/src/helpers.rs#L35
    // Under MIT / Apache2.0 license
    let attrs = attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("doc") {
                if let NameValue(MetaNameValue {
                    value:
                        Lit(ExprLit {
                            lit: Str(ref lit_str),
                            ..
                        }),
                    ..
                }) = attr.meta
                {
                    Some(lit_str.value())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let mut lines = attrs
        .iter()
        .flat_map(|a| a.split('\n'))
        .map(str::trim)
        .skip_while(|s| s.is_empty())
        .collect::<Vec<_>>();

    if let Some(&"") = lines.last() {
        lines.pop();
    }

    // Added for backward-compatibility, but perhaps we shouldn't do this
    // https://github.com/rust-lang/rust/issues/32088
    if lines.iter().all(|l| l.starts_with('*')) {
        for line in &mut lines {
            *line = line[1..].trim();
        }
        while let Some(&"") = lines.first() {
            lines.remove(0);
        }
    }

    lines.join("\n")
}
