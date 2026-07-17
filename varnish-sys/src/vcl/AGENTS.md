# vcl/ — safe wrapper subsystem

See also: [varnish-sys](../../AGENTS.md) · [backend/ subsystem](backend/AGENTS.md) · [varnish-macros](../../../varnish-macros/AGENTS.md)

Re-exported whole as `varnish::vcl`. Wraps raw Varnish C structs with checked, mostly-safe Rust types. `mod.rs` re-exports everything + renames `VclEvent as Event`, `VslTag as LogTag`.

## Files

- `acl.rs` — `Acl` wrapper.
- `convert.rs` (large) — Rust ⇄ `VCL_*` conversion traits. **Macro-generated code depends on these directly** — changing a trait here ripples into `varnish-macros` codegen.
- `ctx.rs` — `Ctx`, `Req`, `TestCtx`, `PerVclState<T>`, `log()`. Main request-context wrapper around `vrt_ctx`. Has unit tests. Also holds `Ctx::cached_req_body()` (client `req` body, cached-only) and the backend-side pair `Ctx::req_body_state()`/`Ctx::req_body()` + `BodyState` enum (bereq's body, cached or streamed from the client — mirrors `body_status_t`/`V1F_SendReq` in Varnish itself). The backend pair calls into `backend/`'s `sc_to_ptr`/`StreamClose` for `doclose` bookkeeping, despite living in this file, not `backend/backend_main.rs` — kept next to `cached_req_body` for symmetry (client-body vs bereq-body reading), not split by module.
- `error.rs` — VCL error type.
- `http.rs` — `HttpHeaders`, header iteration. **Assumes UTF-8, panics otherwise** (issue #4, known not fixed).
- `probe.rs` — `Probe`/`CowProbe`/`Request`, backend health checks.
- `processor.rs` — `DeliveryProcessor`/`FetchProcessor` traits, `FetchFilters`/`DeliveryFilters`, `new_vdp`/`new_vfp`. Backs `examples/vmod_vdp`, `vmod_vfp`.
- `str_or_bytes.rs` — `StrOrBytes<'a>` enum.
- `subroutine.rs` — `Subroutine` wrapper, `VALID_RESTRICT_SCOPES`, `bitmask_const_name`. **Consumed directly by `varnish-macros`** (parser.rs, gen_func.rs) for `#[restrict(...)]` codegen.
- `vsb.rs` — `Buffer`, wraps Varnish `vsb` string buffer.
- `ws.rs` — `Workspace<'ctx>`, `TestWS`. Per-task arena allocator. Has unit tests.
- `ws_str_buffer.rs` — `WsBuffer`/`WsStrBuffer`/`WsBlobBuffer` builders on top of workspace. Has unit tests.
- `backend/` — director/backend subsystem, own guide: [backend/AGENTS.md](backend/AGENTS.md).

## Conventions

- **Validate before trust**: any function taking a raw pointer from C calls into `../validate.rs`'s magic-number checks first. Follow this pattern for new wrappers — don't trust a C pointer's type tag blind.
- `unsafe` is the norm here, not the exception — this is the boundary layer. Push safety further up, not down.
- `convert.rs` traits are the seam macro-generated code plugs into. Changing a trait signature = check `varnish-macros/src/gen_func.rs` for breakage.
