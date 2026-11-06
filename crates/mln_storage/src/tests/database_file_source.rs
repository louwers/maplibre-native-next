// Ports test/storage/database_file_source.test.cpp. The tests take
// `serial()`: they share the FileSourceManager's database file source.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use mln_actor::{RunLoop, Timer};
use mln_util::chrono::Duration;

use super::{local_callback, req, serial};
use crate::ClientOptions;
use crate::database_file_source::DatabaseFileSource;
use crate::file_source::{FileSource, FileSourceType};
use crate::file_source_manager::FileSourceManager;
use crate::resource::{Kind, LoadingMethod, Resource, StoragePolicy};
use crate::resource_options::ResourceOptions;
use crate::response::{Reason, Response};

fn dbfs() -> Arc<dyn FileSource> {
    FileSourceManager::get()
        .get_file_source(FileSourceType::Database, &ResourceOptions::default(), &ClientOptions::default())
        .expect("database file source")
}

// gtest: DatabaseFileSource.PauseResume
#[test]
fn pause_resume() {
    let _serial = serial();
    let run_loop = RunLoop::new();

    let dbfs = dbfs();
    dbfs.pause();

    let res = Resource::with_tile_data(Kind::Unknown, "http://127.0.0.1:3000/test", None, LoadingMethod::CacheOnly);
    let stop = run_loop.clone();
    let _req = dbfs.request(&res, Box::new(move |_: Response| stop.stop()));

    let resume_timer = Timer::new();
    let resumed = dbfs.clone();
    resume_timer.start(Duration::from_millis(5), Duration::zero(), move || resumed.resume());

    run_loop.run();
}

// gtest: DatabaseFileSource.VolatileResource
#[test]
fn volatile_resource() {
    let _serial = serial();
    let run_loop = RunLoop::new();

    let dbfs = dbfs();

    let resource = Rc::new(RefCell::new(Resource::with_tile_data(
        Kind::Unknown,
        "http://127.0.0.1:3000/test",
        None,
        LoadingMethod::CacheOnly,
    )));
    let mut response = Response::new();
    response.data = Some(Arc::new(b"Cached value".to_vec()));
    let req = req();

    let (r, fs, res, stop) = (req.clone(), dbfs.clone(), resource.clone(), run_loop.clone());
    let forwarded = resource.borrow().clone();
    dbfs.forward(
        &forwarded,
        &response,
        Some(local_callback(move || {
            let (r1, fs1, res1) = (r.clone(), fs.clone(), res.clone());
            let request = res.borrow().clone();
            *r.borrow_mut() = Some(fs.request(
                &request,
                Box::new(move |res_1: Response| {
                    assert_eq!(None, res_1.error);
                    assert!(res_1.data.is_some());
                    assert!(!res_1.no_content);
                    assert_eq!(b"Cached value", res_1.data.unwrap().as_slice());
                    res1.borrow_mut().storage_policy = StoragePolicy::Volatile;
                    let r2 = r1.clone();
                    let stop = stop.clone();
                    let request = res1.borrow().clone();
                    *r1.borrow_mut() = Some(fs1.request(
                        &request,
                        Box::new(move |res_2: Response| {
                            r2.borrow_mut().take();
                            let error = res_2.error.as_ref().expect("error");
                            assert!(res_2.no_content);
                            assert_eq!(Reason::NotFound, error.reason);
                            assert_eq!("Not found in offline database", error.message);
                            stop.stop();
                        }),
                    ));
                }),
            ));
        })),
    );
    run_loop.run();
}

// gtest: DatabaseFileSource.VolatileResourceIsNotStored
#[test]
fn volatile_resource_is_not_stored() {
    let _serial = serial();
    let run_loop = RunLoop::new();

    let dbfs = dbfs();

    let mut resource = Resource::with_tile_data(
        Kind::Unknown,
        "http://127.0.0.1:3000/volatile-not-stored",
        None,
        LoadingMethod::CacheOnly,
    );
    resource.storage_policy = StoragePolicy::Volatile;
    let mut response = Response::new();
    response.data = Some(Arc::new(b"Volatile value".to_vec()));

    // Forwarding a volatile response must not write it to the database, so
    // a regular request for the same resource finds nothing.
    dbfs.forward(&resource, &response, None);
    resource.storage_policy = StoragePolicy::Permanent;
    let req = req();
    let result: Rc<RefCell<Option<Response>>> = Rc::new(RefCell::new(None));
    let (r, out, stop) = (req.clone(), result.clone(), run_loop.clone());
    *req.borrow_mut() = Some(dbfs.request(
        &resource,
        Box::new(move |res: Response| {
            r.borrow_mut().take();
            stop.stop();
            *out.borrow_mut() = Some(res);
        }),
    ));
    run_loop.run();

    let res = result.borrow_mut().take().expect("response");
    assert!(res.no_content);
    let error = res.error.expect("error");
    assert_eq!(Reason::NotFound, error.reason);
}

// Not a C++ test: `canRequest`.
#[test]
fn can_request() {
    let dbfs = DatabaseFileSource::new(&ResourceOptions::default(), &ClientOptions::default());
    let cache_only = |url: &str| Resource::with_tile_data(Kind::Unknown, url, None, LoadingMethod::CacheOnly);
    assert!(dbfs.can_request(&cache_only("http://example.com")));
    assert!(!dbfs.can_request(&cache_only("asset://example.json")));
    assert!(!dbfs.can_request(&cache_only("file://example.json")));
    assert!(!dbfs.can_request(&Resource::with_tile_data(
        Kind::Unknown,
        "http://example.com",
        None,
        LoadingMethod::NetworkOnly
    )));
}
