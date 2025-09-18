varnish::run_vtc_tests!("tests/*.vtc");

/// Parse files into numbers
///
/// This is a simple example of how to handle errors in a Varnish VMOD.
/// All three functions will do the same thing: read a file and try to parse its content into a `VCL_INT`.
/// However, they will handle failure (file not found, permission issue, unparsable content, etc.) differently.
#[varnish::vmod(docs = "README.md")]
mod error {
    use std::fs::read_to_string;

    use varnish::vcl::Ctx;

    /// This function never fails, returning 0 if anything goes wrong
    pub fn cannot_fail(path: &str) -> i64 {
        // try to read the path at fp into a string, but return if there was an error
        let Ok(content) = read_to_string(path) else {
            return 0;
        };

        // try to convert the string into an i64, if parsing fails, force 0
        // no need to return as the last expression is automatically returned
        content.parse().unwrap_or(0)
    }

    /// If the file cannot be parsed into an INT, the vmod will trigger a VCL error,
    /// stopping the processing of the request and logging the error.
    /// The client will receive an error message with a 500 status code.
    ///
    /// We call `ctx.fail()` ourselves, but we still need to return an i64.
    pub fn manual_fail(ctx: &mut Ctx, fp: &str) -> i64 {
        // try to read the path at fp into a string, or return 0 if there was an error
        let Ok(content) = read_to_string(fp) else {
            ctx.fail("manual_fail: couldn't read file into string");
            return 0;
        };

        // try to convert the string into an i64
        // no need to return as the last expression is automatically returned
        let Ok(result) = content.parse() else {
            ctx.fail("manual_fail: conversion failed");
            return 0;
        };

        result
    }

    /// From a user perspective, this function does the same thing as `.manual_fail()`,
    /// except its underlying `rust` implementation is slightly different.
    ///
    /// In a more idiomatic way, we return a Result, and the generated boilerplate will be in charge of
    /// calling `ctx.fail() and return a default value.
    pub fn result_fail(fp: &str) -> Result<i64, String> {
        // read the file
        read_to_string(fp)
            // convert the error (if any!), into a string and return right away with the `?` operator
            .map_err(|e| format!("result_fail: {e}"))?
            // try to parse content into i64
            .parse()
            // map the error to a string message and return either the parsed integer or that error
            .map_err(|e| format!("result_fail: {e}"))
    }
}
