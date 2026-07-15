# varnish — public facade crate

See also: [workspace root](../AGENTS.md) · [vcl/ domain logic](../varnish-sys/src/vcl/AGENTS.md) · [tests/](tests/AGENTS.md) · [examples](../examples/AGENTS.md)

What VMOD authors actually depend on. Almost no logic of its own — thin facade re-exporting `varnish-sys` (FFI/safe wrappers) and `varnish-macros` (`#[vmod]` codegen).

## Own code (src/)

- `lib.rs` — crate root. **Doubles as the VMOD-authoring tutorial** (project layout, Cargo.toml shape, arg/return type tables, objects, `#[shared_per_task]`/`#[shared_per_vcl]`, event fns, metrics) written as doc comments. Treat as living docs — keep in sync when framework behavior changes. Re-exports: `pub use varnish_sys::vcl;`, `pub use varnish_sys::ffi;` (behind `ffi` feature), `pub use varnish_macros::{run_vtc_tests, vmod, VscMetric};`.
- `metrics_reader.rs` — `MetricsReader`/`Metric`/`MetricFormat`/`Semantics`. Read VSC stats from an external (non-vmod) process. Used by `examples/stat_tree`.
- `metrics_publisher.rs` — `Vsc`/`VscMetric`. Expose custom counters to `varnishstat` from within a VMOD. Used by `examples/vmod_counters`.
- `varnishtest.rs` — runtime support for `run_vtc_tests!`. Shells out to `varnishtest` binary, caps concurrency (each test forks a `varnishd` + does CLI handshake), treats exit code 77 as skip.

Actual domain logic (Ctx, HTTP, workspace, backends, VDP/VFP, ACL, subroutine, convert) lives in `varnish-sys/src/vcl/`, re-exported here as `varnish::vcl` — see [that guide](../varnish-sys/src/vcl/AGENTS.md).

## Gotchas

- **`ffi` feature** gates the raw `varnish_sys::ffi` re-export + `pass_ffi`/ffi-only tests. Only enable for low-level/backend code needing direct C struct access.
- Objects: struct + `impl` block go **outside** the `#[vmod]` module body — only fns/methods go inside. Constructor = any method returning `Self`.
- `#[shared_per_task]` / `#[shared_per_vcl]` type must stay consistent across the whole vmod.
- Curated `ffi` re-export list (feature off) must stay in sync with `use_ffi_items` in `varnish-macros/src/generator.rs` — don't edit one without the other.
