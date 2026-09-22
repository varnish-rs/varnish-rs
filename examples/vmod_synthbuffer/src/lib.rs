//varnish::run_vtc_tests!("tests/*.vtc");

/// Demonstrates `Ctx::response_buffer()`, which exposes the VSB used to build
/// the response body directly from `vcl_synth` or `vcl_backend_error` — the
/// only two subroutines where it's valid, enforced here with `#[restrict(...)]`.
#[varnish::vmod(docs = "README.md")]
mod synthbuffer {
    use varnish::vcl::Ctx;

    /// Push `s` onto the response body, unchanged.
    #[restrict(vcl_synth, vcl_backend_error)]
    pub fn push(ctx: &mut Ctx, s: &str) {
        let mut buf = ctx
            .response_buffer()
            .expect("push is #[restrict]-ed to vcl_synth/vcl_backend_error");
        buf.write(&s).expect("VSB write must succeed");
    }

    /// Push `s` onto the response body, with its bytes in reverse order.
    #[restrict(vcl_synth, vcl_backend_error)]
    pub fn push_reverse(ctx: &mut Ctx, s: &str) {
        let mut buf = ctx
            .response_buffer()
            .expect("push_reverse is #[restrict]-ed to vcl_synth/vcl_backend_error");
        for &byte in s.as_bytes().iter().rev() {
            buf.write(&[byte]).expect("VSB write must succeed");
        }
    }
}
