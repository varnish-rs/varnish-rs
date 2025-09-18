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

use std::mem::transmute;
use std::slice::from_raw_parts_mut;

use crate::ffi;
use crate::ffi::VslTag;
use crate::vcl::str_or_bytes::StrOrBytes;
use crate::vcl::{VclResult, Workspace};

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

    fn change_header<'a>(&mut self, idx: u16, value: impl Into<StrOrBytes<'a>>) -> VclResult<()> {
        assert!(idx < self.raw.nhd);

        /* XXX: aliasing warning, it's the same pointer as the one in Ctx */
        let mut ws = Workspace::from_ptr(self.raw.ws);
        unsafe {
            let hd = self.raw.hd.offset(idx as isize).as_mut().unwrap();
            *hd = ws.copy_bytes_with_null(value.into())?;
            let hdf = self.raw.hdf.offset(idx as isize).as_mut().unwrap();
            *hdf = 0;
        }
        Ok(())
    }

    /// Append a new header using `name` and `value`. This can fail if we run out of internal slots
    /// to store the new header
    pub fn set_header(&mut self, name: &str, value: &str) -> VclResult<()> {
        assert!(self.raw.nhd <= self.raw.shd);
        if self.raw.nhd == self.raw.shd {
            return Err(c"no more header slot".into());
        }

        let idx = self.raw.nhd;
        self.raw.nhd += 1;
        // FIXME: optimize this to avoid allocating a temporary string
        let res = self.change_header(idx, &format!("{name}: {value}"));
        if res.is_ok() {
            unsafe {
                ffi::VSLbt(
                    self.raw.vsl,
                    transmute::<u32, VslTag>((self.raw.logtag as u32) + u32::from(HDR_FIRST)),
                    *self.raw.hd.add(idx as usize),
                );
            }
        } else {
            self.raw.nhd -= 1;
        }
        res
    }

    pub fn unset_header(&mut self, name: &str) {
        let hdrs = unsafe {
            &from_raw_parts_mut(self.raw.hd, self.raw.nhd as usize)[(HDR_FIRST as usize)..]
        };

        let mut idx_empty = 0;
        for (idx, hd) in hdrs.iter().enumerate() {
            let (n, _) = hd.parse_header().unwrap();
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
                    .unwrap()
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
            ffi::http_SetStatus(
                self.raw,
                status,
                #[cfg(not(varnishsys_6))]
                std::ptr::null(),
            );
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

    /// Returns the value of a header based on its name
    ///
    /// The header names are compared in a case-insensitive manner
    pub fn header(&self, name: &str) -> Option<StrOrBytes<'_>> {
        self.iter()
            .find(|hdr| name.eq_ignore_ascii_case(hdr.0))
            .map(|hdr| hdr.1)
    }

    pub fn iter(&self) -> HttpHeadersIter<'_> {
        HttpHeadersIter {
            http: self,
            cursor: HDR_FIRST as isize,
        }
    }
}

impl<'a> IntoIterator for &'a HttpHeaders<'a> {
    type Item = (&'a str, StrOrBytes<'a>);
    type IntoIter = HttpHeadersIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

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
            let hd = unsafe { self.http.raw.hd.offset(self.cursor).as_ref().unwrap() };
            self.cursor += 1;
            if let Some(hdr) = hd.parse_header() {
                return Some(hdr);
            }
        }
    }
}
