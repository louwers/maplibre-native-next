// Ports src/mln/storage/http_file_source.hpp and
// platform/default/src/mln/storage/http_file_source.cpp (curl -> ureq).
//
// C++ drives a curl multi handle from the requesting thread's RunLoop (socket
// watches and a timer). ureq is blocking, so each request runs on one of the
// file source's worker threads, and the result goes back to the requesting
// thread through a mailbox, where `handleResult` runs the callback as in
// C++. Dropping the request cancels it: the mailbox closes, so the callback
// never runs (an in-flight transfer finishes on its worker and is dropped).
//
// Request headers as in C++: `Range` for data ranges, `If-None-Match` from
// the prior etag, else `If-Modified-Since` from the prior modification date,
// user agent "MapLibreNative/1.0", redirects followed. Response headers are
// read in order with curl's header callback rules (case-insensitive names;
// `Last-Modified`/`Expires` through vendor parsedate, `Cache-Control`,
// `ETag`, `Retry-After`, `X-Rate-Limit-Reset`). Status codes map to the same
// Response fields and errors.
//
// Under Emscripten the transfer is an `emscripten_fetch` on the same worker
// threads, which wait for it by suspending through JSPI, so the browser does
// the HTTP: CORS, its cache, compression. It sends no `If-None-Match` /
// `If-Modified-Since` (they would force a CORS preflight; the browser cache
// revalidates instead) and can't set the user agent. See `transport`.
//
// Differences (DEVIATIONS.md [glfw]): ureq sends `Accept-Encoding: gzip` and
// decodes gzip (curl: "gzip, deflate"); a connection failure's message is
// ureq's text instead of curl's `curl_easy_strerror: errorbuffer`; dates use
// the vendored parsedate instead of libcurl's `curl_getdate` (they differ
// only after 2037).

use crate::async_request::AsyncRequest;
use crate::file_source::{Callback, FileSource};
use crate::resource::{Kind, Resource};
use crate::resource_options::ResourceOptions;
use crate::response::{Error, Reason, Response};
use mln_actor::{ActorRef, Mailbox, scheduler};
use mln_util::chrono::{Timestamp, parse_timestamp_str};
use mln_util::client_options::ClientOptions;
use mln_util::http_header::{CacheControl, parse_retry_headers};
use mln_util::string::to_string_i64;
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

/// The part of `HTTPRequest` that lives on the requesting thread.
pub struct HTTPRequestState {
    callback: RefCell<Callback>,
}

impl HTTPRequestState {
    /// The callback half of `handleResult(CURLcode)`.
    fn handle_result(&self, response: Response) {
        // Calling `callback` may result in deleting `this`; the mailbox keeps
        // the state alive while this message runs.
        (self.callback.borrow_mut())(response);
    }
}

/// `HTTPRequest`
struct HTTPRequest {
    mailbox: Arc<Mailbox>,
    cancelled: Arc<AtomicBool>,
}

impl AsyncRequest for HTTPRequest {}

impl Drop for HTTPRequest {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.mailbox.close();
    }
}

struct Job {
    resource: Resource,
    reply: ActorRef<HTTPRequestState>,
    cancelled: Arc<AtomicBool>,
}

/// `HTTPFileSource::Impl`
struct Impl {
    jobs: Mutex<mpsc::Sender<Job>>,
    resource_options: Mutex<ResourceOptions>,
    client_options: Mutex<ClientOptions>,
}

/// `headerMatches(header, buffer, length)`: ASCII-case-insensitive prefix.
fn header_value<'a>(name: &str, header: &str, line: &'a str) -> Option<&'a str> {
    (name.eq_ignore_ascii_case(header)).then_some(line)
}

/// `curl_getdate` (vendor parsedate, -1 when unparsable).
fn getdate(value: &str) -> Timestamp {
    parse_timestamp_str(value)
}

/// What a transport hands back: the status code, the response headers in
/// order, and the body, which only a 200 or 206 reads.
struct Transfer {
    status: i64,
    headers: Vec<(String, String)>,
    body: Result<Vec<u8>, Box<Error>>,
}

/// The worker half of `HTTPRequest`: performs the transfer and builds the
/// response as the header callback and `handleResult(CURLcode)` do.
fn perform(transport: &transport::Transport, resource: &Resource) -> Response {
    // Make sure a response object exists in case we haven't got any headers or content.
    let mut response = Response::new();
    let transfer = match transport.transfer(resource) {
        Ok(transfer) => transfer,
        Err(error) => {
            response.error = Some(error);
            return response;
        }
    };

    let mut retry_after: Option<String> = None;
    let mut x_rate_limit_reset: Option<String> = None;
    for (name, value) in &transfer.headers {
        let (name, value) = (name.as_str(), value.as_str());
        if let Some(value) = header_value(name, "last-modified", value) {
            // Always overwrite the modification date; We might already have
            // a value here from the Date header, but this one is more accurate.
            response.modified = Some(getdate(value));
        } else if let Some(value) = header_value(name, "etag", value) {
            response.etag = Some(value.to_owned());
        } else if let Some(value) = header_value(name, "cache-control", value) {
            let cc = CacheControl::parse(value);
            response.expires = cc.to_time_point();
            response.must_revalidate = cc.must_revalidate;
        } else if let Some(value) = header_value(name, "expires", value) {
            response.expires = Some(getdate(value));
        } else if let Some(value) = header_value(name, "retry-after", value) {
            retry_after = Some(value.to_owned());
        } else if let Some(value) = header_value(name, "x-rate-limit-reset", value) {
            x_rate_limit_reset = Some(value.to_owned());
        }
    }

    let response_code = transfer.status;
    if response_code == 200 || response_code == 206 {
        match transfer.body {
            Ok(data) => response.data = Some(Arc::new(data)),
            // curl reports a failed transfer as an error code, not a status.
            Err(error) => response.error = Some(error),
        }
    } else if response_code == 204 || (response_code == 404 && resource.kind == Kind::Tile) {
        response.no_content = true;
    } else if response_code == 304 {
        response.not_modified = true;
    } else if response_code == 404 {
        response.error = Some(Box::new(Error::new(Reason::NotFound, "HTTP status code 404")));
    } else if response_code == 429 {
        response.error = Some(Box::new(Error::with_retry_after(
            Reason::RateLimit,
            "HTTP status code 429",
            parse_retry_headers(&retry_after, &x_rate_limit_reset),
        )));
    } else if (500..600).contains(&response_code) {
        response.error =
            Some(Box::new(Error::new(Reason::Server, format!("HTTP status code {}", to_string_i64(response_code)))));
    } else {
        response.error =
            Some(Box::new(Error::new(Reason::Other, format!("HTTP status code {}", to_string_i64(response_code)))));
    }
    response
}

/// The `Range` request header for a data range.
fn range_header(resource: &Resource) -> Option<String> {
    resource.data_range.map(|(first, second)| format!("bytes={first}-{second}"))
}

/// curl through ureq: blocking transfers on the worker threads.
///
/// ureq connects to a host's addresses one after the other and waits for
/// each, so a host whose first (IPv6) address drops packets stalls every new
/// connection until the OS gives up (75 s on macOS). curl races them (Happy
/// Eyeballs), so the agent's TCP step does too: see `HappyEyeballsConnector`.
#[cfg(not(target_os = "emscripten"))]
mod transport {
    use super::{Transfer, range_header};
    use crate::resource::Resource;
    use crate::response::{Error, Reason};
    use mln_util::chrono::rfc1123;
    use std::net::SocketAddr;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;
    use ureq::unversioned::resolver::{DefaultResolver, Resolver};
    use ureq::unversioned::transport::{
        ConnectProxyConnector, ConnectionDetails, Connector, Either, RustlsConnector, TcpConnector,
        Transport as UreqTransport,
    };

    /// The worker threads of a file source: as many as the online file
    /// source's default maximum of concurrent requests.
    pub const WORKERS: u32 = mln_util::constants::DEFAULT_MAXIMUM_CONCURRENT_REQUESTS;

    /// curl's default `CURLOPT_HAPPY_EYEBALLS_TIMEOUT_MS`.
    const HAPPY_EYEBALLS_DELAY: Duration = Duration::from_millis(200);

    /// The TCP step of ureq's default connector chain with Happy Eyeballs
    /// (RFC 8305): the addresses alternate between the families, starting
    /// with the resolver's first, and each attempt gets a head start of
    /// `HAPPY_EYEBALLS_DELAY` (or until it fails) before the next one starts
    /// alongside it. The first connection wins; the others are closed when
    /// they finish. Each attempt is `per_address` (ureq's own TcpConnector,
    /// or a test's) on one address, on its own thread.
    #[derive(Debug)]
    struct HappyEyeballsConnector<A> {
        per_address: Arc<A>,
    }

    impl<In: UreqTransport, A: Connector> Connector<In> for HappyEyeballsConnector<A> {
        type Out = Either<In, Box<dyn UreqTransport>>;

        fn connect(&self, details: &ConnectionDetails, chained: Option<In>) -> Result<Option<Self::Out>, ureq::Error> {
            if let Some(chained) = chained {
                // A CONNECT proxy's tunnel.
                return Ok(Some(Either::A(chained)));
            }
            if details.addrs.len() < 2 {
                // Nothing to race.
                let transport = self.per_address.connect(details, None)?;
                return Ok(transport.map(|transport| Either::B(Box::new(transport) as Box<dyn UreqTransport>)));
            }

            let (first, second): (Vec<SocketAddr>, Vec<SocketAddr>) =
                details.addrs.iter().partition(|addr| addr.is_ipv6() == details.addrs[0].is_ipv6());
            let addrs: Vec<SocketAddr> = (0..first.len().max(second.len()))
                .flat_map(|i| first.get(i).into_iter().chain(second.get(i)).copied())
                .collect();

            let (sender, receiver) = mpsc::channel();
            let mut started = 0;
            let mut running = 0;
            let mut last_error = None;
            loop {
                if started < addrs.len() {
                    start_attempt(&self.per_address, details, addrs[started], sender.clone())?;
                    started += 1;
                    running += 1;
                }
                let result = if started < addrs.len() {
                    match receiver.recv_timeout(HAPPY_EYEBALLS_DELAY) {
                        Ok(result) => result,
                        Err(_) => continue, // the next attempt's turn
                    }
                } else if running > 0 {
                    // `sender` is still here, so this can't disconnect.
                    receiver.recv().expect("happy eyeballs channel")
                } else {
                    return Err(last_error.unwrap_or(ureq::Error::ConnectionFailed));
                };
                running -= 1;
                match result {
                    Ok(transport) => return Ok(Some(Either::B(transport))),
                    Err(error) => last_error = Some(error),
                }
            }
        }
    }

    /// One connection attempt of `HappyEyeballsConnector`: `per_address` on
    /// `addr` with the rest of `details`, on a thread that sends the result
    /// to `sender`.
    fn start_attempt<A: Connector>(
        per_address: &Arc<A>,
        details: &ConnectionDetails,
        addr: SocketAddr,
        sender: mpsc::Sender<Result<Box<dyn UreqTransport>, ureq::Error>>,
    ) -> Result<(), ureq::Error> {
        let uri = details.uri.clone();
        let config = details.config.clone();
        let request_level = details.request_level;
        let now = details.now;
        let timeout = details.timeout;
        let current_time = details.current_time.clone();
        let run_connector = details.run_connector.clone();
        let register_shutdown = details.register_shutdown.clone();
        let per_address = per_address.clone();
        std::thread::Builder::new().name("HTTPFileSource connect".to_owned()).spawn(move || {
            // TcpConnector doesn't resolve; a resolver is only required.
            let resolver = DefaultResolver::default();
            let mut addrs = resolver.empty();
            addrs.push(addr);
            let details = ConnectionDetails {
                uri: &uri,
                addrs,
                config: &config,
                request_level,
                resolver: &resolver,
                now,
                timeout,
                current_time,
                run_connector,
                register_shutdown,
            };
            let result = per_address
                .connect(&details, None)
                .and_then(|transport| transport.ok_or(ureq::Error::ConnectionFailed))
                .map(|transport| Box::new(transport) as Box<dyn UreqTransport>);
            // After another attempt won, nobody listens: the
            // connection closes here.
            let _ = sender.send(result);
        })?;
        Ok(())
    }

    #[derive(Clone)]
    pub struct Transport {
        agent: ureq::Agent,
    }

    /// The curl error codes `handleResult` maps to `Reason::Connection`.
    fn is_connection_error(error: &ureq::Error) -> bool {
        matches!(
            error,
            ureq::Error::HostNotFound | ureq::Error::ConnectionFailed | ureq::Error::Timeout(_) | ureq::Error::Io(_)
        )
    }

    fn error(error: ureq::Error) -> Box<Error> {
        // Add human-readable error code
        let reason = if is_connection_error(&error) { Reason::Connection } else { Reason::Other };
        Box::new(Error::new(reason, error.to_string()))
    }

    impl Transport {
        pub fn new() -> Self {
            Self::with_parts(TcpConnector::default(), DefaultResolver::default())
        }

        /// `new` with another per-address connector and resolver (tests).
        fn with_parts(per_address: impl Connector, resolver: impl Resolver) -> Self {
            let config =
                ureq::Agent::config_builder().http_status_as_error(false).user_agent("MapLibreNative/1.0").build();
            // ureq's DefaultConnector with HappyEyeballsConnector for its
            // TcpConnector (and without the SOCKS and native-tls steps,
            // whose features are off).
            let connector =
                ().chain(ConnectProxyConnector::default())
                    .chain(HappyEyeballsConnector { per_address: Arc::new(per_address) })
                    .chain(RustlsConnector::default());
            let agent = ureq::Agent::with_parts(config, connector, resolver);
            Transport { agent }
        }

        pub fn transfer(&self, resource: &Resource) -> Result<Transfer, Box<Error>> {
            let mut request = self.agent.get(&resource.url);
            if let Some(range) = range_header(resource) {
                request = request.header("Range", range);
            }

            // If there's already a response, set the correct etags/modified headers to
            // make sure we are getting a 304 response if possible. This avoids
            // redownloading unchanged data.
            if let Some(prior_etag) = &resource.prior_etag {
                request = request.header("If-None-Match", prior_etag.as_str());
            } else if let Some(prior_modified) = resource.prior_modified {
                request = request.header("If-Modified-Since", rfc1123(prior_modified));
            }

            let mut http_response = request.call().map_err(error)?;
            let headers = http_response
                .headers()
                .iter()
                .filter_map(|(name, value)| Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned())))
                .collect();
            let status = i64::from(http_response.status().as_u16());
            let body = if status == 200 || status == 206 {
                http_response.body_mut().with_config().limit(u64::MAX).read_to_vec().map_err(error)
            } else {
                Ok(Vec::new())
            };
            Ok(Transfer { status, headers, body })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::Mutex;
        use std::time::Instant;
        use ureq::config::Config;
        use ureq::http::Uri;
        use ureq::unversioned::resolver::ResolvedSocketAddrs;
        use ureq::unversioned::transport::NextTimeout;

        /// Resolves every host to `addrs`, in order.
        #[derive(Debug)]
        struct FixedResolver(Vec<SocketAddr>);

        impl Resolver for FixedResolver {
            fn resolve(
                &self,
                _uri: &Uri,
                _config: &Config,
                _timeout: NextTimeout,
            ) -> Result<ResolvedSocketAddrs, ureq::Error> {
                let mut addrs = self.empty();
                for addr in &self.0 {
                    addrs.push(*addr);
                }
                Ok(addrs)
            }
        }

        /// TcpConnector, except that a connect to one of `dropped` hangs as
        /// for a host that drops the SYN. Records the addresses tried.
        #[derive(Debug)]
        struct DroppingConnector {
            dropped: Vec<SocketAddr>,
            attempts: Arc<Mutex<Vec<SocketAddr>>>,
        }

        impl Connector for DroppingConnector {
            type Out = Box<dyn UreqTransport>;

            fn connect(
                &self,
                details: &ConnectionDetails,
                _chained: Option<()>,
            ) -> Result<Option<Self::Out>, ureq::Error> {
                let addr = details.addrs[0];
                self.attempts.lock().unwrap().push(addr);
                if self.dropped.contains(&addr) {
                    std::thread::sleep(Duration::from_secs(60));
                    return Err(ureq::Error::ConnectionFailed);
                }
                let transport = TcpConnector::default().connect(details, None::<()>)?;
                Ok(transport.map(|transport| Box::new(transport) as Box<dyn UreqTransport>))
            }
        }

        /// mapeak.com resolved to an IPv6 address that drops packets, then
        /// an IPv4 one: each new connection waited for the OS connect
        /// timeout (75 s on macOS) before trying the IPv4 address.
        #[test]
        fn falls_back_from_an_address_that_drops_connects() {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let server = listener.local_addr().unwrap();
            std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.ends_with(b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).unwrap();
                    if read == 0 {
                        return;
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").unwrap();
            });

            let dropped: Vec<SocketAddr> =
                vec!["[2001:db8::1]:80".parse().unwrap(), "[2001:db8::2]:80".parse().unwrap()];
            let attempts = Arc::new(Mutex::new(Vec::new()));
            let transport = Transport::with_parts(
                DroppingConnector { dropped: dropped.clone(), attempts: attempts.clone() },
                FixedResolver(vec![dropped[0], dropped[1], server]),
            );

            let started = Instant::now();
            let transfer = transport.transfer(&Resource::style("http://mapeak.test/style.json")).unwrap();
            assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());
            assert_eq!(200, transfer.status);
            assert_eq!(b"ok".to_vec(), transfer.body.unwrap());
            // The families alternate: the IPv4 address goes second, after
            // HAPPY_EYEBALLS_DELAY.
            assert_eq!([dropped[0], server], attempts.lock().unwrap()[..2]);
        }
    }
}

/// The browser's fetch through Emscripten (`-sFETCH=1`, `-sJSPI`). The worker
/// thread waits for the transfer as it waits for a ureq one, but by
/// suspending in `emscripten_sleep`, which lets its event loop deliver the
/// response. (A synchronous XMLHttpRequest from these threads intermittently
/// fails with a NetworkError in Chromium.)
///
/// Chromium also aborts an occasional request from these threads right after
/// it starts (net::ERR_ABORTED), which emscripten_fetch reports through no
/// callback, only through `readyState` reaching DONE. Such a request is tried
/// again a few times before it fails with a connection error.
#[cfg(target_os = "emscripten")]
mod transport {
    use super::{Transfer, range_header};
    use crate::resource::Resource;
    use crate::response::{Error, Reason};
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::ptr::null;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Browsers run a handful of requests per host at a time, and every
    /// worker is a pthread, so fewer than natively.
    pub const WORKERS: u32 = 6;

    const EMSCRIPTEN_FETCH_LOAD_TO_MEMORY: u32 = 1;
    const EMSCRIPTEN_FETCH_REPLACE: u32 = 16;

    /// `emscripten_fetch_attr_t` (emscripten/fetch.h).
    #[repr(C)]
    struct FetchAttr {
        request_method: [c_char; 32],
        user_data: *mut c_void,
        onsuccess: Option<unsafe extern "C" fn(*mut Fetch)>,
        onerror: Option<unsafe extern "C" fn(*mut Fetch)>,
        onprogress: Option<unsafe extern "C" fn(*mut Fetch)>,
        onreadystatechange: Option<unsafe extern "C" fn(*mut Fetch)>,
        attributes: u32,
        timeout_msecs: u32,
        with_credentials: bool,
        destination_path: *const c_char,
        user_name: *const c_char,
        password: *const c_char,
        request_headers: *const *const c_char,
        overridden_mime_type: *const c_char,
        request_data: *const c_char,
        request_data_size: usize,
    }

    /// `emscripten_fetch_t` (emscripten/fetch.h).
    #[repr(C)]
    struct Fetch {
        id: u32,
        user_data: *mut c_void,
        url: *const c_char,
        data: *const c_char,
        num_bytes: u64,
        data_offset: u64,
        total_bytes: u64,
        ready_state: u16,
        status: u16,
        status_text: [c_char; 64],
        attributes: FetchAttr,
        response_url: *const c_char,
    }

    unsafe extern "C" {
        fn emscripten_fetch_attr_init(attr: *mut FetchAttr);
        fn emscripten_fetch(attr: *mut FetchAttr, url: *const c_char) -> *mut Fetch;
        fn emscripten_fetch_close(fetch: *mut Fetch) -> i32;
        fn emscripten_fetch_get_response_headers_length(fetch: *mut Fetch) -> usize;
        fn emscripten_fetch_get_response_headers(fetch: *mut Fetch, dst: *mut c_char, size: usize) -> usize;
        fn emscripten_sleep(ms: u32);
    }

    /// The fetch's `onsuccess` and `onerror`: they run on the requesting
    /// thread, from its event loop, while it sleeps.
    unsafe extern "C" fn finished(fetch: *mut Fetch) {
        // SAFETY: user_data is the requesting thread's flag, which outlives
        // the fetch.
        unsafe { (*(*fetch).user_data.cast::<AtomicBool>()).store(true, Ordering::SeqCst) };
    }

    /// `XMLHttpRequest.DONE`.
    const READY_STATE_DONE: u16 = 4;
    const ATTEMPTS: u32 = 4;

    #[derive(Clone)]
    pub struct Transport;

    /// `Name: value` lines of the browser's raw header block, in order.
    fn parse_headers(raw: &str) -> Vec<(String, String)> {
        raw.split("\r\n")
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                Some((name.trim().to_owned(), value.trim().to_owned()))
            })
            .collect()
    }

    impl Transport {
        pub fn new() -> Self {
            Transport
        }

        pub fn transfer(&self, resource: &Resource) -> Result<Transfer, Box<Error>> {
            for attempt in 1..ATTEMPTS {
                match self.attempt(resource) {
                    Some(result) => return result,
                    // SAFETY: sleeping suspends this thread through JSPI.
                    None => unsafe { emscripten_sleep(50 * attempt) },
                }
            }
            self.attempt(resource).unwrap_or_else(|| {
                Err(Box::new(Error::new(
                    Reason::Connection,
                    format!("the browser aborted the request for {}", resource.url),
                )))
            })
        }

        /// One fetch: `None` when the browser aborted it before it finished.
        fn attempt(&self, resource: &Resource) -> Option<Result<Transfer, Box<Error>>> {
            let connection = |message: String| Box::new(Error::new(Reason::Connection, message));
            let url = match CString::new(resource.url.as_str()) {
                Ok(url) => url,
                Err(_) => return Some(Err(connection("URL contains a NUL byte".into()))),
            };
            let range = range_header(resource).map(|range| CString::new(range).unwrap());
            let range_name = c"Range";
            let headers: Vec<*const c_char> = match &range {
                Some(range) => vec![range_name.as_ptr(), range.as_ptr(), null()],
                None => vec![null()],
            };
            let done = AtomicBool::new(false);
            // SAFETY: emscripten_fetch_attr_init initializes every field; the
            // strings, the header list and `done` outlive the fetch, which
            // finishes before this returns.
            unsafe {
                let mut attr = std::mem::MaybeUninit::<FetchAttr>::uninit();
                emscripten_fetch_attr_init(attr.as_mut_ptr());
                let mut attr = attr.assume_init();
                for (slot, byte) in attr.request_method.iter_mut().zip(b"GET\0") {
                    *slot = *byte as c_char;
                }
                // REPLACE: straight to the network, never to emscripten_fetch's
                // IndexedDB cache.
                attr.attributes = EMSCRIPTEN_FETCH_LOAD_TO_MEMORY | EMSCRIPTEN_FETCH_REPLACE;
                attr.request_headers = headers.as_ptr();
                attr.user_data = (&raw const done).cast_mut().cast();
                attr.onsuccess = Some(finished);
                attr.onerror = Some(finished);
                let fetch = emscripten_fetch(&mut attr, url.as_ptr());
                if fetch.is_null() {
                    return Some(Err(connection(format!("emscripten_fetch failed for {}", resource.url))));
                }
                while !done.load(Ordering::SeqCst) {
                    if (*fetch).ready_state == READY_STATE_DONE && (*fetch).status == 0 {
                        // Aborted: give a late callback one more turn, then
                        // stop waiting.
                        emscripten_sleep(1);
                        if !done.load(Ordering::SeqCst) {
                            emscripten_fetch_close(fetch);
                            return None;
                        }
                    }
                    emscripten_sleep(1);
                }
                let status = i64::from((*fetch).status);
                if status == 0 {
                    // A network error or a CORS rejection: the browser reports
                    // no status.
                    let text = CStr::from_ptr((*fetch).status_text.as_ptr()).to_string_lossy().into_owned();
                    emscripten_fetch_close(fetch);
                    let message = if text.is_empty() { "network error".to_owned() } else { text };
                    return Some(Err(connection(message)));
                }
                let length = emscripten_fetch_get_response_headers_length(fetch);
                let mut raw = vec![0u8; length + 1];
                emscripten_fetch_get_response_headers(fetch, raw.as_mut_ptr().cast(), raw.len());
                raw.truncate(length);
                let headers = parse_headers(&String::from_utf8_lossy(&raw));
                let body = if status == 200 || status == 206 {
                    let data = (*fetch).data.cast::<u8>();
                    let size = (*fetch).num_bytes as usize;
                    Ok(if data.is_null() { Vec::new() } else { std::slice::from_raw_parts(data, size).to_vec() })
                } else {
                    Ok(Vec::new())
                };
                emscripten_fetch_close(fetch);
                Some(Ok(Transfer { status, headers, body }))
            }
        }
    }
}

fn worker(transport: transport::Transport, jobs: Arc<Mutex<mpsc::Receiver<Job>>>) {
    loop {
        let job = match jobs.lock() {
            Ok(receiver) => receiver.recv(),
            Err(_) => return,
        };
        let Ok(job) = job else {
            return; // the file source is gone
        };
        if job.cancelled.load(Ordering::SeqCst) {
            continue;
        }
        let response = perform(&transport, &job.resource);
        if !job.cancelled.load(Ordering::SeqCst) {
            job.reply.invoke(move |request: &HTTPRequestState| request.handle_result(response));
        }
    }
}

/// `HTTPFileSource`: plain HTTP(S) requests, used by the OnlineFileSource.
pub struct HTTPFileSource {
    impl_: Impl,
}

impl HTTPFileSource {
    /// `HTTPFileSource(const ResourceOptions&, const ClientOptions&)`: starts
    /// the transfer threads (`transport::WORKERS`).
    pub fn new(resource_options: &ResourceOptions, client_options: &ClientOptions) -> Self {
        let transport = transport::Transport::new();
        let (sender, receiver) = mpsc::channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        for i in 0..transport::WORKERS {
            let transport = transport.clone();
            let receiver = receiver.clone();
            // Detached: a worker exits when the channel closes, after its
            // current transfer.
            let _ = std::thread::Builder::new()
                .name(format!("HTTPFileSource {i}"))
                .spawn(move || worker(transport, receiver));
        }
        HTTPFileSource {
            impl_: Impl {
                jobs: Mutex::new(sender),
                resource_options: Mutex::new(resource_options.clone()),
                client_options: Mutex::new(client_options.clone()),
            },
        }
    }

    /// `request(const Resource&, Callback)`. The callback runs on the
    /// calling thread's RunLoop.
    pub fn request(&self, resource: &Resource, callback: Callback) -> Box<dyn AsyncRequest> {
        let state = Rc::new(HTTPRequestState { callback: RefCell::new(callback) });
        let mailbox = Mailbox::with_target(&scheduler::get_current(), state as Rc<dyn Any>);
        let cancelled = Arc::new(AtomicBool::new(false));
        let job = Job { resource: resource.clone(), reply: ActorRef::new(&mailbox), cancelled: cancelled.clone() };
        let _ = self.impl_.jobs.lock().unwrap().send(job);
        Box::new(HTTPRequest { mailbox, cancelled })
    }
}

impl FileSource for HTTPFileSource {
    fn request(&self, resource: &Resource, callback: Callback) -> Box<dyn AsyncRequest> {
        HTTPFileSource::request(self, resource, callback)
    }

    fn can_request(&self, _resource: &Resource) -> bool {
        true
    }

    fn set_resource_options(&self, options: ResourceOptions) {
        *self.impl_.resource_options.lock().unwrap() = options;
    }

    fn get_resource_options(&self) -> ResourceOptions {
        self.impl_.resource_options.lock().unwrap().clone()
    }

    fn set_client_options(&self, options: ClientOptions) {
        *self.impl_.client_options.lock().unwrap() = options;
    }

    fn get_client_options(&self) -> ClientOptions {
        self.impl_.client_options.lock().unwrap().clone()
    }
}
