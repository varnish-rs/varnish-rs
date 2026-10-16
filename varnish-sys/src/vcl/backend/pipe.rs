//! What [`VclBackend::pipe`](crate::vcl::VclBackend::pipe) gets to work with: the client
//! connection, the bytes Varnish already consumed from it, and the byte accounting `wrap_pipe`
//! logs once `pipe()` returns.
use std::cell::Cell;
use std::ffi::CStr;
use std::io::{self, Write};
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::net::TcpStream;
use std::os::unix::io::FromRawFd;
use std::ptr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::ffi;
use crate::vcl::ctx::cached_req_body_chunks;
use crate::vcl::{StreamClose, VclError};

/// Byte accounting for one pipe transaction, logged as the `PipeAcct` record once
/// [`VclBackend::pipe`](crate::vcl::VclBackend::pipe) returns.
///
/// It uses atomics, so threads copying data can update it through a shared reference (see
/// [`PipeRequest::acct`]). As in core's `PipeAcct` record, "in" is client to backend and "out" is
/// backend to client.
#[derive(Debug, Default)]
pub struct PipeAcct {
    bereq_hdr: AtomicU64,
    in_: AtomicU64,
    out: AtomicU64,
    /// `f64::to_bits` of the first `add_*` call's time, 0 if none yet.
    t_first: AtomicU64,
    /// `f64::to_bits` of the last time bytes moved, 0 if they never did.
    t_last: AtomicU64,
}

impl PipeAcct {
    /// Count `n` bytes of backend request headers (the `bereq` field of `PipeAcct`).
    pub fn add_bereq_hdrbytes(&self, n: u64) {
        self.bereq_hdr.fetch_add(n, Ordering::Relaxed);
        self.mark_first(now());
    }

    /// Count `n` bytes moved from the client to the backend.
    pub fn add_in(&self, n: u64) {
        self.in_.fetch_add(n, Ordering::Relaxed);
        self.touch();
    }

    /// Count `n` bytes moved from the backend to the client.
    pub fn add_out(&self, n: u64) {
        self.out.fetch_add(n, Ordering::Relaxed);
        self.touch();
    }

    /// Backend request header bytes counted so far.
    pub fn bereq_hdrbytes(&self) -> u64 {
        self.bereq_hdr.load(Ordering::Relaxed)
    }

    /// Client-to-backend bytes counted so far.
    pub fn in_bytes(&self) -> u64 {
        self.in_.load(Ordering::Relaxed)
    }

    /// Backend-to-client bytes counted so far.
    pub fn out_bytes(&self) -> u64 {
        self.out.load(Ordering::Relaxed)
    }

    /// When the first `add_*` call happened, or `None` if none did. This starts the pipe (see
    /// [`PipeRequest`]).
    pub fn t_first(&self) -> Option<SystemTime> {
        load_time(&self.t_first).map(to_system_time)
    }

    /// When bytes last moved, or `None` if they never did.
    pub fn t_last(&self) -> Option<SystemTime> {
        load_time(&self.t_last).map(to_system_time)
    }

    fn touch(&self) {
        let now = now();
        self.mark_first(now);
        self.t_last.fetch_max(now.to_bits(), Ordering::Relaxed);
    }

    fn mark_first(&self, now: f64) {
        // Only the first caller wins, later ones see a non-zero value.
        let _ =
            self.t_first
                .compare_exchange(0, now.to_bits(), Ordering::Relaxed, Ordering::Relaxed);
    }
}

fn load_time(a: &AtomicU64) -> Option<f64> {
    match a.load(Ordering::Relaxed) {
        0 => None,
        bits => Some(f64::from_bits(bits)),
    }
}

/// Same clock as `VTIM_real()`, in seconds since the epoch. Callable from any thread.
fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn to_system_time(secs: f64) -> SystemTime {
    UNIX_EPOCH + Duration::try_from_secs_f64(secs).unwrap_or_default()
}

/// The client side of a `vcl_pipe` transaction, handed to
/// [`VclBackend::pipe`](crate::vcl::VclBackend::pipe).
///
/// The pipe "starts" with the first data movement: the first call to
/// [`take_pipelined`](Self::take_pipelined) or [`forward_preamble`](Self::forward_preamble), or
/// the first [`PipeAcct`] `add_*` call, whichever comes first. [`client`](Self::client) never
/// does, so the client socket can be set up before connecting to the backend.
///
/// Once `pipe()` returns, `wrap_pipe` logs the `PipeAcct` record, plus the `Pipe` and `PipeSess`
/// timestamps if the pipe started, like core does for its own backends. A `pipe()` that returns
/// without starting (say, because the backend connection failed) logs no timestamps.
///
/// Not `Send` or `Sync`: it points at the request, which only the calling worker thread may
/// touch. Threads spawned by `pipe()` may use the `&TcpStream` from [`client`](Self::client) and
/// the `&PipeAcct` from [`acct`](Self::acct), but never the `Ctx` (including `ctx.log()`).
///
/// ```compile_fail,E0277
/// # mod varnish { pub use varnish_sys::vcl; }
/// fn is_send<T: Send>() {}
/// is_send::<varnish::vcl::PipeRequest<'static>>();
/// ```
///
/// ```compile_fail,E0277
/// # mod varnish { pub use varnish_sys::vcl; }
/// fn is_sync<T: Sync>() {}
/// is_sync::<varnish::vcl::PipeRequest<'static>>();
/// ```
#[derive(Debug)]
pub struct PipeRequest<'a> {
    req: *mut ffi::req,
    bo: *mut ffi::busyobj,
    client: ManuallyDrop<TcpStream>,
    acct: PipeAcct,
    /// When the pipe was started from the calling thread (and `Pipe` logged).
    t_start: Cell<Option<f64>>,
    refused: bool,
    _req: PhantomData<&'a mut ffi::req>,
}

impl<'a> PipeRequest<'a> {
    /// SAFETY: `req` and `bo` must be valid for `'a`, and `fd` must be the client socket of `req`,
    /// owned by its session.
    pub(super) unsafe fn new(req: *mut ffi::req, bo: *mut ffi::busyobj, fd: i32) -> Self {
        Self {
            req,
            bo,
            // `ManuallyDrop`: `cnt_pipe` closes the fd (`SES_Close`) after `.http1pipe` returns,
            // closing it here as well would close it twice.
            client: ManuallyDrop::new(TcpStream::from_raw_fd(fd)),
            acct: PipeAcct::default(),
            t_start: Cell::new(None),
            refused: false,
            _req: PhantomData,
        }
    }

    /// The client connection. Using it never starts the pipe (see [`PipeRequest`]).
    ///
    /// This is the raw socket: `pipe()` is never called for TLS client sessions (`-A`), whose
    /// bytes on the wire are encrypted.
    ///
    /// Don't close it: Varnish owns the file descriptor and closes it after `pipe()` returns.
    /// Shutting it down (`shutdown()`) or changing its timeouts is fine. `Read` and `Write` are
    /// implemented for `&TcpStream`, so `req.client().write_all(...)` works. Being a shared
    /// reference, it can't be used to swap the stream out and drop it:
    ///
    /// ```compile_fail,E0308
    /// # mod varnish { pub use varnish_sys::vcl; }
    /// fn steal(req: &mut varnish::vcl::PipeRequest<'_>) {
    ///     // would close Varnish's fd early, and leak the replacement
    ///     let replacement = req.client().try_clone().unwrap();
    ///     drop(std::mem::replace(req.client(), replacement));
    /// }
    /// ```
    pub fn client(&self) -> &TcpStream {
        &self.client
    }

    /// Bytes the client sent after the request headers that Varnish already read from the socket:
    /// the start of a request body, a WebSocket's first frame, pipelined requests... They must
    /// reach the backend before anything else read from [`client`](Self::client). Starts the
    /// pipe.
    ///
    /// Returns them once, like core: later calls return `None`. Count them with
    /// [`PipeAcct::add_in`] once forwarded (or use [`forward_preamble`](Self::forward_preamble),
    /// which does both).
    pub fn take_pipelined(&mut self) -> Option<&'a [u8]> {
        self.start();
        // SAFETY: `req` is valid for `'a` (see `new`), and so is its `htc` while a client request
        // is processed.
        unsafe {
            let htc = (*self.req).htc;
            if htc.is_null() || (*htc).pipeline_b.is_null() {
                return None;
            }
            let b = (*htc).pipeline_b;
            let e = (*htc).pipeline_e;
            (*htc).pipeline_b = ptr::null_mut();
            (*htc).pipeline_e = ptr::null_mut();
            let len = usize::try_from(e.offset_from(b)).ok()?;
            // `b..e` lies in `req->ws`, which core only resets after `.http1pipe` returns, and
            // `'a` can't outlive `pipe()`.
            Some(std::slice::from_raw_parts(b.cast::<u8>(), len))
        }
    }

    /// The request body, if VCL cached it (`std.cache_req_body()`), as a list of chunks.
    ///
    /// Without caching, the body is still on the client connection (or in
    /// [`take_pipelined`](Self::take_pipelined)), and gets piped like any other byte.
    pub fn cached_body(&self) -> Result<Vec<&'a [u8]>, VclError> {
        // SAFETY: `req` is valid for `'a`.
        unsafe { cached_req_body_chunks(self.req) }
    }

    /// Writes to `origin` (the backend connection, after sending it the request headers) what
    /// Varnish already consumed from the client, like core does: the cached body if any, then the
    /// pipelined bytes. Starts the pipe.
    ///
    /// The cached body is counted as "out" and the pipelined bytes as "in". Counting the body as
    /// "out" (backend to client) looks wrong, but it is what core does for its own backends, so
    /// the `PipeAcct` records match.
    pub fn forward_preamble<W: Write>(&mut self, origin: &mut W) -> io::Result<()> {
        self.start();
        // SAFETY: `req` is valid for `'a`.
        if unsafe { (*self.req).req_body_status } == unsafe { ffi::BS_CACHED.as_ptr() } {
            // A failed iteration must not go unnoticed: the backend would wait for a body that
            // never comes.
            let chunks = self
                .cached_body()
                .map_err(|e| io::Error::other(e.to_string()))?;
            for chunk in chunks {
                origin.write_all(chunk)?;
                self.acct.add_out(chunk.len() as u64);
            }
        }
        if let Some(bytes) = self.take_pipelined() {
            origin.write_all(bytes)?;
            self.acct.add_in(bytes.len() as u64);
        }
        Ok(())
    }

    /// The accounting logged when `pipe()` returns.
    pub fn acct(&self) -> &PipeAcct {
        &self.acct
    }

    /// How long the pipe may stay idle (`pipe_timeout` parameter), `Duration::ZERO` meaning no
    /// timeout. Core's own backends close the pipe after that long without traffic in either
    /// direction.
    // A method, not an associated function: it's a property of this pipe, even if Varnish only
    // has a global value for now.
    #[allow(clippy::unused_self)]
    pub fn pipe_timeout(&self) -> Duration {
        // SAFETY: `cache_param` is set for the whole lifetime of the child process.
        let secs = unsafe { (*ffi::cache_param).pipe_timeout.0 };
        Duration::try_from_secs_f64(secs).unwrap_or(Duration::ZERO)
    }

    /// When the whole pipe transaction must end, if ever: `bereq.task_deadline` if set, otherwise
    /// the `pipe_task_deadline` parameter, counted from when the session last went idle (before
    /// this request), as core does for its own backends.
    pub fn deadline(&self) -> Option<SystemTime> {
        // SAFETY: `req`, `req->sp` and `bo` are valid for `'a`; `cache_param` is always set.
        let (task_deadline, t_idle) = unsafe {
            let mut d = (*self.bo).task_deadline.0;
            if d.is_nan() {
                d = (*ffi::cache_param).pipe_task_deadline.0;
            }
            (d, (*(*self.req).sp).t_idle.0)
        };
        (task_deadline > 0.0).then(|| to_system_time(t_idle + task_deadline))
    }

    /// Declines the pipe. Return its value from `pipe()`.
    ///
    /// Only effective before the pipe started: the request is then accounted as if never piped
    /// (`ReqAcct` instead of `PipeAcct`), like with a director that can't pipe. After the pipe
    /// started, it is logged and otherwise ignored.
    pub fn refuse(&mut self) -> StreamClose {
        self.refused = true;
        StreamClose::TxError
    }

    /// Whether the pipe started, and when.
    pub fn started(&self) -> Option<SystemTime> {
        self.started_secs().map(to_system_time)
    }

    fn started_secs(&self) -> Option<f64> {
        match (self.t_start.get(), load_time(&self.acct.t_first)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn start(&self) {
        if self.t_start.get().is_some() {
            return;
        }
        let now = now();
        self.t_start.set(Some(now));
        // SAFETY: `req` is valid for `'a`.
        unsafe { ts_req(self.req, c"Pipe", now) };
    }

    /// Logs what core's own backends log at the end of a pipe: the `PipeSess` timestamp and
    /// `PipeAcct`. Returns whether the request was piped, `false` if it was refused before
    /// starting.
    ///
    /// SAFETY: must only be called once, from `wrap_pipe`, after `pipe()` returned.
    pub(super) unsafe fn finish(&self, req_hdrbytes: u64) -> bool {
        let req = self.req;
        // Started only through `PipeAcct::add_*`, possibly on a thread that can't log: log `Pipe`
        // now, with the time of that first call.
        if self.t_start.get().is_none() {
            if let Some(t) = self.started_secs() {
                ts_req(req, c"Pipe", t);
            }
        }
        if self.started_secs().is_none() {
            if self.refused {
                // Like a director without `.http1pipe`: the request was never piped.
                (*req).acct.req_hdrbytes += req_hdrbytes;
                return false;
            }
        } else {
            if self.refused {
                ffi::VSLbt(
                    (*req).vsl.as_mut_ptr(),
                    ffi::VslTag::Error,
                    ffi::txt::from_str("pipe: refuse() after the pipe started, ignored"),
                );
            }
            ts_req(req, c"PipeSess", now());
        }
        // Like core's backends: the request header bytes go to `PipeAcct`, so core must not log
        // `ReqAcct` as well.
        (*req).set_res_pipe(1);
        // Same format as core's `V1P_Charge()`.
        let msg = format!(
            "{} {} {} {}",
            req_hdrbytes,
            self.acct.bereq_hdrbytes(),
            self.acct.in_bytes(),
            self.acct.out_bytes()
        );
        ffi::VSLbt(
            (*req).vsl.as_mut_ptr(),
            ffi::VslTag::PipeAcct,
            ffi::txt::from_str(&msg),
        );
        true
    }
}

/// `VSLb_ts_req()`, which is a `static inline` in Varnish 9.0's `cache.h`, so not bindable there.
///
/// SAFETY: `req` must be valid.
unsafe fn ts_req(req: *mut ffi::req, event: &CStr, now: f64) {
    let r = &mut *req;
    if r.t_first.0.is_nan() || r.t_first.0 == 0.0 {
        r.t_first = ffi::vtim_real(now);
        r.t_prev = ffi::vtim_real(now);
    }
    ffi::VSLb_ts(
        r.vsl.as_mut_ptr(),
        event.as_ptr(),
        r.t_first,
        &raw mut r.t_prev,
        ffi::vtim_real(now),
    );
}
