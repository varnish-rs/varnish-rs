use std::borrow::Cow;
use std::ffi::CStr;
use std::num::NonZeroUsize;
use std::str::Utf8Error;

// TODO: at some point we may want to add a `txt` variant - e.g. if user wants to handle error creation directly.

/// An `Error` describing all issues with the VCL module
///
/// This enum is used to describe all the possible errors that can happen when working with the VCL module,
/// or converting between Rust and C types.
#[derive(thiserror::Error, Debug)]
pub enum VclError {
    /// Create a new `Error` from a string
    #[error("{0}")]
    String(String),
    /// Create a new `Error` from a string slice
    #[error("{0}")]
    Str(&'static str),
    /// Create a new `VclError` from a C string
    #[error("{}", cstr_to_string(.0))]
    CStr(&'static CStr),
    /// In case Workspace allocation fails
    #[error("Unable to allocate {0} bytes in a Workspace")]
    WsOutOfMemory(NonZeroUsize),
    /// Create a new `VclError` from a UTF-8 error
    #[error("{0}")]
    Utf8Error(#[from] Utf8Error),
    /// Create a new `VclError` from a boxed error
    #[error("{0}")]
    Box(#[from] Box<dyn std::error::Error>),
}

impl VclError {
    /// Create a new `Error` from a string
    pub fn new(s: String) -> Self {
        Self::String(s)
    }

    pub fn as_str(&self) -> Cow<'_, str> {
        match self {
            Self::String(s) => Cow::Borrowed(s.as_str()),
            Self::Utf8Error(e) => Cow::Owned(e.to_string()),
            Self::Str(s) => Cow::Borrowed(s),
            Self::Box(e) => Cow::Owned(e.to_string()),
            Self::CStr(s) => Cow::Owned(cstr_to_string(s)),
            Self::WsOutOfMemory(sz) => {
                Cow::Owned(format!("Unable to allocate {sz} bytes in a Workspace"))
            }
        }
    }
}

fn cstr_to_string(value: &CStr) -> String {
    match value.to_string_lossy() {
        Cow::Borrowed(s) => s.to_string(),
        Cow::Owned(s) => {
            format!("{s} (error is not exact because it contains non-utf8 characters)")
        }
    }
}

// Any error types are done with the #[from] attribute, so we don't need to implement From for them

impl From<String> for VclError {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&'static str> for VclError {
    fn from(s: &'static str) -> Self {
        Self::Str(s)
    }
}

impl From<&'static CStr> for VclError {
    fn from(s: &'static CStr) -> Self {
        Self::CStr(s)
    }
}

/// Shorthand to [`Result<T, VclError>`]
pub type VclResult<T> = Result<T, VclError>;
