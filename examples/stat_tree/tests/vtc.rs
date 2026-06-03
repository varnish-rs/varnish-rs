use std::ffi::OsString;
use std::io::{stderr, stdout, Write};
use std::path::Path;
use std::process::Command;

fn run_vtc(testfile: &Path) {
    let binary = env!("CARGO_BIN_EXE_stat_tree");

    let mut stat_tree_arg = OsString::from("stat_tree=");
    stat_tree_arg.push(binary);

    let mut cmd = Command::new("varnishtest");
    cmd.arg("-D").arg(stat_tree_arg).arg(testfile);

    let output = cmd.output().expect("failed to run varnishtest");

    if !output.status.success() {
        stdout().write_all(&output.stdout).unwrap();
        stderr().write_all(&output.stderr).unwrap();
        panic!("varnishtest {} failed\n{cmd:?}", testfile.display());
    }
}

#[test]
fn test01() {
    run_vtc(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/test01.vtc"
    )));
}

#[test]
fn test02() {
    run_vtc(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/test02.vtc"
    )));
}
