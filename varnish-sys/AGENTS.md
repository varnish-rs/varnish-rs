# varnish-sys — FFI + bindgen foundation

See also: [workspace root](../AGENTS.md) · [vcl/ safe wrappers](src/vcl/AGENTS.md)

Crate name `varnish_sys`, `links = "varnishapi"`. Bottom layer: raw bindgen bindings (`pub mod ffi`) + thin unsafe-but-checked wrappers (`pub mod vcl`) around Varnish C structs (`vrt_ctx`, `ws`, `director`, `http`, vfp/vdp, `vmod_priv`, ACLs, probes, subs). Foundation for `varnish-macros` and `varnish`.

## build.rs pipeline

1. `detect_varnish()`: if `VARNISH_INCLUDE_PATHS` env set (colon-separated), use directly, **assume latest supported version** — prints an explicit `cargo::warning` saying so (can't auto-detect version this way — may be wrong if you're actually on an older lib). Else `pkg_config::Config::new().probe("varnishapi")` — gets real version from `.pc` file.
2. Docs.rs path: if `DOCS_RS` env set and no varnishapi found, copies checked-in `bindings.for-docs` straight to `OUT_DIR/bindings.rs`, skips real bindgen entirely. This is how docs.rs builds without libvarnish installed.
3. Emits version cfgs, e.g. `cfg(varnishsys_90_sslflags)` for >=9.0.0 or `trunk`. Warns if <8.0.0 (unsupported). New version-gated cfgs follow the "name after the first (minimum) Varnish version where the feature exists" convention.
4. `generate_bindings()`: bindgen on `c_code/wrapper.h`, `bindgen_helpers::Renamer` renames enums (`VSL_tag_e`→`VslTag`, `boc_state_e`→`BocState`, `director_state_e`→`DirectorState`, `gethdr_e`→`GetHeader`, `sess_attr`→`SessionAttr`, `lbody_e`→`Body`, `task_prio`→`TaskPriority`, `vas_e`→`Vas`, `vcl_event_e`→`VclEvent`, `vcl_func_call_e`→`VclFuncCall`, `vcl_func_fail_e`→`VclFuncFail`, `vdp_action`→`VdpAction`, `vfp_status`→`VfpStatus`), treated as non-exhaustive. New-types all `VCL_*`/`vtim_*` typedefs (except `VCL_VOID`/`VCL_INSTANCE` → alias `c_void`). Blocklists `FP_.*`/`FILE`.
5. Writes `OUT_DIR/bindings.rs`, diffs vs checked-in `bindings.for-docs`. Two separate warnings, not one: if generated bindings *content* differs, `cargo::warning` tells dev to `cp $OUT_DIR/bindings.rs varnish-sys/bindings.for-docs`; if content matches but `BINDINGS_FILE_VER` doesn't match the detected version, a different `cargo::warning` tells dev to bump `BINDINGS_FILE_VER` in build.rs instead.
6. Links `varnishapi` (`cargo::rustc-link-lib=varnishapi`).

## Hard rules

- **Never hand-edit `OUT_DIR/bindings.rs` or `bindings.for-docs`.** Both generated. Regen: real build against installed libvarnish → `cp` → bump `BINDINGS_FILE_VER` in build.rs if version changed. Build script warns you when stale.
- `c_code/wrapper.h` is the *only* vendored C file — just `#include`s real system headers (`cache_varnishd.h`, `cache_backend.h`, `cache_director.h`, `vrt_obj.h`, `cache_filter.h`, `vmod_abi.h`, `vsb.h`, `vsa.h`, `vapi/vsm.h`, `vapi/vsc.h`, `vcl.h`) + one manual `VFP_Push` forward decl. No vendored `.c` sources.
- Requires `clang`/`llvm` toolchain locally for bindgen (see `docker/Dockerfile`).
- Crate must compile clean against oldest supported (8.0, no `varnishsys_*` cfgs) and newest (all applicable cfgs).

## src/ map

- `src/lib.rs` — `pub mod ffi` (raw `include!` of generated bindings), then extensions/txt/utils/validate, `pub mod vcl`.
- `src/extensions.rs` — `vmod_priv` helpers: `take`, `take_per_vcl`, `get_ref`, `put`, `on_fini`, `on_fini_per_vcl`. Safe box/unbox of Rust objects behind C `void*`.
- `src/txt.rs` — C `txt` struct (byte-range string) ⇄ `&str`/`CStr`/`StrOrBytes` conversions, header parsing.
- `src/utils.rs` — `get_backend<S,T>()`, pulls typed backend impl out of a `director`.
- `src/validate.rs` — magic-number validation (`validate_vrt_ctx`, `validate_director`, `validate_ws`, `validate_vfp_ctx`, `validate_vfp_entry`, `validate_vdir`). **Standard pattern**: any new wrapper around a raw C pointer must validate-before-use like this.
- `src/vcl/` — the safe-wrapper subsystem re-exported as `varnish::vcl`. See [src/vcl/AGENTS.md](src/vcl/AGENTS.md).

## Testing

No `tests/` dir here. Unit tests inline (`#[cfg(test)] mod tests`) in `src/vcl/ctx.rs`, `src/vcl/ws.rs`, `src/vcl/ws_str_buffer.rs`, `src/vcl/backend/backend_main.rs`. Real e2e/VCL-behavior tests live in `varnish/tests/vmod_test` and `examples/`, not here.

## Known limitation

`src/vcl/http.rs` assumes UTF-8 headers, **panics** otherwise — tracked as issue #4. Real limitation, not a silent-fix target.

## Consumers

- `varnish-macros` reads `varnish_sys::ffi::VMOD_ABI_Version` (generator.rs) and `varnish_sys::vcl::subroutine::{VALID_RESTRICT_SCOPES, bitmask_const_name}` (parser.rs/gen_func.rs). This coupling is *why* `varnish-sys` must be a separate non-proc-macro crate.
- `varnish` re-exports `varnish_sys::vcl` and (feature-gated) `varnish_sys::ffi`; `metrics_publisher.rs`/`metrics_reader.rs` use `ffi::{vsc_seg, VRT_VSC_Alloc, VRT_VSC_Destroy}`.
