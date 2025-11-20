use std::collections::BTreeMap;
use std::fmt::Display;
use std::mem::size_of;
use std::str::FromStr;
use std::sync::atomic::AtomicU64;

use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;
use serde::Serialize;
use syn::meta::ParseNestedMeta;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::Comma;
use syn::{Data, DeriveInput, Field, Fields, Type};

use crate::errors::Errors;
use crate::parser_utils::{find_attr, has_attr, parse_doc_str};
use crate::ProcResult;

type FieldList = Punctuated<Field, Comma>;

#[derive(Serialize, Clone)]
enum CType {
    #[serde(rename = "uint64_t")]
    Uint64,
}

impl CType {
    const fn size(&self) -> usize {
        match self {
            CType::Uint64 => size_of::<AtomicU64>(),
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "lowercase")]
enum MetricType {
    Counter,
    Gauge,
    Bitmap,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "lowercase")]
enum Level {
    #[default]
    Info,
    Diag,
    Debug,
}

impl FromStr for Level {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "info" => Ok(Level::Info),
            "diag" => Ok(Level::Diag),
            "debug" => Ok(Level::Debug),
            _ => Err(format!(
                "Invalid level value '{s}'. Must be one of: info, diag, debug"
            )),
        }
    }
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "lowercase")]
enum Format {
    #[default]
    Integer,
    Bitmap,
    Duration,
    Bytes,
}

impl FromStr for Format {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "integer" => Ok(Format::Integer),
            "bitmap" => Ok(Format::Bitmap),
            "duration" => Ok(Format::Duration),
            "bytes" => Ok(Format::Bytes),
            _ => Err(format!(
                // TODO: can s come from an untrusted source? If so, we should not include it
                "Invalid format value '{s}'. Must be one of: integer, bitmap, duration, bytes"
            )),
        }
    }
}

#[derive(Serialize, Clone)]
struct VscMetricDef {
    pub name: String,
    #[serde(rename = "type")]
    pub metric_type: MetricType,
    pub ctype: CType,
    pub level: Level,
    pub oneliner: String, // "Counts the number of X", etc
    pub format: Format,
    pub docs: String,
    pub index: Option<usize>,
}

#[derive(Serialize)]
struct VscMetadata {
    version: &'static str,
    name: String,
    oneliner: String,
    order: u32,
    docs: String,
    elements: usize,
    // Using BTreeMap to ensure the order of elements
    elem: BTreeMap<String, VscMetricDef>,
}

pub fn derive_vsc_metric(input: &DeriveInput) -> ProcResult<TokenStream> {
    let name = &input.ident;

    if !has_repr_c(input) {
        return Err(syn::Error::new(
            name.span(),
            "VSC structs must be marked with #[repr(C)] for correct memory layout",
        )
        .into());
    }

    let fields = get_struct_fields(&input.data);
    validate_fields(fields)?;

    let metadata = generate_metadata_json(name, fields)?;
    let hashes = metadata
        .split('"')
        .skip(1) // Skip the first part which is before the first quote
        .map(|s| s.chars().take_while(|&c| c == '#').count() + 1)
        .max()
        .unwrap_or_default();
    let hashes = "#".repeat(hashes);
    let metadata = Literal::from_str(&format!("cr{hashes}\"{metadata}\"{hashes}")).unwrap();
    Ok(quote! {
        unsafe impl varnish::VscMetric for #name {
            fn get_metadata() -> &'static std::ffi::CStr {
                #metadata
            }
        }
    })
}

pub fn get_struct_fields(data: &Data) -> &FieldList {
    // FIXME: use errors collection
    match data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => panic!("Only named fields are supported"),
        },
        _ => panic!("Only structs are supported"),
    }
}

pub fn validate_fields(fields: &FieldList) -> ProcResult<()> {
    let mut errors = Errors::new();
    for field in fields {
        match &field.ty {
            Type::Path(path) => {
                let is_atomic_u64 = path
                    .path
                    .segments
                    .last()
                    .is_some_and(|seg| seg.ident == "AtomicU64");

                if !is_atomic_u64 {
                    let field_name = field.ident.as_ref().map(ToString::to_string).unwrap();
                    errors.push(syn::Error::new(
                        Spanned::span(field),
                        format!("Field {field_name} must be of type AtomicU64"),
                    ));
                }
            }
            _ => {
                errors.push(syn::Error::new(
                    Spanned::span(field),
                    "Field types must be of type AtomicU64",
                ));
            }
        }
    }
    Ok(errors.into_result()?)
}

fn generate_metrics(fields: &FieldList) -> ProcResult<BTreeMap<String, VscMetricDef>> {
    let mut offset = 0;
    let mut result = BTreeMap::new();

    for field in fields {
        let name = field.ident.as_ref().unwrap().to_string();

        let has_counter = has_attr(&field.attrs, "counter");
        let has_gauge = has_attr(&field.attrs, "gauge");
        let has_bitmap = has_attr(&field.attrs, "bitmap");
        let metric_type = match (has_counter, has_gauge, has_bitmap) {
            (true, false, false) => MetricType::Counter,
            (false, true, false) => MetricType::Gauge,
            (false, false, true) => MetricType::Bitmap,
            (false, false, false) => {
                // FIXME: use errors collection
                panic!("Field {name} must have either #[counter], #[gauge], or #[bitmap] attribute")
            }
            _ => {
                // FIXME: use errors collection
                panic!("Field {name} cannot have multiple metric type attributes (#[counter], #[gauge], #[bitmap])")
            }
        };

        let doc_str = parse_doc_str(&field.attrs);
        let mut doc_lines = doc_str.split('\n').filter(|s| !s.is_empty());
        let oneliner = doc_lines.next().unwrap_or_default().to_string();
        let docs = doc_lines.next().unwrap_or_default().to_string();

        let (level, format) = parse_metric_attributes(
            field,
            match metric_type {
                MetricType::Counter => "counter",
                MetricType::Gauge => "gauge",
                MetricType::Bitmap => "bitmap",
            },
        )?;

        let ctype = CType::Uint64;
        let index = Some(offset);
        offset += ctype.size();

        result.insert(
            name.clone(),
            VscMetricDef {
                name,
                metric_type,
                ctype,
                level,
                oneliner,
                format,
                docs,
                index,
            },
        );
    }

    Ok(result)
}

pub fn generate_metadata_json(name: &Ident, fields: &FieldList) -> ProcResult<String> {
    let metrics = generate_metrics(fields)?;

    let metadata = VscMetadata {
        version: "1",
        name: name.to_string(),
        oneliner: format!("{name} statistics"),
        order: 100,
        docs: String::new(),
        elements: metrics.len(),
        elem: metrics,
    };

    Ok(
        serde_json::to_string(&metadata)
            .map_err(|e| syn::Error::new(name.span(), e.to_string()))?,
    )
}

#[expect(clippy::unnecessary_wraps)]
fn parse_metric_attributes(field: &Field, metric_type: &str) -> ProcResult<(Level, Format)> {
    /// Helper function to parse the attribute value
    fn parse<T: FromStr>(id: &Ident, meta: &ParseNestedMeta, field: &Field) -> Result<T, syn::Error>
    where
        <T as FromStr>::Err: Display,
    {
        let value = meta.value()?.parse::<syn::LitStr>()?.value();
        T::from_str(&value).map_err(|e| {
            meta.error(format!(
                "Invalid {id} value for field {}: {e}",
                field.ident.as_ref().unwrap()
            ))
        })
    }

    let mut level = Level::default();
    let mut format = Format::default();
    if let Some(attrs) = find_attr(&field.attrs, metric_type) {
        // FIXME: parse_nested_meta() returns an error:
        //     "expected attribute arguments in parentheses ..."
        //   ignoring seems to work, but that's a strange approach and needs fixing
        let _ = attrs.parse_nested_meta(|meta| {
            if let Some(ident) = meta.path.get_ident() {
                if ident == "level" {
                    level = parse(ident, &meta, field)?;
                } else if ident == "format" {
                    format = parse(ident, &meta, field)?;
                }
            }
            Ok(())
        });
    }
    Ok((level, format))
}

pub fn has_repr_c(input: &DeriveInput) -> bool {
    input.attrs.iter().any(|attr| {
        if !attr.path().is_ident("repr") {
            return false;
        }

        let Ok(meta) = attr.parse_args::<syn::Meta>() else {
            return false;
        };

        matches!(meta, syn::Meta::Path(path) if path.is_ident("C"))
    })
}
