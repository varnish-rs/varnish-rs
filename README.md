# varnish-rs

[![GitHub](https://img.shields.io/badge/github-varnish-8da0cb?logo=github)](https://github.com/gquintard/varnish-rs)
[![crates.io version](https://img.shields.io/crates/v/varnish.svg)](https://crates.io/crates/varnish)
[![docs.rs docs](https://docs.rs/varnish/badge.svg)](https://docs.rs/varnish)
[![crates.io version](https://img.shields.io/crates/l/varnish.svg)](https://github.com/gquintard/varnish-rs/blob/main/LICENSE)
[![CI build](https://github.com/gquintard/varnish-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/gquintard/varnish-rs/actions)

![Varnish Logo](./logo.svg)

The `varnish` crate provides a safe and idiomatic interface to the [Varnish](https://varnish-cache.org/intro/index.html) C API, allowing you to write Varnish modules (VMODs) in Rust. See the [crate API](https://docs.rs/varnish) for more details.

Some VMODs that use this library:

- [vmod-reqwest](https://github.com/gquintard/vmod_reqwest): issue HTTP calls from VCL, use dynamic, HTTPS backends (support HTTP2)
- [vmod-rers](https://github.com/gquintard/vmod_rers): support for dynamic regex, including response body manipulation
- [vmod-fileserver](https://github.com/gquintard/vmod_fileserver): serve files directly from disk, without the need for an HTTP backend

Don't hesitate to open GitHub issues if something is unclear or impractical. You can also join us on [discord](https://discord.com/invite/EuwdvbZR6d).

## Requirements

When compiling, this library generates bindings from the `libvarnish` headers. Depending on you Linux distribution, you may need to install the related package, which could be named `varnish-devel`, `varnish-dev` or maybe `libvarnish-dev`.

Before v0.1.0, this library relied on a specific version of `libvarnish`. Since v0.1.0, this is no longer the case, and it should work with multiple supported Varnish versions.

| varnish-rs (Rust) | libvarnish (C) |
|:-----------------:|:--------------:|
|       0.4+        |  6.0lts, 7.5+  |
|   0.1.0 - 0.2.0   |   7.4 - 7.6    |
|  0.0.18 - 0.0.19  |      7.5       |
|      0.0.17       |      7.4       |
|  0.0.15 - 0.0.16  |      7.3       |
|  0.0.12 - 0.0.14  |      7.2       |
|  0.0.9 - 0.0.11   |      7.1       |
|   0.0.0 - 0.0.8   |      7.0       |

## Development

* This project is easier to develop with [just](https://github.com/casey/just#readme), a modern alternative to `make`.
  Install it with `cargo install just`.
* To get a list of available commands, run `just`.
* To run tests, use `just test`.

It is recommended if your `varnish` headers are installed where `pkg-config` can find them.  If not, you can set the `VARNISH_INCLUDE_PATHS` environment variable to a colon-separated list of paths to search, but note that `build.rs` script cannot detect `libvarnish` version, and assumes the latest.

```
VARNISH_INCLUDE_PATHS=/my/custom/libpath:/my/other/custom/libpath cargo build
```

See `CONTRIBUTING.md` for other details.

## License

Licensed under the 3-Clause BSD License ([LICENSE](LICENSE) or <https://opensource.org/license/BSD-3-clause>)

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you shall be licensed as above, without any additional terms or conditions.
