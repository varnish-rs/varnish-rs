//! Example VMOD demonstrating VCL_BLOB support
//!
//! This example shows how to use BLOB arguments in vmod functions.
//! BLOBs are passed as `&[u8]` slices in Rust.

use varnish::vmod;

varnish::run_vtc_tests!("tests/*.vtc");

/// Example VMOD demonstrating BLOB input support
#[vmod]
mod blobs {
    use sha2::{Digest, Sha256};

    /// Returns the length of a BLOB
    ///
    /// Example VCL:
    /// ```vcl
    /// set req.http.blob-len = blob_example.blob_length(blob);
    /// ```
    pub fn blob_length(data: &[u8]) -> i64 {
        data.len() as i64
    }

    /// Returns the length of an optional BLOB, or -1 if NULL
    ///
    /// Example VCL:
    /// ```vcl
    /// set req.http.blob-len = blob_example.blob_length_opt(blob);
    /// ```
    pub fn blob_length_opt(data: Option<&[u8]>) -> i64 {
        data.map_or(-1, |d| d.len() as i64)
    }

    /// Returns true if the BLOB is empty
    ///
    /// Example VCL:
    /// ```vcl
    /// if (blob_example.is_empty(blob)) {
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
    /// set req.http.checksum = blob_example.checksum(blob);
    /// ```
    pub fn checksum(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let hash = hasher.finalize();
        format!("{:x}", hash)
    }

    //    pub fn access_pointer(b: VCL_BLOB) {
    //    }
}
