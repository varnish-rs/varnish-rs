//! The data model is the validated result of parsing user code.
//! Once fully parsed and vetted, the data model is used to generate the Varnish VMOD code.

use std::iter::once;

use darling::FromMeta;

/// Represents the entire VMOD. A single instance of this struct is parsed for each VMOD.
#[derive(Debug, Default)]
pub struct VmodInfo {
    pub params: VmodParams,
    pub ident: String,
    pub docs: String,
    pub funcs: Vec<FuncInfo>,
    pub objects: Vec<ObjInfo>,
    pub shared_types: SharedTypes,
}

impl VmodInfo {
    fn iter_all_funcs(&self) -> impl Iterator<Item = &FuncInfo> {
        self.funcs.iter().chain(self.objects.iter().flat_map(|o| {
            o.funcs
                .iter()
                .chain(once(&o.constructor).chain(once(&o.destructor)))
        }))
    }

    pub fn count_funcs<F: FnMut(&&FuncInfo) -> bool>(&self, filter: F) -> usize {
        self.iter_all_funcs().filter(filter).count()
    }

    pub fn count_args<F: Copy + Fn(&&ParamTypeInfo) -> bool>(&self, filter: F) -> usize {
        self.iter_all_funcs().map(|f| f.count_args(filter)).sum()
    }
}

/// Represents the shared types used by multiple functions. Each of these types is unique per VMOD.
#[derive(Debug, Default)]
pub struct SharedTypes {
    pub shared_per_task_ty: Option<String>,
    pub shared_per_vcl_ty: Option<String>,
}

impl SharedTypes {
    pub fn get_per_vcl_ty(&self) -> &str {
        self.shared_per_vcl_ty.as_deref().unwrap_or("()")
    }
}

/// Represents the parameters inside the `#[vmod(....)]` attribute itself.
#[derive(Default, Debug, FromMeta)]
#[darling(default)]
pub struct VmodParams {
    pub docs: Option<String>,
}

/// Represents the object information parsed from an `impl` block.
#[derive(Debug)]
pub struct ObjInfo {
    pub ident: String,
    pub docs: String,
    pub constructor: FuncInfo,
    pub destructor: FuncInfo,
    pub funcs: Vec<FuncInfo>,
}

/// Represents the function information parsed from a function or method.
#[derive(Debug)]
pub struct FuncInfo {
    pub func_type: FuncType,
    pub ident: String,
    pub docs: String,
    pub has_optional_args: bool,
    pub args: Vec<ParamTypeInfo>,
    pub output_ty: OutputTy,
    pub out_result: bool,
}

impl FuncInfo {
    pub fn count_args<F: Fn(&&ParamTypeInfo) -> bool>(&self, filter: F) -> usize {
        self.args.iter().filter(filter).count()
    }
}

/// What kind of function is this?
#[derive(Debug, Clone, Copy, Default)]
pub enum FuncType {
    #[default]
    Function,
    Constructor,
    Destructor,
    Method,
    Event,
}

impl FuncType {
    pub fn to_vcc_type(self) -> &'static str {
        match self {
            Self::Function => "$FUNC",
            Self::Constructor => "$INIT",
            Self::Destructor => "$FINI",
            Self::Method => "$METHOD",
            Self::Event => "$EVENT",
        }
    }
}

/// Represents the information about a single function argument.
#[derive(Debug)]
pub struct ParamTypeInfo {
    pub ident: String,
    pub docs: String,
    pub ty: ParamType,
}

/// Represents the type of the function argument.
#[derive(Debug, Clone)]
pub enum ParamType {
    /// An argument representing Varnish context (`VRT_CTX`) wrapper
    Context { is_mut: bool },
    /// An argument representing Varnish Workspace wrapper
    Workspace { is_mut: bool },
    /// For object methods, the first argument is always a reference to the object
    SelfType,
    /// An argument is an event type
    Event,
    /// A `&str` or `&CStr` argument automatically passed for object creation representing a VCL name.
    VclName(ParamInfo),
    /// An argument `&mut Option<Box<T>>` representing any Rust name and type shared across tasks (i.e. `PRIV_TASK`)
    SharedPerTask,
    /// A readonly argument `Option<&T>` representing any Rust name and type shared across VCL load (i.e. `PRIV_VCL`)
    SharedPerVclRef,
    /// A mutable argument `&mut Option<Box<T>>` representing any Rust name and type shared across VCL load (i.e. `PRIV_VCL`)
    SharedPerVclMut,
    /// An argument is a fetch filter registry
    FetchFilters,
    /// An argument is a delivery filter registry
    DeliveryFilters,
    /// An argument representing a basic VCL type
    Value(ParamInfo),
}

#[derive(Debug, Clone)]
pub enum ParamKind {
    /// Type is declared without the `Option<...>`
    Regular,
    /// Type is declared with the `Option<...>`
    Optional,
    /// Type is declared with the `Option<...>`, but has a `#[required]` attribute.
    /// This means it must be present when calling the function, but it could be `NULL`.
    Required,
}

/// Represents the information about the common function argument types
#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub kind: ParamKind,
    pub default: serde_json::Value,
    pub ty_info: ParamTy,
}

/// Represents the common function argument types. These could also be returned.
#[derive(Debug, Clone)]
pub enum ParamTy {
    Bool,
    Blob,
    Duration,
    F64,
    I64,
    Probe, // FIXME: can probes be returned?
    ProbeCow,
    SocketAddr,
    Str,
    CStr,
    VclType(String), // Raw VCL types like VCL_BACKEND, VCL_ACL
}

impl ParamTy {
    pub fn to_vcc_type(&self) -> &str {
        match self {
            Self::Bool => "BOOL",
            Self::Blob => "BLOB",
            Self::Duration => "DURATION",
            Self::F64 => "REAL",
            Self::I64 => "INT",
            Self::Probe | Self::ProbeCow => "PROBE",
            Self::SocketAddr => "IP",
            Self::Str | Self::CStr => "STRING",
            Self::VclType(s) => &s[4..], // Strip "VCL_" prefix
        }
    }

    pub fn to_c_type(&self) -> &str {
        // ATTENTION: Each VCL_* type here must also be listed in the `use varnish::...`
        //            statement in the `varnish-macros/src/generator.rs` file.
        match self {
            Self::Bool => "VCL_BOOL",
            Self::Blob => "VCL_BLOB",
            Self::Duration => "VCL_DURATION",
            Self::F64 => "VCL_REAL",
            Self::I64 => "VCL_INT",
            Self::Probe | Self::ProbeCow => "VCL_PROBE",
            Self::SocketAddr => "VCL_IP",
            Self::Str | Self::CStr => "VCL_STRING",
            Self::VclType(s) => s, // Return as-is
        }
    }

    /// User MUST use some types with `Option`
    pub fn must_be_optional(&self) -> bool {
        match self {
            Self::Bool
            | Self::Blob
            | Self::Duration
            | Self::F64
            | Self::I64
            | Self::Str
            | Self::CStr => false,
            Self::Probe | Self::ProbeCow | Self::SocketAddr | Self::VclType(_) => true,
        }
    }

    /// Some VCL->Rust conversions require `TryFrom` instead of `From`,
    /// e.g. if `&CStr` contains invalid UTF-8 characters and cannot be converted to `&str`.
    pub fn use_try_from(&self) -> bool {
        match self {
            Self::Probe
            | Self::ProbeCow
            | Self::SocketAddr
            | Self::Bool
            | Self::Duration
            | Self::F64
            | Self::I64
            | Self::CStr
            | Self::Blob
            | Self::VclType(_) => false,
            Self::Str => true,
        }
    }

    /// Raw VCL types (VCL_BACKEND, etc.) need explicit null pointer checking
    /// instead of using `.into()` because we can't override the blanket
    /// `impl<T> From<T> for Option<T>` from core.
    pub fn needs_null_check(&self) -> bool {
        matches!(self, Self::VclType(_))
    }
}

/// Represents all return types of functions.
#[derive(Debug, Clone)]
pub enum OutputTy {
    Default, // Nothing is returned
    SelfType,
    ParamType(ParamTy),
    String,
    Bytes,
    VclType(String), // Raw VCL type, stored as original "VCL_..." string
}

impl OutputTy {
    pub fn to_vcc_type(&self) -> String {
        match self {
            // Self is returned by obj constructors which are void in VCC
            Self::Default | Self::SelfType => "VOID".into(),
            Self::ParamType(ty) => ty.to_vcc_type().into(),
            Self::Bytes | Self::String => "STRING".into(),
            Self::VclType(ty) => ty[4..].to_string(), // remove "VCL_" prefix
        }
    }

    pub fn to_c_type(&self) -> String {
        // ATTENTION: Each VCL_* type here must also be listed in the `use varnish::...`
        //            statement in the `varnish-macros/src/generator.rs` file.
        match self {
            Self::ParamType(ty) => ty.to_c_type().into(),
            Self::Bytes | Self::String => "VCL_STRING".into(),
            Self::SelfType | Self::Default => "VCL_VOID".into(),
            Self::VclType(ty) => ty.into(),
        }
    }
}
