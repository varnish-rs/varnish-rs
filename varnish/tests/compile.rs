// Use this hack to include a specific file into the regular IDE code parsing to help fix a specific test file
// #[path = "pass/vcl_returns.rs"]
// mod try_to_build;

#[test]
fn compile_expected_failures() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/fail/*.rs");
}

// Types that need the `full` feature must be rejected by `#[vmod]` in vrt-only builds.
#[cfg(not(feature = "full"))]
#[test]
fn compile_expected_failures_vrt() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/fail_vrt/*.rs");
}

#[test]
fn compile_valid() {
    compile_pass("tests/pass/*.rs");
}

// Fixtures using APIs that only exist with the `full` feature (filters, probes, `SocketAddr`).
#[cfg(feature = "full")]
#[test]
fn compile_valid_full() {
    compile_pass("tests/pass_full/*.rs");
}

#[cfg(feature = "ffi")]
#[test]
fn compile_valid_ffi_code() {
    compile_pass("tests/pass_ffi/*.rs");
}

fn compile_pass(pattern: &str) {
    let t = trybuild::TestCases::new();
    for file in glob::glob(pattern).expect("glob pattern must be valid") {
        t.pass(file.expect("glob entry must be accessible"));
    }
}
