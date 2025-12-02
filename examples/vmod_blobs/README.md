# VMOD Blobs

This example demonstrates how to use VCL_BLOB arguments in Varnish modules written in Rust.

## Overview

Starting with this version of varnish-rs, you can accept BLOB data as input arguments to your VMOD functions. In Rust, BLOBs are represented as `&[u8]` slices, providing safe and efficient access to binary data.

## Features Demonstrated

This example VMOD provides several functions that work with BLOB data:

- `blob_length(data: &[u8]) -> i64` - Returns the length of a BLOB
- `blob_length_opt(data: Option<&[u8]>) -> i64` - Handles optional BLOBs
- `is_empty(data: &[u8]) -> bool` - Checks if BLOB is empty
- `checksum(data: &[u8]) -> i64` - Computes a simple checksum

## Usage in VCL

```vcl
vcl 4.1;

import blobs;

backend default {
    .host = "127.0.0.1";
    .port = "8080";
}

sub vcl_init {
    new b = blob.blob(IDENTITY, "Hello, World!");
}

sub vcl_recv {
    set req.http.blob-len = blobs.blob_length(b.get());
    set req.http.checksum = blobs.checksum(b.get());
}
```

## Building

```bash
cargo build --release
```

The compiled VMOD will be available at `target/release/libvmod_blobs.so`.

## See Also

- [Varnish BLOB documentation](https://varnish-cache.org/docs/trunk/reference/vmod.html#blob)
- [varnish-rs documentation](https://docs.rs/varnish)
