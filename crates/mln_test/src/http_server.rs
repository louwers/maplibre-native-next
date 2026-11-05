// Ports test/src/mln/test/http_server.cpp (`mln::test::HttpServer`, the
// cpp-httplib server the C++ unit tests start on 127.0.0.1:3000) as an
// in-process server on std's TcpListener.
//
// Differences from C++: every test starts its own server on a free port
// (`HttpServer::new()`, URLs from `url(path)`) instead of one per process on
// port 3000, so the per-route state (`/cache` counter, `/temporary-error`,
// `/style-fail-once-*`, ...) starts fresh for each test, as when a C++ test
// runs alone, and tests in parallel (one Rust test binary, several Bazel
// test targets) never share a server. The JSON fixtures under
// test/fixtures/map/online name their resources as
// `http://127.0.0.1:3000/online/...`; `/online/` serves them with that
// origin replaced by the server's own (`rewrite_origin`), so the style's
// sources and sprite load from this server too. HTTP/1.1 keep-alive is
// supported, as in httplib. A route that sets no status answers 200
// (httplib's `res.status == -1` default), and a `Range: bytes=a-b` request
// header on a 200 response with content answers 206 with that slice
// (httplib's range handling); an unknown path is a 404 with an empty body.

use crate::cpp_path;
use mln_util::chrono::{now, parse_timestamp_i32, parse_timestamp_str, rfc1123};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::thread::JoinHandle;

/// The parts of `httplib::Request` the routes use.
struct Request {
    path: String,
    params: BTreeMap<String, String>,
    /// Lower-case header names.
    headers: BTreeMap<String, String>,
}

impl Request {
    fn has_param(&self, name: &str) -> bool {
        self.params.contains_key(name)
    }

    fn get_param_value(&self, name: &str) -> &str {
        self.params.get(name).map(String::as_str).unwrap_or("")
    }

    fn has_header(&self, name: &str) -> bool {
        self.headers.contains_key(name)
    }

    fn get_header_value(&self, name: &str) -> &str {
        self.headers.get(name).map(String::as_str).unwrap_or("")
    }
}

/// `httplib::Response`: `status == -1` until a route sets one.
struct Response {
    status: i32,
    headers: Vec<(String, String)>,
    content: Option<Vec<u8>>,
}

impl Response {
    fn set_header(&mut self, name: &str, value: impl Into<String>) {
        self.headers.push((name.to_owned(), value.into()));
    }

    fn set_content(&mut self, content: impl Into<Vec<u8>>) {
        self.content = Some(content.into());
    }
}

/// The state the C++ routes keep in `runServer`'s locals.
#[derive(Default)]
struct State {
    cache_counter: AtomicI32,
    expired_once_done: AtomicBool,
    revalidate_etag_counter: AtomicI32,
    temporary_error_counter: AtomicI32,
    style_fail_once_500_done: AtomicBool,
    style_fail_once_404_done: AtomicBool,
    style_fail_once_404_cache_done: AtomicBool,
}

/// The origin the C++ fixtures and tests use for the test server.
pub const CPP_ORIGIN: &str = "http://127.0.0.1:3000";

/// `runServer`'s routes.
fn route(state: &State, origin: &str, req: &Request, res: &mut Response) -> bool {
    let path = req.path.as_str();
    match path {
        "/test" => {
            let mut content = b"Hello World!".to_vec();
            if req.has_param("modified") {
                let modified: i32 = req.get_param_value("modified").parse().unwrap_or(0);
                res.set_header("Last-Modified", rfc1123(parse_timestamp_i32(modified)));
            }
            if req.has_param("expires") {
                let expires: i32 = req.get_param_value("expires").parse().unwrap_or(0);
                res.set_header("Expires", rfc1123(parse_timestamp_i32(expires)));
            }
            if req.has_param("etag") {
                res.set_header("ETag", req.get_param_value("etag"));
            }
            if req.has_param("cachecontrol") {
                res.set_header("Cache-Control", format!("max-age={}", req.get_param_value("cachecontrol")));
            }
            if req.has_param("range") {
                let range = req.get_param_value("range");
                let range = &range["bytes=".len().min(range.len())..];
                let (start, end) = range.split_once('-').unwrap_or((range, ""));
                let start: usize = start.parse().unwrap_or(0);
                let end: usize = end.parse().unwrap_or(0);
                content = content[start.min(content.len())..(end + 1).min(content.len())].to_vec();
                res.status = 206;
            }
            res.set_content(content);
        }
        "/stale" => {
            // Never respond.
        }
        "/cache" => {
            res.set_header("Cache-Control", "max-age=30"); // Allow caching for 30 seconds
            let n = state.cache_counter.fetch_add(1, Ordering::SeqCst) + 1;
            res.set_content(format!("Response {n}"));
        }
        "/revalidate-same" => {
            if req.get_header_value("if-none-match") == "snowfall" {
                // Second request can be cached for 1 second.
                res.set_header("Cache-Control", "max-age=1, must-revalidate");
                res.status = 304;
            } else {
                // First request must always be revalidated.
                res.set_header("ETag", "snowfall");
                res.set_header("Cache-Control", "must-revalidate");
                res.status = 200;
                res.set_content("Response");
            }
        }
        "/clockskew" => {
            let date_str = if !state.expired_once_done.swap(true, Ordering::SeqCst) {
                "jan 1 10:00:00 2010 utc"
            } else {
                "jan 1 10:01:00 2010 utc"
            };
            res.set_header("Expires", rfc1123(parse_timestamp_str(date_str)));
            res.status = 200;
            res.set_content("Response");
        }
        "/revalidate-modified" => {
            let jan1 = parse_timestamp_str("jan 1 2015 utc");

            if req.has_header("if-modified-since") {
                let modified_since = parse_timestamp_str(req.get_header_value("if-modified-since"));
                if modified_since >= jan1 {
                    res.set_header("Cache-Control", "max-age=1, must-revalidate");
                    res.status = 304;
                    return true;
                }
            }

            // First request must always be revalidated.
            res.set_header("Last-Modified", rfc1123(jan1));
            res.set_header("Cache-Control", "must-revalidate");
            res.status = 200;
            res.set_content("Response");
        }
        "/revalidate-etag" => {
            let n = state.revalidate_etag_counter.fetch_add(1, Ordering::SeqCst) + 1;
            res.set_header("ETag", format!("response-{n}"));
            res.set_header("Cache-Control", "must-revalidate");

            res.status = 200;
            res.set_content(format!("Response {n}"));
        }
        "/empty-data" => res.status = 200,
        "/no-content" => res.status = 204,
        "/not-found" => {
            res.status = 404;
            res.set_content("Not Found!");
        }
        "/permanent-error" => {
            res.status = 500;
            res.set_content("Server Error!");
        }
        "/temporary-error" => {
            if state.temporary_error_counter.fetch_add(1, Ordering::SeqCst) == 0 {
                res.status = 500;
            } else {
                res.status = 200;
                res.set_content("Hello World!");
            }
        }
        "/rate-limit" => {
            if req.has_param("std") {
                res.set_header("Retry-After", "1");
            } else if req.has_param("mbx") {
                res.set_header("x-rate-limit-reset", (now().secs_since_epoch() + 1).to_string());
            }
            res.status = 429;
        }
        "/style-fail-once-500" => {
            if !state.style_fail_once_500_done.swap(true, Ordering::SeqCst) {
                res.status = 500;
                res.set_content("Not found!");
            } else {
                res.status = 200;
                res.set_content(r#"{ "version": 8, "name": "Test Style" }"#);
            }
        }
        "/style-fail-once-404" => {
            if !state.style_fail_once_404_done.swap(true, Ordering::SeqCst) {
                res.status = 404;
                res.set_content("Not found!");
            } else {
                res.status = 200;
                res.set_content(r#"{ "version": 8, "name": "Test Style" }"#);
            }
        }
        "/style-fail-once-404-cache" => {
            if !state.style_fail_once_404_cache_done.swap(true, Ordering::SeqCst) {
                res.set_header("Cache-Control", "max-age=30");
                res.status = 404;
                res.set_content("Not found!");
            } else {
                res.status = 200;
                res.set_content(r#"{ "version": 8, "name": "Teste Style" }"#);
            }
        }
        "/delayed" => {
            std::thread::sleep(std::time::Duration::from_millis(200));
            res.status = 200;
            res.set_content("Response");
        }
        _ => {
            if let Some(numbers) = path.strip_prefix("/load/")
                && !numbers.is_empty()
                && numbers.bytes().all(|b| b.is_ascii_digit())
            {
                res.set_content(format!("Request {numbers}"));
            } else if let Some(file) = path.strip_prefix("/online/") {
                res.status = 200;
                // C++ reads "test/fixtures/map/online/<file>" relative to the
                // working directory (util::read_file throws when missing).
                let content = std::fs::read(cpp_path(&format!("test/fixtures/map/online/{file}"))).unwrap_or_default();
                res.set_content(if file.ends_with(".json") { rewrite_origin(&content, origin) } else { content });
            } else {
                return false;
            }
        }
    }
    true
}

/// `%XX` and `+` decoding of a query component (httplib `decode_url(s, true)`).
fn decode_query_component(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                    None => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_target(target: &str) -> (String, BTreeMap<String, String>) {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut params = BTreeMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        // httplib's parse_query_text splits on every '=': the first piece is
        // the key, the last one the value ("cachecontrol=max-age=120" ->
        // "120").
        let mut pieces = pair.split('=');
        let key = pieces.next().unwrap_or("");
        let value = pieces.next_back().unwrap_or("");
        params
            .entry(decode_query_component(key))
            .or_insert_with(|| decode_query_component(value));
    }
    (path.to_owned(), params)
}

fn reason_phrase(status: i32) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        206 => "Partial Content",
        304 => "Not Modified",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

/// `Range: bytes=a-b` (one range, both ends given or open-ended).
fn parse_range(value: &str, len: usize) -> Option<(usize, usize)> {
    let range = value.trim().strip_prefix("bytes=")?;
    let (start, end) = range.split_once('-')?;
    let start: usize = start.trim().parse().ok()?;
    let end: usize = if end.trim().is_empty() { len.checked_sub(1)? } else { end.trim().parse().ok()? };
    (start <= end && start < len).then_some((start, end.min(len - 1)))
}

/// `content` with every `http://127.0.0.1:3000` (the C++ server's origin)
/// replaced by `origin`.
pub fn rewrite_origin(content: &[u8], origin: &str) -> Vec<u8> {
    let from = CPP_ORIGIN.as_bytes();
    let mut out = Vec::with_capacity(content.len());
    let mut i = 0;
    while i < content.len() {
        if content[i..].starts_with(from) {
            out.extend_from_slice(origin.as_bytes());
            i += from.len();
        } else {
            out.push(content[i]);
            i += 1;
        }
    }
    out
}

fn serve(state: Arc<State>, origin: Arc<str>, stream: TcpStream) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut stream = stream;
    loop {
        let mut request_line = String::new();
        match reader.read_line(&mut request_line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let target = request_line.split_whitespace().nth(1).unwrap_or("/").to_owned();
        let mut headers = BTreeMap::new();
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
            }
        }
        let close = headers.get("connection").is_some_and(|c| c.eq_ignore_ascii_case("close"));

        let (path, params) = parse_target(&target);
        let req = Request { path, params, headers };
        let mut res = Response {
            status: -1,
            headers: Vec::new(),
            content: None,
        };
        let routed = route(&state, &origin, &req, &mut res);
        if !routed {
            res.status = 404;
        } else if res.status == -1 {
            res.status = 200;
        }

        let mut body = res.content.take().unwrap_or_default();
        if res.status == 200
            && !body.is_empty()
            && let Some(range) = req.headers.get("range")
            && let Some((start, end)) = parse_range(range, body.len())
        {
            res.set_header("Content-Range", format!("bytes {start}-{end}/{}", body.len()));
            body = body[start..=end].to_vec();
            res.status = 206;
        }

        let mut head = format!("HTTP/1.1 {} {}\r\n", res.status, reason_phrase(res.status));
        for (name, value) in &res.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        let no_body = res.status == 204 || res.status == 304;
        if no_body {
            body.clear();
        } else {
            head.push_str("Content-Type: text/plain\r\n");
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        if close {
            head.push_str("Connection: close\r\n");
        } else {
            head.push_str("Connection: Keep-Alive\r\n");
        }
        head.push_str("\r\n");
        let mut out = head.into_bytes();
        out.extend_from_slice(&body);
        if stream.write_all(&out).is_err() || stream.flush().is_err() || close {
            return;
        }
    }
}

/// `mln::test::HttpServer`: serves the routes until dropped.
pub struct HttpServer {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl HttpServer {
    pub fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let port = listener.local_addr().expect("local address").port();
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(State::default());
        let origin: Arc<str> = format!("http://127.0.0.1:{port}").into();
        let stopped = stop.clone();
        let thread = std::thread::Builder::new()
            .name("HttpServer".to_owned())
            .spawn(move || {
                for stream in listener.incoming() {
                    if stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    let Ok(stream) = stream else {
                        continue;
                    };
                    let (state, origin) = (state.clone(), origin.clone());
                    let _ = std::thread::Builder::new()
                        .name("HttpServer connection".to_owned())
                        .spawn(move || serve(state, origin, stream));
                }
            })
            .expect("spawn test server");
        HttpServer {
            port,
            stop,
            thread: Some(thread),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// `http://127.0.0.1:<port><path>` (C++: `http://127.0.0.1:3000<path>`).
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// `content` (a C++ fixture or URL naming `http://127.0.0.1:3000`) with
    /// that origin replaced by this server's.
    pub fn rewrite_origin(&self, content: &[u8]) -> Vec<u8> {
        rewrite_origin(content, &self.url(""))
    }
}

impl Default for HttpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        // `server->stop()`: wake the accept loop, which then returns.
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A URL on a port nothing listens on (C++ uses 127.0.0.1:3001).
pub fn closed_port_url(path: &str) -> String {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("local address").port()
    };
    format!("http://127.0.0.1:{port}{path}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn get(server: &HttpServer, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn online_json_fixtures_name_this_server() {
        let server = HttpServer::new();
        let style = get(&server, "/online/style.json");
        assert!(style.starts_with("HTTP/1.1 200 OK\r\n"), "{style}");
        assert!(style.contains(&format!("\"{}\"", server.url("/online/sprite"))), "{style}");
        assert!(!style.contains(CPP_ORIGIN), "{style}");
    }

    #[test]
    fn rewrite_origin_replaces_every_occurrence() {
        assert_eq!(
            b"[\"http://127.0.0.1:9/a\", \"http://127.0.0.1:9/b\"]".to_vec(),
            rewrite_origin(b"[\"http://127.0.0.1:3000/a\", \"http://127.0.0.1:3000/b\"]", "http://127.0.0.1:9")
        );
    }

    #[test]
    fn unknown_paths_are_not_found() {
        let server = HttpServer::new();
        assert!(get(&server, "/nothing-here").starts_with("HTTP/1.1 404 Not Found\r\n"));
    }
}
