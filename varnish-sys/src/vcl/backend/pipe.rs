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

    /// Writes all of `buf` to `backend`, like `write_all()`, counting each successful `write()`
    /// with [`add_in`](Self::add_in). On error, the bytes written before it stay counted.
    pub fn write_in<W: Write>(&self, backend: &mut W, buf: &[u8]) -> io::Result<()> {
        write_counted(backend, buf, |n| self.add_in(n))
    }

    /// Writes all of `buf` to `client`, like `write_all()`, counting each successful `write()`
    /// with [`add_out`](Self::add_out). On error, the bytes written before it stay counted.
    pub fn write_out<W: Write>(&self, client: &mut W, buf: &[u8]) -> io::Result<()> {
        write_counted(client, buf, |n| self.add_out(n))
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

    /// When the first `add_*` call happened, or `None` if none did. That call starts the pipe if
    /// nothing did before, but this time is only its `Pipe` timestamp if no [`PipeRequest`] method
    /// started it (see [`PipeRequest::started`]).
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
/// The `Pipe` timestamp, also returned by [`started`](Self::started), is the time of the first
/// `take_pipelined()`/`forward_preamble()` call, logged right away, even if `add_*` calls came
/// first. Only if neither is ever called is it the time of the first `add_*` call, logged once
/// `pipe()` returns. Core stamps `Pipe` once it has sent the request headers and the cached body,
/// just before the pipelined bytes. Following the steps of
/// [`VclBackend::pipe`](crate::vcl::VclBackend::pipe), `forward_preamble()` stamps it after the
/// request headers, but before the cached body.
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
        unsafe { take_pipelined(self.req) }
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
    ///
    /// On error, the bytes written before it are still counted, as core does for the request body
    /// (core does not count a partial write of the pipelined bytes, it treats it as fatal).
    pub fn forward_preamble<W: Write>(&mut self, origin: &mut W) -> io::Result<()> {
        self.start();
        // SAFETY: `req` is valid for `'a`.
        let cached = unsafe { (*self.req).req_body_status } == unsafe { ffi::BS_CACHED.as_ptr() };
        let chunks = if cached {
            // A failed iteration must not go unnoticed: the backend would wait for a body that
            // never comes.
            self.cached_body()
                .map_err(|e| io::Error::other(e.to_string()))?
        } else {
            Vec::new()
        };
        let req = self.req;
        // SAFETY: `req` is valid for `'a`, and so is its `htc`.
        forward(
            origin,
            &chunks,
            || unsafe { take_pipelined(req) },
            &self.acct,
        )
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

    /// Whether the pipe started, and when: its `Pipe` timestamp (see [`PipeRequest`]).
    ///
    /// That is the time of the first `take_pipelined()`/`forward_preamble()` call once there was
    /// one, even if [`PipeAcct`] `add_*` calls came first. Before that, it is the time of the
    /// first `add_*` call, which `wrap_pipe` logs as `Pipe` if no such call follows.
    pub fn started(&self) -> Option<SystemTime> {
        self.started_secs().map(to_system_time)
    }

    fn started_secs(&self) -> Option<f64> {
        self.t_start.get().or_else(|| load_time(&self.acct.t_first))
    }

    fn start(&self) {
        if let Some(now) = self.mark_start() {
            // SAFETY: `req` is valid for `'a`.
            unsafe { ts_req(self.req, c"Pipe", now) };
        }
    }

    /// Sets the start time to now if unset, returning it only when it did.
    fn mark_start(&self) -> Option<f64> {
        if self.t_start.get().is_some() {
            return None;
        }
        let now = now();
        self.t_start.set(Some(now));
        Some(now)
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

/// [`PipeRequest::take_pipelined`], without starting the pipe.
///
/// SAFETY: `req` must be valid for `'b`, and so must its `htc` (if set).
unsafe fn take_pipelined<'b>(req: *mut ffi::req) -> Option<&'b [u8]> {
    let htc = (*req).htc;
    if htc.is_null() || (*htc).pipeline_b.is_null() {
        return None;
    }
    let b = (*htc).pipeline_b;
    let e = (*htc).pipeline_e;
    (*htc).pipeline_b = ptr::null_mut();
    (*htc).pipeline_e = ptr::null_mut();
    let len = usize::try_from(e.offset_from(b)).ok()?;
    // `b..e` lies in `req->ws`, which core only resets after `.http1pipe` returns, and `'b` can't
    // outlive `pipe()`.
    Some(std::slice::from_raw_parts(b.cast::<u8>(), len))
}

/// [`PipeRequest::forward_preamble`] once its inputs are known: writes `body` (counted as "out"),
/// then, if that worked, what `pipelined` returns (counted as "in"). The pipelined bytes are only
/// taken once the body was sent, so a failed body write leaves them in place.
fn forward<'b, W: Write>(
    origin: &mut W,
    body: &[&[u8]],
    pipelined: impl FnOnce() -> Option<&'b [u8]>,
    acct: &PipeAcct,
) -> io::Result<()> {
    for chunk in body {
        // "out", like core, see `PipeRequest::forward_preamble`.
        acct.write_out(origin, chunk)?;
    }
    if let Some(bytes) = pipelined() {
        acct.write_in(origin, bytes)?;
    }
    Ok(())
}

/// `write_all()`, but passes each successful `write()` length to `count`, so bytes sent before an
/// error are still counted, like core does (`V1F_SendReq()` and `rdf_rw()` count what `write()`
/// returned, even on a later failure).
fn write_counted<W: Write>(w: &mut W, mut buf: &[u8], count: impl Fn(u64)) -> io::Result<()> {
    while !buf.is_empty() {
        match w.write(buf) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => {
                count(n as u64);
                buf = &buf[n..];
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use std::net::TcpListener;
    use std::os::unix::io::AsRawFd;
    use std::time::Duration;

    use super::{forward, to_system_time, write_counted, PipeAcct, PipeRequest};
    use crate::ffi;

    /// Accepts `budget` bytes, at most `chunk` per `write()` (after one `Interrupted`), then
    /// fails.
    struct Flaky {
        budget: usize,
        chunk: usize,
        interrupted: bool,
        written: Vec<u8>,
    }

    impl Write for Flaky {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            if self.budget == 0 {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            let n = buf.len().min(self.budget).min(self.chunk);
            self.budget -= n;
            self.written.extend_from_slice(&buf[..n]);
            Ok(n)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn flaky(budget: usize) -> Flaky {
        Flaky {
            budget,
            chunk: 3,
            interrupted: false,
            written: Vec::new(),
        }
    }

    #[test]
    fn partial_write_is_counted() {
        let acct = PipeAcct::default();
        let mut w = flaky(7);
        let err = write_counted(&mut w, b"0123456789", |n| acct.add_in(n)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(acct.in_bytes(), 7);
        assert_eq!(w.written, b"0123456");
    }

    #[test]
    fn full_write_is_counted() {
        let acct = PipeAcct::default();
        let mut w = flaky(100);
        write_counted(&mut w, b"0123456789", |n| acct.add_in(n)).unwrap();
        assert_eq!(acct.in_bytes(), 10);
        assert_eq!(w.written, b"0123456789");
    }

    #[test]
    fn write_zero_is_an_error() {
        struct Zero;
        impl Write for Zero {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Ok(0)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let acct = PipeAcct::default();
        let err = write_counted(&mut Zero, b"x", |n| acct.add_in(n)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::WriteZero);
        assert_eq!(acct.in_bytes(), 0);
    }

    #[test]
    fn acct_write_counts_partial_writes() {
        let acct = PipeAcct::default();
        let mut w = flaky(4);
        assert!(acct.write_out(&mut w, b"abcdef").is_err());
        assert_eq!((acct.out_bytes(), acct.in_bytes()), (4, 0));

        let mut w = flaky(5);
        assert!(acct.write_in(&mut w, b"abcdef").is_err());
        assert_eq!((acct.out_bytes(), acct.in_bytes()), (4, 5));

        let mut w = flaky(100);
        acct.write_out(&mut w, b"abcdef").unwrap();
        assert_eq!((acct.out_bytes(), acct.in_bytes()), (10, 5));
    }

    #[test]
    fn started_prefers_mark_start_over_first_accounting() {
        // SAFETY: `req` is plain data, and zeroed is valid for what `started()` and
        // `mark_start()` read.
        let mut req: Box<ffi::req> = Box::new(unsafe { std::mem::zeroed() });
        let sock = TcpListener::bind("127.0.0.1:0").unwrap();
        // SAFETY: `req` outlives `preq`, which never reads `bo`; `client` is `ManuallyDrop`, so
        // `sock` keeps owning the fd.
        let preq =
            unsafe { PipeRequest::new(&raw mut *req, std::ptr::null_mut(), sock.as_raw_fd()) };

        assert_eq!(preq.started(), None);
        preq.acct().add_bereq_hdrbytes(42);
        let t_first = preq.acct().t_first().unwrap();
        assert_eq!(preq.started(), Some(t_first));

        std::thread::sleep(Duration::from_millis(5));
        // What `start()` logs as `Pipe`.
        let t_start = to_system_time(preq.mark_start().unwrap());
        assert!(t_start > t_first);
        assert_eq!(preq.started(), Some(t_start));
    }

    #[test]
    fn forward_counts_body_as_out_and_pipelined_as_in() {
        let body: [&[u8]; 2] = [b"abc", b"de"];

        let acct = PipeAcct::default();
        let mut w = flaky(100);
        forward(&mut w, &body, || Some(&b"0123"[..]), &acct).unwrap();
        assert_eq!(w.written, b"abcde0123");
        assert_eq!((acct.out_bytes(), acct.in_bytes()), (5, 4));

        // Fails in the pipelined bytes: the sent prefix is "in".
        let acct = PipeAcct::default();
        let mut w = flaky(7);
        assert!(forward(&mut w, &body, || Some(&b"0123"[..]), &acct).is_err());
        assert_eq!((acct.out_bytes(), acct.in_bytes()), (5, 2));

        // Fails in the body: the sent prefix is "out", and the pipelined bytes are not taken.
        let acct = PipeAcct::default();
        let mut w = flaky(4);
        let res = forward(&mut w, &body, || panic!("taken after a failed body"), &acct);
        assert!(res.is_err());
        assert_eq!((acct.out_bytes(), acct.in_bytes()), (4, 0));
    }
}
