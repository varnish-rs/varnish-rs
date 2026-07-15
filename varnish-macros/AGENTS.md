# varnish-macros — proc-macro crate

See also: [workspace root](../AGENTS.md) · [varnish-sys](../varnish-sys/AGENTS.md) · [test fixtures](../varnish/tests/AGENTS.md)

`[lib] proc-macro = true`. Sole public surface VMOD authors touch (via `varnish::{vmod, run_vtc_tests, VscMetric}`). Three entry points, all in `src/lib.rs`:

- `#[vmod]` — main macro. Turns a `mod` into a full VMOD: functions, objects/impls, events, restrict scopes, shared state.
- `#[derive(VscMetric)]` (attrs `counter`, `gauge`, `bitmap`) — derives VSC metric structs.
- `run_vtc_tests!("glob")` — generates one `#[test]` per matching `.vtc` file.

`build.rs` reads `DEP_VARNISHAPI_VERSION_NUMBER` (from `varnish-sys`'s `links = "varnishapi"`), sets `cfg(varnishsys_77_vmod_data)` + `VARNISHAPI_VERSION_NUMBER` env — couples codegen to installed Varnish version.

## Pipeline model

```
TokenStream (attr args, ItemMod)
  → parser::tokens_to_model        (darling parses #[vmod(docs=...)] etc)
  → model::VmodInfo                (validated, source of truth)
  → generator::render_model        (Item::Verbatim inserted into original module)
  → quote! → TokenStream
```

## src/ map

- `lib.rs` — 3 macro entry points + full `#[vmod]` attribute reference in doc comments (`#[event]`, `#[restrict(...)]`, `#[shared_per_task]`, `#[shared_per_vcl]`, `#[vcl_name]`, `docs = "..."`).
- `parser.rs` — `tokens_to_model`: walks `ItemMod`, rejects disallowed items (structs/enums/consts/macros/nested mods/statics/traits/type aliases outside allowed shape), builds `VmodInfo`.
- `parser_args.rs` — function-signature parsing (`FuncStatus`, arg kinds, dedup checks, shared-state detection).
- `parser_utils.rs` — low-level `syn` helpers (`remove_attr`, `has_attr`, `find_attr`, type-shape checks).
- `model.rs` — validated data model: `VmodInfo`, `ObjInfo`, `FuncInfo`, `VmodParams{docs}`, `ParamType`. Treat as source of truth.
- `generator.rs` — `render_model`, top-level codegen driver.
- `gen_func.rs` (671 lines, largest) — per-function/method wrapper + C proto/JSON generation.
- `gen_objects.rs` — per-object (impl block) codegen, delegates to `gen_func.rs`.
- `gen_docs.rs` — writes markdown doc file when `#[vmod(docs = "…")]` set (build-time side effect).
- `metrics.rs` — `derive_vsc_metric` impl.
- `names.rs` — centralized identifier generation (`Names` struct).
- `errors.rs` — `Errors` accumulator, combines `syn::Error`s.
- `vtc_tests.rs` — `run_vtc_tests!` generator: glob → `#[test]` fns, collision-suffixes names (`vtc_foo_bar`, `vtc_foo_bar_1`).
- `tests.rs` (`#[cfg(test)]`) — the actual test harness. **Lives inside crate** because proc-macro crates can't export non-macro fns for external test binaries to call.
- `snapshots/*.snap` — insta snapshots, but only for `vtc_tests!` expansion tests (collision/error/no_matches/invalid_glob/populated) and `sanitize_ident` unit test.

## Key deps

`darling` parses `#[vmod(...)]` into `VmodParams` (currently only supports `docs` — new args go through darling in `parser.rs`). `syn`/`proc-macro2`/`quote` for AST+codegen. `prettyplease` used **only in tests** (`tests.rs`) to pretty-print generated code before snapshotting. `serde_json`/`sha2` for VCC JSON blob + CString hashing at codegen time. `glob` for `run_vtc_tests!` discovery. `varnish-sys` supplies `VALID_RESTRICT_SCOPES`.

## Testing — two systems, see [varnish/tests/AGENTS.md](../varnish/tests/AGENTS.md) for the fixture side

1. `src/tests.rs` (insta): reads real fixtures from `varnish/tests/pass/*.rs` and `varnish/tests/pass_ffi/*.rs` (cross-crate!), runs parser/generator directly, snapshots 4 outputs per fixture (`@model`, `@docs`, `@code`, `@json`) into `varnish/snapshots<VARNISHAPI_VERSION>/` — one dir per supported Varnish version.
2. `varnish/tests/compile.rs` (trybuild, lives in `varnish` crate): `fail/*.rs` + `.stderr` compile-fail, `pass/*.rs`/`pass_ffi/*.rs` compile-pass (same fixtures reused above).

**Update snapshots**: `just bless` (wraps `cargo insta test --accept --unreferenced=ignore -p varnish-macros -p varnish` + `TRYBUILD=overwrite`).

## Gotchas

- Any codegen change touches insta snapshots under `varnish/snapshots*` — expect large diffs. 3 real dirs are checked in (`snapshots8.0.0`, `snapshots9.0.0`, `snapshotstrunk`); other version dirs are symlinks to one of these (see [varnish/tests/AGENTS.md](../varnish/tests/AGENTS.md)) so you don't need to bless each one separately per patch release.
- Debug generated code with `cargo expand` (see root CONTRIBUTING.md for the exact recipe).
- `run_vtc_tests!` needs `CARGO_MANIFEST_DIR` at compile time — errors outside cargo build.
- Breaking macro API changes ripple into every downstream VMOD example — run `just semver` (cargo-semver-checks).
- No subdirectory here is complex enough for its own AGENTS.md — everything flat under `src/`.
