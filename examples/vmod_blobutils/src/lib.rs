//! Example VMOD demonstrating VCL_BLOB support
//!
//! This example shows how to use BLOB arguments in vmod functions.
//! BLOBs are passed as `&[u8]` slices in Rust.

use varnish::vmod;

varnish::run_vtc_tests!("tests/*.vtc");

/// Example VMOD demonstrating BLOB input support
#[vmod]
mod blobutils {
    use sha2::{Digest, Sha256};
    use varnish::ffi::VCL_BLOB;

    /// Returns the length of a BLOB
    ///
    /// Example VCL:
    /// ```vcl
    /// set req.http.blob-len = blobutils.blob_length(blob);
    /// ```
    pub fn blob_length(data: &[u8]) -> i64 {
        data.len() as i64
    }

    /// Returns the length of an optional BLOB, or -1 if NULL
    ///
    /// Example VCL:
    /// ```vcl
    /// set req.http.blob-len = blobutils.blob_length_opt(blob);
    /// ```
    pub fn blob_length_opt(data: Option<&[u8]>) -> i64 {
        data.map_or(-1, |d| d.len() as i64)
    }

    /// Returns true if the BLOB is empty
    ///
    /// Example VCL:
    /// ```vcl
    /// if (blobutils.is_empty(blob)) {
    ///     # Handle empty blob
    /// }
    /// ```
    pub fn is_empty(data: &[u8]) -> bool {
        data.is_empty()
    }

    /// Computes the SHA256 checksum of the blob
    ///
    /// Example VCL:
    /// ```vcl
    /// set req.http.checksum = blobutils.checksum(blob);
    /// ```
    pub fn checksum(data: &[u8]) -> String {
        use std::fmt::Write as _;

        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = hasher.finalize();
        let mut out = String::with_capacity(hash.len() * 2);
        for byte in hash {
            write!(&mut out, "{byte:02x}").expect("writing to String should not fail");
        }
        out
    }

    /// Computes the SHA256 checksum of a blob passed as a raw VCL_BLOB pointer
    ///
    /// Example VCL:
    /// ```vcl
    /// set req.http.checksum = blobutils.checksum_from_pointer(blob);
    /// ```
    pub fn checksum_from_pointer(data: VCL_BLOB) -> String {
        checksum(<&[u8]>::from(data))
    }
}
