# backend/ — custom backend & director subsystem

See also: [vcl/](../AGENTS.md) · [examples](../../../../examples/AGENTS.md)

Lets a VMOD implement a custom Varnish backend (director) in Rust — health-checked, streaming, or load-balancing origins.

## Files

- `backend_main.rs` (biggest file in crate) — `Backend<S,T>`, `VclBackend`/`VclResponse` traits (what a VMOD implements), `NativeBackendBuilder`, `StreamClose`. Has unit tests.
- `backend_ref.rs` — backend reference/handle glue.
- `director.rs` — VMOD director glue (routing requests to a backend impl).

## Real usage — read these examples alongside this code

- `examples/vmod_native_backend`, `examples/vmod_simple_backend` — implement `VclBackend`/`VclResponse` directly.
- `examples/vmod_round_robin` — director picking among multiple backends.

## Gotcha

Backend traits sit right at the C ABI boundary (director vtable calls from `varnishd`). Changing `VclBackend`/`VclResponse` signatures breaks every example backend above — grep them before touching.
