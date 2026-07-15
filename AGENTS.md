# varnish-rs — workspace guide

Rust framework for writing Varnish Cache VMODs (modules). Safe wrappers over `libvarnishapi` C ABI, plus `#[vmod]` proc-macro that generates the C glue. No `.vcc` files — macro replaces classic VCC toolchain entirely.

## Workspace map

| crate | role | guide |
|---|---|---|
| `varnish-sys` | FFI/bindgen, low-level unsafe + thin safe wrappers | [varnish-sys/AGENTS.md](varnish-sys/AGENTS.md) |
| `varnish-macros` | proc-macro crate: `#[vmod]`, `derive(VscMetric)`, `run_vtc_tests!` | [varnish-macros/AGENTS.md](varnish-macros/AGENTS.md) |
| `varnish` | public facade crate, what VMOD authors depend on | [varnish/AGENTS.md](varnish/AGENTS.md) |
| `varnish/tests` | trybuild+insta macro-output tests, `vmod_test` integration subcrate | [varnish/tests/AGENTS.md](varnish/tests/AGENTS.md) |
| `examples/*` | 14 example VMODs + `stat_tree` bin | [examples/AGENTS.md](examples/AGENTS.md) |

Dependency direction: `varnish-sys` ← `varnish-macros` ← `varnish` ← examples. `varnish` is a thin facade re-exporting the other two.

## Version matrix

`varnish-rs` 0.7+ supports libvarnish 8.0 and 9.0 (+ trunk in CI). Older ranges: see README.md table. `varnish-sys/build.rs` detects installed version via pkg-config, emits `cfg(varnishsys_*)` flags — code gates version-specific fields/fns behind these, never behind crate version.

## Build / test

Use `just` (install: `cargo install just`). Key recipes:

- `just build` / `just test` — needs real `varnishapi` installed (pkg-config finds it, or set `VARNISH_INCLUDE_PATHS=path1:path2`, assumes latest version if used).
- `just bless` — regen insta snapshots + trybuild `.stderr` for current Varnish version (`-p varnish-macros -p varnish`).
- `just bless-all` — same, across all supported versions via Docker.
- `just docker-run <version>` — multi-version dev container, state cached in `docker/.cache/<version>`.
- `just install-varnish <version|trunk>` — installs a specific libvarnish for local dev.

`just test` runs real `.vtc` files via `run_vtc_tests!`, which shells out to `varnishtest` binary and forks real `varnishd` — needs working Varnish install, not just headers.

## Gotchas

- **macOS linking**: VMODs reference `VRT_*`/`VSL*`/`WS_*`/`BS_*` symbols living in `varnishd` binary, not `libvarnishapi`. Linux resolves at `dlopen` (flat namespace); macOS needs `-Wl,-undefined,dynamic_lookup` at link time. Repo's `.cargo/config.toml` sets this for in-tree builds; downstream VMOD crates need same snippet. See CONTRIBUTING.md.
- Debugging macro-generated code: `cargo expand`, or read `varnish/snapshots*/*@code.snap`. Full recipe in CONTRIBUTING.md.
- No `.vcc` files anywhere — this framework fully replaces that toolchain.
