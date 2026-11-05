// Ports test/storage/http_file_source.test.cpp against the in-process test
// server (//crates/mln_test http_server.rs) instead of the C++ one on port 3000.

use super::http_server::HttpServer;
use super::{Req, req};
use crate::file_source::FileSource;
use crate::http_file_source::HTTPFileSource;
use crate::resource::{Kind, Resource};
use crate::resource_options::ResourceOptions;
use crate::response::{Reason, Response};
use crate::ClientOptions;
use mln_actor::RunLoop;
use mln_util::chrono::{Timestamp, now};
use std::cell::Cell;
use std::rc::Rc;

fn http_file_source() -> HTTPFileSource {
    HTTPFileSource::new(&ResourceOptions::default_options(), &ClientOptions::default())
}

fn data(res: &Response) -> &[u8] {
    res.data.as_deref().map(Vec::as_slice).expect("data")
}

// gtest: HTTPFileSource.Cancel
#[test]
fn cancel() {
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let fs = http_file_source();

    drop(fs.request(
        &Resource::new(Kind::Unknown, server.url("/test")),
        Box::new(|_: Response| panic!("Callback should not be called")),
    ));

    run_loop.run_once();
}

// gtest: HTTPFileSource.HTTP200
#[test]
fn http200() {
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let fs = http_file_source();

    let stop = run_loop.clone();
    let _req = fs.request(
        &Resource::new(Kind::Unknown, server.url("/test")),
        Box::new(move |res: Response| {
            assert_eq!(None, res.error);
            assert_eq!(b"Hello World!", data(&res));
            assert!(res.expires.is_none());
            assert!(!res.must_revalidate);
            assert!(res.modified.is_none());
            assert!(res.etag.is_none());
            stop.stop();
        }),
    );

    run_loop.run();
}

// gtest: HTTPFileSource.HTTP206
#[test]
fn http206() {
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let fs = http_file_source();

    let mut resource = Resource::new(Kind::Unknown, server.url("/test"));
    resource.data_range = Some((3, 8));

    let stop = run_loop.clone();
    let _req = fs.request(
        &resource,
        Box::new(move |res: Response| {
            assert_eq!(None, res.error);
            assert_eq!(b"lo Wor", data(&res));
            assert!(res.expires.is_none());
            assert!(!res.must_revalidate);
            assert!(res.modified.is_none());
            assert!(res.etag.is_none());
            stop.stop();
        }),
    );

    run_loop.run();
}

// gtest: HTTPFileSource.HTTP404
#[test]
fn http404() {
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let fs = http_file_source();

    let stop = run_loop.clone();
    let _req = fs.request(
        &Resource::new(Kind::Unknown, server.url("/doesnotexist")),
        Box::new(move |res: Response| {
            let error = res.error.as_ref().expect("error");
            assert_eq!(Reason::NotFound, error.reason);
            assert_eq!("HTTP status code 404", error.message);
            assert!(res.data.is_none());
            assert!(res.expires.is_none());
            assert!(!res.must_revalidate);
            assert!(res.modified.is_none());
            assert!(res.etag.is_none());
            stop.stop();
        }),
    );

    run_loop.run();
}

// gtest: HTTPFileSource.HTTPTile404
#[test]
fn http_tile404() {
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let fs = http_file_source();

    let stop = run_loop.clone();
    let _req = fs.request(
        &Resource::new(Kind::Tile, server.url("/doesnotexist")),
        Box::new(move |res: Response| {
            assert!(res.no_content);
            assert!(res.error.is_none());
            assert!(res.data.is_none());
            assert!(res.expires.is_none());
            assert!(!res.must_revalidate);
            assert!(res.modified.is_none());
            assert!(res.etag.is_none());
            stop.stop();
        }),
    );

    run_loop.run();
}

// gtest: HTTPFileSource.HTTP200EmptyData
#[test]
fn http200_empty_data() {
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let fs = http_file_source();

    let stop = run_loop.clone();
    let _req = fs.request(
        &Resource::new(Kind::Unknown, server.url("/empty-data")),
        Box::new(move |res: Response| {
            assert!(!res.no_content);
            assert!(res.error.is_none());
            assert_eq!(b"", data(&res));
            assert!(res.expires.is_none());
            assert!(!res.must_revalidate);
            assert!(res.modified.is_none());
            assert!(res.etag.is_none());
            stop.stop();
        }),
    );

    run_loop.run();
}

// gtest: HTTPFileSource.HTTP204
#[test]
fn http204() {
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let fs = http_file_source();

    let stop = run_loop.clone();
    let _req = fs.request(
        &Resource::new(Kind::Unknown, server.url("/no-content")),
        Box::new(move |res: Response| {
            assert!(res.no_content);
            assert!(res.error.is_none());
            assert!(res.data.is_none());
            assert!(res.expires.is_none());
            assert!(!res.must_revalidate);
            assert!(res.modified.is_none());
            assert!(res.etag.is_none());
            stop.stop();
        }),
    );

    run_loop.run();
}

// gtest: HTTPFileSource.HTTP500
#[test]
fn http500() {
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let fs = http_file_source();

    let stop = run_loop.clone();
    let _req = fs.request(
        &Resource::new(Kind::Unknown, server.url("/permanent-error")),
        Box::new(move |res: Response| {
            let error = res.error.as_ref().expect("error");
            assert_eq!(Reason::Server, error.reason);
            assert_eq!("HTTP status code 500", error.message);
            assert!(res.data.is_none());
            assert!(res.expires.is_none());
            assert!(!res.must_revalidate);
            assert!(res.modified.is_none());
            assert!(res.etag.is_none());
            stop.stop();
        }),
    );

    run_loop.run();
}

// gtest: HTTPFileSource.ExpiresParsing
#[test]
fn expires_parsing() {
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let fs = http_file_source();

    let stop = run_loop.clone();
    let _req = fs.request(
        &Resource::new(
            Kind::Unknown,
            server.url("/test?modified=1420794326&expires=1420797926&etag=foo"),
        ),
        Box::new(move |res: Response| {
            assert_eq!(None, res.error);
            assert_eq!(b"Hello World!", data(&res));
            assert_eq!(Some(Timestamp::from_secs(1420797926)), res.expires);
            assert!(!res.must_revalidate);
            assert_eq!(Some(Timestamp::from_secs(1420794326)), res.modified);
            assert_eq!(Some("foo"), res.etag.as_deref());
            stop.stop();
        }),
    );

    run_loop.run();
}

// gtest: HTTPFileSource.CacheControlParsing
#[test]
fn cache_control_parsing() {
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let fs = http_file_source();

    let stop = run_loop.clone();
    let _req = fs.request(
        &Resource::new(Kind::Unknown, server.url("/test?cachecontrol=max-age=120")),
        Box::new(move |res: Response| {
            assert_eq!(None, res.error);
            assert_eq!(b"Hello World!", data(&res));
            let delta = res.expires.expect("expires").secs_since_epoch() - now().secs_since_epoch() - 120;
            assert!(delta.abs() < 2, "Expiration date isn't about 120 seconds in the future");
            assert!(!res.must_revalidate);
            assert!(res.modified.is_none());
            assert!(res.etag.is_none());
            stop.stop();
        }),
    );

    run_loop.run();
}

/// The shared state of the `Load` tests: `reqs[concurrency]` and `number`.
pub struct LoadState {
    pub reqs: Vec<Req>,
    pub number: Cell<i32>,
}

/// The `req(i)` lambda of the `Load` tests (also used by
/// OnlineFileSource.Load).
pub fn load_request(fs: &Rc<dyn FileSource>, state: &Rc<LoadState>, url: &Rc<dyn Fn(i32) -> String>, run_loop: &RunLoop, i: usize, max: i32) {
    let current = state.number.get();
    state.number.set(current + 1);
    let (fs2, state2, url2, run_loop2) = (fs.clone(), state.clone(), url.clone(), run_loop.clone());
    let request = fs.request(
        &Resource::new(Kind::Unknown, url(current)),
        Box::new(move |res: Response| {
            state2.reqs[i].borrow_mut().take();
            assert_eq!(None, res.error);
            assert_eq!(format!("Request {current}").as_bytes(), data(&res));
            assert!(res.expires.is_none());
            assert!(!res.must_revalidate);
            assert!(res.modified.is_none());
            assert!(res.etag.is_none());

            if state2.number.get() <= max {
                load_request(&fs2, &state2, &url2, &run_loop2, i, max);
            } else if current == max {
                run_loop2.stop();
            }
        }),
    );
    *state.reqs[i].borrow_mut() = Some(request);
}

// gtest: HTTPFileSource.Load
#[test]
fn load() {
    let server = HttpServer::new();
    let run_loop = RunLoop::new();
    let fs: Rc<dyn FileSource> = Rc::new(http_file_source());

    const CONCURRENCY: usize = 50;
    const MAX: i32 = 10000;

    let state = Rc::new(LoadState {
        reqs: (0..CONCURRENCY).map(|_| req()).collect(),
        number: Cell::new(1),
    });
    let base = server.url("/load/");
    let url: Rc<dyn Fn(i32) -> String> = Rc::new(move |n| format!("{base}{n}"));

    for i in 0..CONCURRENCY {
        load_request(&fs, &state, &url, &run_loop, i, MAX);
    }

    run_loop.run();
}

