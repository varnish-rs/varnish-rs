# Unpublished
- In probe support, renamed `Request::URL` to `Request::Url`
- Refactored Workspace API:
  - Introduce `Workspace::vcl_string_builder`, `vcl_blob_builder`, and `slice_builder` instead of `reserve`
  - There is no longer any need to write NUL bytes to the end of the buffer
  - The returned buffers support `Write` trait, and can be inspected/modified what has been written so far
  - The buffer does not allow any access to "dirty" (unset) portion of the buffer
  - The buffer must be finalized with `finish()`, which returns `VCL_STRING`, `VCL_BLOB`, or `&[T]` depending on the builder used
- Add support for the non-UTF8 header values by using a `StrOrBytes` enum. This also fixes crashes when non-UTF8 headers are present.
- Remove `vsc` feature - all of its functionality is now available without any feature flags
- Rename `Stat` &rarr; `Metrics`, `Stats` &rarr; `MetricsReader`, `StatsBuilder` &rarr; `MetricsReaderBuilder`, and `Format` &rarr; `MetricsFormat`
- `MetricsReaderBuilder::patience` now returns `Self`
- For backends, rename `Serve` &rarr; `vcl::VclBackend`, `Transfer` &rarr; `vcl::VclResponse`
- Fix #167

# 0.3.0 (2024-12-12)

- Bump MSRV to 1.82.0
- Support for Varnish v6.0 LTS, but without filters, and without `vmod_be`, `vmod_vfp`, `vmod_vdp` examples
- Require all user functions that return `VCL_*` types to be `unsafe`

# 0.2.0 (2024-11-19)

- User functions can return `&CStr` as a return value or a `&'static CStr` error
- Support for safe fetch and delivery filter registration and automatic cleanup. See `vmod_vfp` and `vmod_vdp` examples.
- Rename `VSC` &rarr; `Stats`, `VSCBuilder` &rarr; `StatsBuilder`, and `VSCInternal` into `StatsImpl`
- Consolidate `Probe` and `CowProbe<'a>` into one `Probe<T>` struct with a generic `String` or `Cow<str>`.
- Rename `new` to `from_ptr` for `Buffer`, `Workspace`, `HttpHeaders`, `FetchProcCtx`, `DeliveryProcCtx`, and make them private. Only `Ctx::from_ptr` is public because it gets created by a macro-generated function.
- Make `txt::from_bytes` private - use other constructors instead
- Added `VclError::WsOutOfMemory` variant when `Workspace::alloc` fails
- Many changes to `Workspace` API:
  - All `Workspace` functions now return `VclError` rather than error strings
  - `alloc` was marked as `unsafe`
  - `alloc` now returns a `*mut c_void` instead of a `VclResult<&'a mut [u8]>`
  - `alloc` now panics if requested size is 0
  - Renamed `is_slice_allocated` to `contains`
  - Added allocation and copying functions:
    - `allocate(...) -> &mut [MaybeUninit<u8>]` (un-initialized memory)
    - `allocate_zeroed(...) -> &mut [u8]` (initialized memory)
    - `copy_blob(...) -> VCL_BLOB`
    - `copy_txt(...) -> txt`
  - Removed `copy_bytes` - use `copy_*` and `alloc_*` functions instead
  - Removed `copy_str` - use `copy_cstr` instead

# 0.1.0 (2024-11-12)

- Introduce a new, vastly improved system of generating boilerplate code using a procedural macro `#[varnish(vmod)]` by @nyurik
  - The macro will generate all the boilerplate code for a Varnish VMOD
  - The macro attribute must be used on a `mod` block that contains the VMOD functions
  - The macro can generate a markdown file, e.g. `#[varnish(docs = "README.md")]`
  - All examples have been [updated](https://github.com/gquintard/varnish-rs/commit/f0f0120d3fddbdad491ff80fccbfdd1930d24dc6) to use the new system
  - See [crate documentation](https://docs.rs/varnish/latest/varnish/) for more details
- `vtc!` macro has been replaced with `run_vtc_tests!("tests/*.vtc")`:
  - supports glob patterns
  - supports `VARNISHTEST_DURATION` env var, defaulting to "5s"
  - supports debug mode - keeps the temporary files and always prints the output: `run_vtc_tests!!("tests/*.vtc", true)`
- Multi-version support for `libvarnish` headers now allows the same code to be used with Varnish v7.4, v7.5, and v7.6
- Set MSRV to 1.77.0 - the minimum version of Rust required to use this crate
- Most FFI objects are public only if the user enables the `ffi` feature. This is to prevent users from using the FFI directly and to encourage them to use the safe Rust API.  SemVer guarantees that the public API will not change, but the FFI API may change without warning.
- Introduce `vsc` feature to enable the `varnish::vsc` module
- Renamed a few types for clarity and to be more consistent:
  - `COWProbe` struct to `CowProbe`
  - `COWRequest` struct to `CowRequest`
  - `HTTP` struct to `HttpHeaders`
  - `HTTPIter` struct to `HttpHeadersIter`
  - `VDPCtx` struct to `DeliveryProcCtx`
  - `VDP` trait to `DeliveryProcessor`
  - `VFPCtx` struct to `FetchProcCtx`
  - `VFP` trait to `FetchProcessor`
  - `Vsb` struct to `Buffer`
  - `Vsb::cat` function to `Buffer::write`
  - `WS` struct to `Workspace`
- Renamed auto-generated C enums to be more consistent and easier to use in Rust:
  - `enum VSL_tag_e` → `VslTag`, removing `SLT_` prefix on enum values, e.g. `SLT_Debug` -> `Debug`
  - `enum boc_state_e` → `BocState`, removing `BOS_` prefix on enum values, e.g. `BOS_INVALID` -> `Invalid`
  - `enum director_state_e` → `DirectorState`, removing `DIR_S_` prefix on enum values, e.g. `DIR_S_NULL` -> `Null`
  - `enum gethdr_e` → `GetHeader`, removing `HDR_` prefix on enum values, e.g. `HDR_REQ_TOP` -> `ReqTop`
  - `enum lbody_e` → `Body`, removing `LBODY_` prefix on enum values, e.g. `LBODY_SET_STRING` -> `SetString`
  - `enum sess_attr` → `SessionAttr`, removing `SA_` prefix on enum values, e.g. `SA_TRANSPORT` -> `Transport`
  - `enum task_prio` → `TaskPriority`, removing `TASK_QUEUE_` prefix on enum values, e.g. `BO` -> `TaskQueueBo`
  - `enum vas_e` → `Vas`, removing `VAS_` prefix on enum values, e.g. `VAS_WRONG` -> `Wrong`
  - `enum vcl_event_e` → `VclEvent`, removing `VCL_EVENT_` prefix on enum values, e.g. `VCL_EVENT_LOAD` -> `Load`
  - `enum vcl_func_call_e` → `VclFuncCall`, removing `VSUB_` prefix on enum values, e.g. `VSUB_STATIC` -> `Static`
  - `enum vcl_func_fail_e` → `VclFuncFail`, removing `VSUB_E_` prefix on enum values, e.g. `VSUB_E_OK` -> `Ok`
  - `enum vdp_action` → `VdpAction`, removing `VDP_` prefix on enum values, e.g. `VDP_NULL` -> `Null`
  - `enum vfp_status` → `VfpStatus`, removing `VFP_` prefix on enum values, e.g. `VFP_ERROR` -> `Error`

# 0.0.19 (2024-03-24)

- `vsc::Error` implements `std::Error`
- improve `vtc!()` debuggability
- use newer `bindgen`

# 0.0.18 (2024-03-19)

- adjust to Varnish 7.5.0

# 0.0.17 (2023-09-23)

- adjust to Varnish 7.4.0

# 0.0.16 (2023-03-19)

- fix `vsc` assert

# 0.0.15 (2023-03-19)

- Varnish 7.3 support
- Fix crash in `Backend` code due do wrong pointer cast
- Add `varnish::vsc`
- More docs
- `VFP::new()` and `VDP::new()` now take a `mut` ref to the context

# 0.0.14 (2023-02-20)

- `Backend::new()` get an extra argument: `has_probe`
- `Probe` is renamed `COWProbe`, and `Probe` now owns its `String`s

# 0.0.13 (2023-02-12)

- `cache_director.h` added to `varnish-sys`
- `generate_boilerplate!` now reads the `PYTHON` environment variable before falling
  back to `python3` to generate code
- `varnish-rs` reexports the VCL types needed to generate boilerplate code, so that
  vmods don't need to add `varnish-sys` to their `Cargo.toml` anymore.
- introduce `vcl::Backend` and co.
- `VCL_IP can be translated to `Option<std::net::SockAdd>` and back
- fix compilation issues on arm
- introduce `vcl::Error` and `vcl::Result`

# 0.0.12 (2022-11-23)

- ctx->handling is now hidden from us, so we use `VRT_fail`, at the cost of a string copy
- the JSON format in vmod_data changed slightly, adjust for that
- explicitly `drop` `Box::from_raw` results to silence `rustc`

# 0.0.11 (2022-06-16)

- fix generated `__fini` prototype

# 0.0.10 (2022-06-16)

- dumb vsb support (`vcl::vsb::Vsb`))
- probe support (`vcl::probe::Probe`)
- vmod object constructors must now return results
- `Intoresult` require the `Err()` to implement `ToString()`

# 0.0.9 (2022-05-01)

- adjust to Varnish 7.1.0

# 0.0.8 (2022-01-30)

- switch to a cargo workspace to speed up builds
- fix buffer size issue in VFPs
- do not copy workspace STRINGS into the workspace again
- fix generated code involving default STRING arguments
- C types now derive `Copy`, `Debug` and `Default`
- silence `clippy` for generated code
- expose `http_conn`
- introduce `HTTP::set_status()`, `HTTP::set_prototype()` and `HTTP::set_reason()`

# 0.0.7 (2021-12-26)

- fix a boilerplate issue when using options (extra comma)
- fix handling of option `STRING` parameters
- introduce delivery and fetch processors
- introduce `VPriv::take()`
- introduce `WS::copy_bytes_with_null()`
- `convert` accepts more types, including `Option<&std|String|&[u8]>`
- `example/vmod_vdp`
- `example/vmod_vfp`

# 0.0.6 (2021-12-23)

- `VARNISH_INCLUDE_PATHS` allows to build bindings in non-standard environments
- `vtc!` macro will also print `stderr` in case of failure
- introduce `Ctx::req_body()`

# 0.0.5 (2021-12-18)

- introduce `WS::reserve()` and `WS::release()`
- introduce `Ctx::log`
- `libvarnishapi` version in `varnish-sys` metadata
- more robust `IntoResult` implementation
- All VCL types are recognized (but not necessarily completely "rustified")
- vmod event support
- `example/vmod_event`

# 0.0.4 (2021-12-05)

- documentation starts getting a bit serious
- CI with GitHub actions
- simplify the vmod structure, `src/vmod.rs` disappears
- `example/vmod_timestamp`
- `example/vmod_infiniteloop`

# 0.0.3 (2021-12-01)

- docs can be built even without `libvarnishapi` installed
- fix vmod object support
- `example/vmod_object`

# 0.0.2 (2021-11-30)

- vmods can return a Result that will automatically call VRT_fail() if needed
- `example/vmod_error`
- `example/vmod_example`

# 0.0.1 (2021-11-28)

Initial release
