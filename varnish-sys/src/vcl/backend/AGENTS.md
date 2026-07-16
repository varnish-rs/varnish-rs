# backend/ — custom backend & director subsystem

See also: [vcl/](../AGENTS.md) · [examples](../../../../examples/AGENTS.md)

Lets a VMOD implement a custom Varnish backend (director) in Rust — health-checked, streaming, or load-balancing origins.

## Files

- `backend_main.rs` (biggest file in crate) — `Backend<S,T>`, `VclBackend`/`VclResponse` traits (what a VMOD implements), `NativeBackendBuilder`, `StreamClose`, `sc_to_ptr` (`pub(crate)` — also called from `../ctx.rs`). Has unit tests.
- `backend_ref.rs` — backend reference/handle glue.
- `director.rs` — VMOD director glue (routing requests to a backend impl).

Reading `bereq`'s body from `VclBackend::get_response`: `Ctx::req_body_read`/`req_body_status` in [`../ctx.rs`](../AGENTS.md), not here — backend-only (needs `ctx.raw.bo`), but kept next to the client-side `cached_req_body` for symmetry.

## Real usage — read these examples alongside this code

- `examples/vmod_native_backend`, `examples/vmod_simple_backend` — implement `VclBackend`/`VclResponse` directly.
- `examples/vmod_echo_backend` — same, plus reads/forwards `bereq`'s body via `Ctx::req_body_read`.
- `examples/vmod_round_robin` — director picking among multiple backends.

## Gotcha

Backend traits sit right at the C ABI boundary (director vtable calls from `varnishd`). Changing `VclBackend`/`VclResponse` signatures breaks every example backend above — grep them before touching.
