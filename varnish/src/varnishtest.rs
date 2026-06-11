use std::env;
use std::env::consts::{DLL_PREFIX, DLL_SUFFIX};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::io::{stderr, stdout, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Condvar, Mutex, OnceLock};

use glob::glob;

/// Cap on concurrent `varnishtest` invocations within a single test binary.
///
/// Each varnishtest forks `varnishd`, which must complete a CLI handshake
/// within a few seconds. Under heavy parallel load, that handshake can miss
/// its deadline. Capping concurrency keeps startups responsive without
/// requiring callers to pass `--test-threads`.
const MAX_CONCURRENT_TESTS: usize = 2;

static TEST_SLOTS: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();

fn test_slots() -> &'static (Mutex<usize>, Condvar) {
    TEST_SLOTS.get_or_init(|| (Mutex::new(MAX_CONCURRENT_TESTS), Condvar::new()))
}

struct TestSlot;

impl TestSlot {
    fn acquire() -> Self {
        let (lock, cvar) = test_slots();
        let mut available = lock.lock().expect("test slot mutex poisoned");
        while *available == 0 {
            available = cvar.wait(available).expect("test slot condvar wait failed");
        }
        *available -= 1;
        Self
    }
}

impl Drop for TestSlot {
    fn drop(&mut self) {
        let (lock, cvar) = test_slots();
        let mut available = lock.lock().expect("test slot mutex poisoned");
        *available += 1;
        cvar.notify_one();
    }
}

/// Run all tests that match the glob pattern
pub fn run_all_tests(
    ld_library_paths: &str,
    vmod_name: &str,
    glob_path: &str,
    timeout: &str,
    debug: bool,
) -> Result<(), String> {
    let vmod_lib_name = format!("{DLL_PREFIX}{vmod_name}{DLL_SUFFIX}");
    let vmod_path = find_vmod_lib(&vmod_lib_name, ld_library_paths)?;
    let mut found = false;
    let mut failed = Vec::new();
    for test in
        glob(glob_path).map_err(|e| format!("Failed to find any tests in '{glob_path}': {e}"))?
    {
        found = true;
        let file = test.map_err(|e| format!("Failed to get test path: {e}"))?;
        if let Err(err) = run_varnish_test(&vmod_path, &file, timeout, debug) {
            failed.push(format!("{}: {err}", file.display()));
            eprintln!("{err}");
        }
    }

    if !found {
        Err(format!("No tests found in '{glob_path}'"))
    } else if failed.is_empty() {
        Ok(())
    } else {
        let mut err = String::new();
        if failed.len() > 1 {
            // If we only had one failed test, we already printed the error
            let _ = write!(err, "{} tests failed:", failed.len());
            for f in failed {
                let _ = write!(err, "{f}");
            }
        }
        Err(err)
    }
}

/// Run a single varnishtest after locating the vmod shared library.
///
/// Concurrency is capped per-process regardless of `cargo test`'s thread
/// count, to avoid overwhelming `varnishd` CLI handshakes.
pub fn run_one_test(
    ld_library_paths: &str,
    vmod_name: &str,
    testfile: &Path,
    timeout: &str,
    debug: bool,
) -> Result<(), String> {
    let _slot = TestSlot::acquire();
    let vmod_lib_name = format!("{DLL_PREFIX}{vmod_name}{DLL_SUFFIX}");
    let vmod_path = find_vmod_lib(&vmod_lib_name, ld_library_paths)?;
    run_varnish_test(&vmod_path, testfile, timeout, debug)
}

/// Run a single `.vtc` test file against the given VMOD shared library.
///
/// This is normally invoked indirectly via the [`varnish::run_vtc_tests!`][crate::run_vtc_tests]
/// macro, which discovers test files by glob and handles the library path. Call this directly
/// only if you need finer control over which tests run or how failures are reported.
pub fn run_varnish_test(
    vmod_path: &Path,
    testfile: &Path,
    timeout: &str,
    debug: bool,
) -> Result<(), String> {
    eprintln!("Running varnishtest {}", testfile.display());
    let mut cmd = Command::new("varnishtest");
    if debug {
        // Keep output, and run in verbose mode
        cmd.arg("-L").arg("-v");
    }

    let mut vmod_arg = OsString::from("vmod=");
    vmod_arg.push(vmod_path);

    cmd.arg("-D")
        .arg(vmod_arg)
        .arg(testfile)
        .env("VARNISHTEST_DURATION", timeout);

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run varnishtest:\n{cmd:?}\n{e}"))?;

    if debug || !output.status.success() {
        stdout()
            .write_all(&output.stdout)
            .map_err(|e| format!("failed to write varnishtest stdout: {e}"))?;
        stderr()
            .write_all(&output.stderr)
            .map_err(|e| format!("failed to write varnishtest stderr: {e}"))?;
    }

    if output.status.success() {
        Ok(())
    } else if output.status.code().unwrap_or_default() == 77 {
        eprintln!("varnishtest exited with code 77, skipping");
        Ok(())
    } else {
        Err(format!(
            "varnishtest {} failed\n{cmd:?}",
            testfile.display()
        ))
    }
}

/// Find the vmod so file
pub fn find_vmod_lib(vmod_lib_name: &str, ld_library_paths: &str) -> Result<PathBuf, String> {
    env::split_paths(ld_library_paths)
        .map(|p| p.join(vmod_lib_name))
        .find(|p| p.exists())
        .ok_or_else(|| {
            format!("Unable to find {vmod_lib_name} in {ld_library_paths}\nHave you built your vmod first?")
        })
}
