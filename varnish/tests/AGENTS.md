# varnish/tests — macro-output fixtures + integration subcrate

See also: [varnish/](../AGENTS.md) · [varnish-macros](../../varnish-macros/AGENTS.md) · [examples](../../examples/AGENTS.md)

Two unrelated things live here: macro-output test fixtures, and a real integration-test subcrate.

## Macro-output fixtures (trybuild + insta)

- `compile.rs` — trybuild harness. `fail/*.rs` must fail to compile, paired with expected `fail/*.stderr`. `pass/*.rs` must compile. `pass_ffi/*.rs` must compile, gated on `ffi` feature.
- Same `pass/*.rs` and `pass_ffi/*.rs` fixtures are **also** read by `varnish-macros/src/tests.rs` (insta) — parser/generator run directly, 4 outputs snapshotted per fixture (`@model`, `@docs`, `@code`, `@json`) into `varnish/snapshots<version>/` — one dir per exact installed version (`snapshots8.0.0` .. `snapshots8.0.2`, `snapshots9.0.0` .. `snapshots9.0.3`, `snapshotstrunk`), path picked via full `env!("VARNISHAPI_VERSION_NUMBER")` (major.minor.patch, or `trunk`) — not just major version.
- **Update both**: `just bless` (regens insta snapshots + trybuild `.stderr` for the currently-installed Varnish version). `just bless-all` does all versions via Docker.
- New pass-fixture in `tests/pass/*.rs` → auto-exercised by both harnesses. New fail-fixture → add `tests/fail/X.rs`, generate `X.stderr` via `just bless`.
- Unreferenced snapshots across version dirs are OK (`--unreferenced=ignore`) — some fixtures may exist only for some versions.

## `vmod_test/` subcrate

Real workspace member, crate name `vmod_test`, VCL module name `rustest` (`import rustest from "${vmod}";` — that's the name to look for at the VCL-import/build-artifact boundary, not the crate name), `varnish = { features = ["ffi"] }`. Exercises nearly every framework feature: workspace reservation, hashing controls, probes, IP building, blobs, backends. Embeds `varnish::run_vtc_tests!("tests/*.vtc")` with 16 `.vtc` files.

**This is the closest thing to a full `varnishd` integration test** — when adding a new framework feature, add coverage here (or in a matching `examples/vmod_*`), not just unit tests.
