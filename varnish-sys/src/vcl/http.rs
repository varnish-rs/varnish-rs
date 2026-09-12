//! Headers and top line of an HTTP object
//!
//! Depending on the VCL subroutine, the `Ctx` will give access to various [`HttpHeaders`] object which
//! expose the request line (`req`, `req_top` and `bereq`), response line (`resp`, `beresp`) and
//! headers of the objects Varnish is manipulating.
//!
//! `HTTP` implements `IntoIterator` that will expose the headers only (not the `method`, `status`,
//! etc.)
//!
//! **Note:** at this stage, headers are assumed to be utf8, and you will get a panic if it's not
//! the case. Future work needs to sanitize the headers to make this safer to use. It is tracked in
//! this [issue](https://github.com/varnish-rs/varnish-rs/issues/4).

use std::fmt;
use std::io::Write;
use std::mem::transmute;
use std::slice::from_raw_parts_mut;

use crate::ffi;
use crate::ffi::{txt, VslTag};
use crate::vcl::str_or_bytes::StrOrBytes;
use crate::vcl::{VclError, VclResult, Workspace, WsStrBuffer};

// C constants pop up as u32, but header indexing uses u16, redefine
// some stuff to avoid casting all the time
const HDR_FIRST: u16 = ffi::HTTP_HDR_FIRST as u16;
const HDR_METHOD: u16 = ffi::HTTP_HDR_METHOD as u16;
const HDR_PROTO: u16 = ffi::HTTP_HDR_PROTO as u16;
const HDR_REASON: u16 = ffi::HTTP_HDR_REASON as u16;
const HDR_STATUS: u16 = ffi::HTTP_HDR_STATUS as u16;
const HDR_UNSET: u16 = ffi::HTTP_HDR_UNSET as u16;
const HDR_URL: u16 = ffi::HTTP_HDR_URL as u16;

/// HTTP headers of an object, wrapping `HTTP` from Varnish
#[derive(Debug)]
pub struct HttpHeaders<'a> {
    pub raw: &'a mut ffi::http,
}

impl HttpHeaders<'_> {
    /// Wrap a raw pointer into an object we can use.
    pub(crate) fn from_ptr(p: ffi::VCL_HTTP) -> Option<Self> {
        Some(HttpHeaders {
            raw: unsafe { p.0.as_mut()? },
        })
    }

    /// Returns the workspace this HTTP object allocates from.
    fn ws(&self) -> Workspace<'_> {
        Workspace::from_ptr(self.raw.ws)
    }

    /// Points the header slot at `idx` to an already allocated header line.
    fn store_header(&mut self, idx: u16, hdr: txt) {
        assert!(idx < self.raw.nhd);
        unsafe {
            let hd = self
                .raw
                .hd
                .offset(idx as isize)
                .as_mut()
                .expect("HTTP header descriptor pointer must not be null");
            *hd = hdr;
            let hdf = self
                .raw
                .hdf
                .offset(idx as isize)
                .as_mut()
                .expect("HTTP header flags pointer must not be null");
            *hdf = 0;
        }
    }

    /// Appends an already allocated header line and logs it.
    ///
    /// # Errors
    ///
    /// Fails if all header slots are taken.
    fn push_header(&mut self, hdr: txt) -> VclResult<()> {
        assert!(self.raw.nhd <= self.raw.shd);
        if self.raw.nhd == self.raw.shd {
            return Err(c"no more header slot".into());
        }
        let idx = self.raw.nhd;
        self.raw.nhd += 1;
        self.store_header(idx, hdr);
        unsafe {
            ffi::VSLbt(
                self.raw.vsl,
                transmute::<u32, VslTag>((self.raw.logtag as u32) + u32::from(HDR_FIRST)),
                *self.raw.hd.add(idx as usize),
            );
        }
        Ok(())
    }

    fn change_header<'a>(&mut self, idx: u16, value: impl Into<StrOrBytes<'a>>) -> VclResult<()> {
        let hdr = self.ws().copy_bytes_with_null(value.into())?;
        self.store_header(idx, hdr);
        Ok(())
    }

    /// Appends a `name: value` header, writing it into the Varnish workspace.
    ///
    /// NUL bytes in `name` or `value` are not rejected, and truncate the header as seen by the
    /// C layers, matching `VRT_SetHdr`.
    ///
    /// # Errors
    ///
    /// Fails if all header slots are taken, or if the workspace is out of memory.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// http.set_header("X-Foo", "bar")?;
    /// assert_eq!(http.header("X-Foo").unwrap().as_ref(), b"bar");
    /// ```
    pub fn set_header(&mut self, name: &str, value: &str) -> VclResult<()> {
        let hdr = alloc_header_line(&mut self.ws(), name, &[value.as_bytes()])?;
        self.push_header(hdr)
    }

    /// Appends a header whose value is formatted into the Varnish workspace.
    ///
    /// NUL bytes in `name` or the formatted value are not rejected, and truncate the header as
    /// seen by the C layers, matching `VRT_SetHdr`.
    ///
    /// # Errors
    ///
    /// Fails if all header slots are taken, or if the workspace is out of memory.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// http.set_header_fmt("X-Info", format_args!("id={id} backend={backend}"))?;
    /// ```
    pub fn set_header_fmt(&mut self, name: &str, args: fmt::Arguments<'_>) -> VclResult<()> {
        let hdr = alloc_header_line_fmt(&mut self.ws(), name, args)?;
        self.push_header(hdr)
    }

    /// Remove all headers matching `name` (case-insensitive). No-op if the header is absent.
    pub fn unset_header(&mut self, name: &str) {
        let hdrs = unsafe {
            &from_raw_parts_mut(self.raw.hd, self.raw.nhd as usize)[(HDR_FIRST as usize)..]
        };

        let mut idx_empty = 0;
        for (idx, hd) in hdrs.iter().enumerate() {
            let (n, _) = hd.parse_header().expect("HTTP header must be parseable");
            if name.eq_ignore_ascii_case(n) {
                unsafe {
                    ffi::VSLbt(
                        self.raw.vsl,
                        transmute::<u32, VslTag>(
                            (self.raw.logtag as u32) + u32::from(HDR_UNSET) + u32::from(HDR_METHOD),
                        ),
                        *self.raw.hd.add(HDR_FIRST as usize + idx),
                    );
                }
                continue;
            }
            if idx != idx_empty {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        self.raw.hd.add(HDR_FIRST as usize + idx),
                        self.raw.hd.add(HDR_FIRST as usize + idx_empty),
                        1,
                    );
                    std::ptr::copy_nonoverlapping(
                        self.raw.hdf.add(HDR_FIRST as usize + idx),
                        self.raw.hdf.add(HDR_FIRST as usize + idx_empty),
                        1,
                    );
                }
            }
            idx_empty += 1;
        }
        self.raw.nhd = HDR_FIRST + idx_empty as u16;
    }

    /// Return header at a specific position
    fn field(&self, idx: u16) -> Option<StrOrBytes<'_>> {
        unsafe {
            if idx >= self.raw.nhd {
                None
            } else {
                self.raw
                    .hd
                    .offset(idx as isize)
                    .as_ref()
                    .expect("HTTP header pointer must not be null")
                    .to_slice()
                    .map(StrOrBytes::from)
            }
        }
    }

    /// Method of an HTTP request, `None` for a response
    pub fn method(&self) -> Option<StrOrBytes<'_>> {
        self.field(HDR_METHOD)
    }

    /// URL of an HTTP request, `None` for a response
    pub fn url(&self) -> Option<StrOrBytes<'_>> {
        self.field(HDR_URL)
    }

    /// Set the URL of this HTTP request.
    ///
    /// This updates the URL (path and query) component of the HTTP request line associated
    /// with this [`HttpHeaders`] object. It is only meaningful for request objects; for responses
    /// the corresponding [`url`](Self::url) accessor will return `None`.
    ///
    /// The new value must fit in the underlying Varnish workspace; otherwise an error is
    /// returned.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Change the URL of the current request before it is processed further.
    /// http.set_url("/new/path?foo=bar")?;
    /// assert_eq!(http.url().unwrap().as_str(), "/new/path?foo=bar");
    /// ```
    pub fn set_url(&mut self, value: &str) -> VclResult<()> {
        self.change_header(HDR_URL, value)
    }

    /// Protocol of an object
    ///
    /// It should exist for both requests and responses, but the `Option` is maintained for
    /// consistency.
    pub fn proto(&self) -> Option<StrOrBytes<'_>> {
        self.field(HDR_PROTO)
    }

    /// Set prototype
    pub fn set_proto(&mut self, value: &str) -> VclResult<()> {
        self.raw.protover = match value {
            "HTTP/0.9" => 9,
            "HTTP/1.0" => 10,
            "HTTP/1.1" => 11,
            "HTTP/2.0" => 20,
            _ => 0,
        };
        self.change_header(HDR_PROTO, value)
    }

    /// Response status, `None` for a request
    pub fn status(&self) -> Option<StrOrBytes<'_>> {
        self.field(HDR_STATUS)
    }

    /// Set the response status, it will also set the reason
    pub fn set_status(&mut self, status: u16) {
        unsafe {
            ffi::http_SetStatus(self.raw, status, std::ptr::null());
        }
    }

    /// Response reason, `None` for a request
    pub fn reason(&self) -> Option<StrOrBytes<'_>> {
        self.field(HDR_REASON)
    }

    /// Set reason
    pub fn set_reason(&mut self, value: &str) -> VclResult<()> {
        self.change_header(HDR_REASON, value)
    }

    /// Weaken the `ETag` header if present and not already weak.
    ///
    /// Implements [RFC 2616 §3.11](https://www.rfc-editor.org/rfc/rfc2616#section-3.11) `ETag`
    /// weakening: if the `ETag` header exists and does not already start with `W/`, it is
    /// replaced with `W/<original-value>`.
    pub fn weaken_etag(&mut self) -> VclResult<()> {
        let Some(etag) = self.header("ETag") else {
            return Ok(());
        };
        let value = etag.as_ref();
        if value.starts_with(b"W/") {
            return Ok(());
        }
        // Allocate first: `hdr` holds only raw pointers, so `value`'s borrow of `self` is
        // released before the mutations below.
        let hdr = alloc_header_line(&mut self.ws(), "ETag", &[b"W/", value])?;
        self.unset_header("ETag");
        self.push_header(hdr)
    }

    /// Returns the value of a header based on its name
    ///
    /// The header names are compared in a case-insensitive manner
    pub fn header(&self, name: &str) -> Option<StrOrBytes<'_>> {
        self.iter()
            .find(|hdr| name.eq_ignore_ascii_case(hdr.0))
            .map(|hdr| hdr.1)
    }

    /// Iterate over `(name, value)` pairs for all headers, excluding the request/status line.
    pub fn iter(&self) -> HttpHeadersIter<'_> {
        HttpHeadersIter {
            http: self,
            cursor: HDR_FIRST as isize,
        }
    }
}

/// Writes `name: <parts...>` into the workspace, returning the header line.
///
/// Copies the fragments verbatim, bypassing `core::fmt`.
fn alloc_header_line(ws: &mut Workspace<'_>, name: &str, parts: &[&[u8]]) -> VclResult<txt> {
    let mut buf = ws.vcl_string_builder()?;
    buf.extend_from_slice(name.as_bytes())?;
    buf.extend_from_slice(b": ")?;
    for part in parts {
        buf.extend_from_slice(part)?;
    }
    Ok(finish_header_line(buf))
}

/// Same as [`alloc_header_line`], but formats the value with `core::fmt`.
fn alloc_header_line_fmt(
    ws: &mut Workspace<'_>,
    name: &str,
    args: fmt::Arguments<'_>,
) -> VclResult<txt> {
    let mut buf = ws.vcl_string_builder()?;
    buf.extend_from_slice(name.as_bytes())?;
    buf.extend_from_slice(b": ")?;
    buf.write_fmt(args)
        .map_err(|_| VclError::CStr(c"no space in the workspace for the header value"))?;
    Ok(finish_header_line(buf))
}

/// Turns the bytes written to `buf` into a header line.
///
/// [`WsStrBuffer::finish`] NUL-terminates them and releases the unused workspace.
fn finish_header_line(buf: WsStrBuffer<'_>) -> txt {
    let len = buf.len();
    let b = buf.finish().0;
    txt {
        b,
        e: unsafe { b.add(len) },
    }
}

/// Appends an HTTP header, writing it into the Varnish workspace.
///
/// # Examples
///
/// ```ignore
/// set_header!(req, "X-Foo", value)?;                          // verbatim value
/// set_header!(req, "X-Count", "count={count}")?;              // interpolated literal
/// set_header!(req, "X-Info", "id={} backend={}", id, name)?;  // template plus arguments
/// ```
#[macro_export]
macro_rules! set_header {
    // Must come first: `$value:expr` below would match a literal too, silently emitting
    // `{count}` instead of interpolating it.
    ($http:expr, $name:expr, $fmt:literal) => {
        $http.set_header_fmt($name, ::std::format_args!($fmt))
    };
    ($http:expr, $name:expr, $value:expr) => {
        $http.set_header($name, $value)
    };
    ($http:expr, $name:expr, $fmt:expr, $($arg:tt)*) => {
        $http.set_header_fmt($name, ::std::format_args!($fmt, $($arg)*))
    };
}

impl<'a> IntoIterator for &'a HttpHeaders<'a> {
    type Item = (&'a str, StrOrBytes<'a>);
    type IntoIter = HttpHeadersIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over HTTP header `(name, value)` pairs, returned by [`HttpHeaders::iter`].
#[derive(Debug)]
pub struct HttpHeadersIter<'a> {
    http: &'a HttpHeaders<'a>,
    cursor: isize,
}

impl<'a> Iterator for HttpHeadersIter<'a> {
    type Item = (&'a str, StrOrBytes<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let nhd = self.http.raw.nhd;
            if self.cursor >= nhd as isize {
                return None;
            }
            let hd = unsafe {
                self.http
                    .raw
                    .hd
                    .offset(self.cursor)
                    .as_ref()
                    .expect("HTTP header pointer must not be null")
            };
            self.cursor += 1;
            if let Some(hdr) = hd.parse_header() {
                return Some(hdr);
            }
        }
    }
}
