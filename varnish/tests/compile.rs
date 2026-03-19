// Use this hack to include a specific file into the regular IDE code parsing to help fix a specific test file
// #[path = "pass/vcl_returns.rs"]
// mod try_to_build;

#[test]
fn compile_expected_failures() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/fail/*.rs");
}

#[test]
fn compile_valid() {
    compile_pass("tests/pass/*.rs");
}

#[cfg(feature = "ffi")]
#[test]
fn compile_valid_ffi_code() {
    compile_pass("tests/pass_ffi/*.rs");
}

fn compile_pass(pattern: &str) {
    let t = trybuild::TestCases::new();
    for file in glob::glob(pattern).unwrap() {
        t.pass(file.unwrap());
    }
}
