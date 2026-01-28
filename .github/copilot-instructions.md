# varnish-rs AI Coding Guide

## Project Overview

This is **varnish-rs**, a Rust framework for building Varnish Cache VMODs (modules). It provides safe, idiomatic Rust bindings to the Varnish C API through a three-crate architecture:

- **varnish-sys**: Low-level FFI bindings generated via bindgen from libvarnish headers
- **varnish-macros**: Proc-macro crate that handles `#[varnish::vmod]` attribute and code generation
- **varnish**: High-level safe API that VMOD authors use (see [examples/](examples/))

## Critical Build System Knowledge

### Multi-Version Varnish Support

This project supports multiple Varnish versions (6.0 LTS, 7.5+, 8.0+). Version detection happens at build time:

- `varnish-sys/build.rs` detects installed libvarnish version via `pkg-config` and sets `DEP_VARNISHAPI_VERSION_NUMBER`
- `varnish/build.rs` consumes this to set conditional compilation flags like `cfg(varnishsys_6)`
- For non-standard installs, use `VARNISH_INCLUDE_PATHS=/path1:/path2` (assumes latest version)

### Snapshot-Based Testing

Generated code is validated against version-specific snapshots in `varnish/snapshots*` directories:

- `just bless` updates all snapshots (runs `TRYBUILD=overwrite cargo insta test --accept`)
- `just bless-all` regenerates snapshots for ALL supported Varnish versions via Docker
- Two test types: **trybuild** (compile pass/fail) and **insta** (generated code stability)
- Snapshots include: `@code.snap` (generated code), `@model.snap` (parse tree), `@json.snap` (vmod spec), `@docs.snap`

### Just Commands (Critical Workflows)

**Use `just` not `cargo` for most tasks:**

```bash
just test          # Build + run all tests (must build before test for proc-macro reasons)
just bless         # Update test snapshots for current Varnish version
just docker-run 7.6 "just test"   # Test against specific Varnish version
just ci-test       # Run full CI suite (fmt, clippy, test, check git clean)
just fmt           # Format code (uses nightly if available for import sorting)
```

## VMOD Development Patterns

### Standard VMOD Structure

Every VMOD follows this pattern (see [examples/vmod_example/](examples/vmod_example/)):

```rust
// In lib.rs - required for VTC tests
varnish::run_vtc_tests!("tests/*.vtc");

#[varnish::vmod(docs = "README.md")]  // Auto-generates README from docstrings
mod my_vmod_name {
    /// Docstring becomes VCL documentation
    pub fn my_function(
        /// Param docs shown in generated VCC file
        arg: i64
    ) -> String {
        format!("result: {}", arg)
    }
}
```

**Cargo.toml must specify:**
```toml
[lib]
crate-type = ["cdylib"]  # Required for Varnish to load as shared library
```

### Object-Based VMODs

For stateful VMODs (see [examples/vmod_object/](examples/vmod_object/)):

```rust
#[allow(non_camel_case_types)]
pub struct my_object {
    // Use interior mutability (DashMap, Mutex, etc.) since methods get &self
    storage: DashMap<String, String>,
}

#[varnish::vmod]
mod object_vmod {
    impl my_object {
        pub fn new(capacity: Option<i64>) -> Self {
            // Constructor becomes VCL `new` statement
        }
        
        pub fn method(&self, arg: &str) -> String {
            // Regular method becomes VCL object.method()
        }
    }
}
```

### Special Attributes

- `#[event]` - Mark function as VMOD event handler (load/discard notifications)
- `#[shared_per_task]` - Function parameter is `PRIV_TASK` (per-VCL-transaction state)
- `#[shared_per_vcl]` - Function parameter is `PRIV_VCL` (per-VCL-load state)
- `#[vcl_name]` - Constructor parameter receives VCL object name as string

## Debugging Generated Code

### Quick Method - Check Snapshots
Look at `varnish/snapshots<version>/*@code.snap` files for current generated output from test cases.

### Cargo Expand Method
```bash
# Make a test case part of regular compilation
# Edit varnish/tests/compiler.rs to add:
#[path = "pass/object.rs"]
mod debug_this;

# Expand and save
cargo expand -p varnish --test compile --tests debug_this > expanded.rs

# Clean up first/last lines, reformat, then cargo check for errors
```

### Running Tests

**Always build before test** due to proc-macro limitations:
```bash
cargo build && cargo test    # Or use `just test`
```

VTC tests use varnishtest utility and require `${vmod}` placeholder in VCL imports.

## Code Conventions

### Workspace Structure
- All crates share version/metadata defined in root `Cargo.toml` `[workspace.package]`
- Versions updated via `release-plz` (see [release-plz.toml](release-plz.toml))
- MSRV: Rust 1.82+ (tracked in workspace, verified via `just msrv`)

### Clippy Configuration
Pedantic lints enabled with specific allows (see [Cargo.toml](Cargo.toml) `[workspace.lints.clippy]`):
- `missing_safety_doc` currently allowed (should be fixed eventually)
- `module_name_repetitions`, `must_use_candidate`, `cast_*` explicitly allowed

### CI Expectations
CI mode activates via `CI=true` env var, sets `RUSTFLAGS='-D warnings'`:
- Run locally with: `CI=true just ci-test`
- All changes must pass `just ci-test` + `assert-git-is-clean` (no uncommitted generated files)

## Cross-Version Compatibility

When modifying code generation or API:
1. Test against all versions: `just bless-all` (uses Docker containers)
2. Check conditional compilation in `varnish/build.rs` for version-specific behavior
3. Version-specific code uses `#[cfg(varnishsys_6)]` for Varnish 6.x differences
4. New snapshots create `varnish/snapshots<major>.<minor>.<patch>/` directories automatically
