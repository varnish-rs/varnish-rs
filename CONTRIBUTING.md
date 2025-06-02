# Testing

There are two types of tests in this project:

### `trybuild` tests
[trybuild](https://docs.rs/trybuild/latest/trybuild/) tests, which verify that the macro code can be compiled, or when there is a problem, the compiler errors are properly reported. The test cases are in the `varnish/tests` -- `pass/*.rs` work by default, `pass-ffi/*.rs` work only with the `ffi` feature is enabled, and `fail/*.rs` should fail. The expected errors are in the `fail/*.stderr` files. To update the expected output, run `just bless`.

### `insta` tests
[insta](https://insta.rs/docs/cli/) tests, which verify that the macros produce the correct output when used correctly, and also that the output is stable. The test uses the same `pass` and `pass-ffi` files, and the expected output is in the `varnish/snapshots/*.snap` files. The test runner must be part of the crate itself due to it being proc-macro, so it is located in `varnish-macros/src/tests.rs`. To update the snapshots, use the same `just bless` command. Make sure to install insta first.

### Debugging Macro-Generated Code

The simplest way to observe generated code is to examine `varnish/sanshots/*@code.snap` files. They contain code generated from the `varnish/test/pass/*.rs` files. Additionally, the model file describes intermediate parse result of the test file, json files shows the data given to the Varnish vmod compiler, and docs contain the generated documentation.

For a more in-depth look, use `cargo expand` command.  You will need to run `cargo install cargo-expand` to install it first. You can copy/paste the expanded code into the same file, removing some boilerplate before and after the expanded code, and then run `cargo check` to see the errors.  Note that some expansions cannot be compiled - e.g. anything expanded from `format!` or `panic!` - so you may need compare the generated code with the original, and keep all the original parts for anything unrelated to the generated code.

Note that the test cases in `varnish/tests` will not work with cargo expand unless you make it part of the regular compilation. For example, let's say we want to debug `varnish/test/pass/object.rs`.  Modify `varnish/tests/compiler.rs` like this:

```rust
#[path = "pass/object.rs"]
mod try_to_build;
```

Once you have done that, you can run the following command to save generated code to a file:

```shell
cargo expand -p varnish --test compile --tests try_to_build > varnish/tests/pass/object.expanded.rs
```

Delete the top 2 lines with `mod try_to_build {` and the last `}`,  and reformat `object.expanded.rs`  before comparing it with `object.rs` to see the resulting code.  Now, modify the `path` in the `compiler.rs` to point to the expanded file, and you should be able to run `cargo check --tests` to see the errors.

# Using Docker

Docker allows multiple Varnish versions on the same machine.  To use it, run `just docker-run` with an optional version number.  On the first run, it will build the image, install necessary dependencies, and start the container.  The container will have the git root directory mounted as `/app`, so you can run `cargo build` and `cargo test` as usual. All work is preserved in the `docker/.cache/<version>`, so restarting container would not need re-install everything.
