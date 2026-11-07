// Ports include/mln/storage/pmtiles_file_source.hpp and
// platform/default/src/mln/storage/pmtiles_file_source.cpp.
//
// The archives are read with the `pmtiles` crate (stadiamaps/pmtiles-rs)
// instead of a port of vendor/PMTiles/cpp/pmtiles.hpp. Its reader is async,
// so each request runs as a future on the "PMTilesFileSource" thread
// (`util::Thread`, as in C++), polled by the small executor below: the
// reader's reads are range requests through the ResourceLoader file source,
// whose callbacks come back to this thread and wake the request's future.
// DEVIATIONS.md lists what that changes.
//
// `tasks` is keyed by a request id instead of the caller's `AsyncRequest*`;
// as in C++, a sub-request replaces the previous one of the same caller and
// nothing removes them when the caller cancels. The TileJSON document is
// built as a rapidjson document would be (members in insertion order,
// duplicates kept) and written with //vendor/rapidjson's `Writer`.
//
// The resource and client options are shared with the thread behind a mutex
// (C++: in the thread object, read back with a blocking `ask`).
//
// TODO(port): makeThreadPrioritySetter(EXPERIMENTAL_THREAD_PRIORITY_FILE).

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use bytes::Bytes;
use mln_actor::{ActorRef, Thread};
use mln_platform_default_image::compression::{decompress, is_compressed};
use mln_util::client_options::ClientOptions;
use mln_util::constants::{FILE_PROTOCOL, PMTILES_PROTOCOL};
use pmtiles::{
    AsyncBackend, AsyncPmTilesReader, BackendResponse, Compression, DirEntry, Directory, DirectoryCache, Header,
    PmtError, PmtResult, TileCoord, TileId, TileType,
};
use serde_json::Value as Json;

use crate::async_request::AsyncRequest;
use crate::file_source::{Callback, FileSource, FileSourceType};
use crate::file_source_manager::FileSourceManager;
use crate::file_source_request::{FileSourceRequest, FileSourceRequestState};
use crate::mbtiles_file_source::{Document, strtol};
use crate::resource::{Kind, LoadingMethod, Resource, TileData};
use crate::resource_options::ResourceOptions;
use crate::response::{Error, Reason, Response};

// To avoid allocating lots of memory with PMTiles directory caching, set a
// limit so it doesn't grow unlimited
const MAX_DIRECTORY_CACHE_ENTRIES: usize = 100;

type Reader = AsyncPmTilesReader<Backend, DirectoryLru>;

fn accepts_url(url: &str) -> bool {
    url.starts_with(PMTILES_PROTOCOL)
}

fn extract_url(url: &str) -> String {
    url[PMTILES_PROTOCOL.len().min(url.len())..].to_owned()
}

fn error(reason: Reason, message: impl Into<String>) -> Option<Box<Error>> {
    Some(Box::new(Error::new(reason, message)))
}

fn json_double(d: f64) -> Json {
    serde_json::Number::from_f64(d).map_or(Json::Null, Json::Number)
}

fn send(req: &ActorRef<FileSourceRequestState>, response: Response) {
    req.invoke(move |request: &FileSourceRequestState| request.set_response(response));
}

/// A failed range request, carried through the reader in a
/// `PmtError::Reading`.
#[derive(Debug)]
struct FetchError(Error);

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0.message)
    }
}

impl std::error::Error for FetchError {}

/// The error of the range request that made the reader fail, if one did.
fn fetch_error(e: &PmtError) -> Option<&Error> {
    let PmtError::Reading(e) = e else {
        return None;
    };
    e.get_ref()?.downcast_ref::<FetchError>().map(|e| &e.0)
}

thread_local! {
    /// The request whose future is being polled: the reads it makes are its
    /// sub-requests.
    static POLLING: RefCell<Option<(Rc<State>, u64)>> = const { RefCell::new(None) };
}

/// The reader's backend: range requests for the archive at `url` through the
/// ResourceLoader file source.
struct Backend {
    url: String,
}

impl AsyncBackend for Backend {
    fn read(&self, offset: u64, length: usize) -> impl Future<Output = PmtResult<BackendResponse>> + Send {
        let last = offset.wrapping_add(length as u64).wrapping_sub(1);
        Read { url: self.url.clone(), range: (offset, last), slot: None }
    }
}

/// What a range request's callback hands its `Read`.
#[derive(Default)]
struct Slot {
    response: Option<Response>,
    answered: bool,
    waker: Option<Waker>,
}

/// A range request of the reader, made when first polled.
struct Read {
    url: String,
    range: (u64, u64),
    slot: Option<Arc<Mutex<Slot>>>,
}

impl Future for Read {
    type Output = PmtResult<BackendResponse>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let slot = this.slot.get_or_insert_with(|| {
            let slot = Arc::new(Mutex::new(Slot::default()));
            let mut resource = Resource::new(Kind::Source, this.url.clone());
            resource.loading_method = LoadingMethod::All;
            resource.data_range = Some(this.range);
            let answer = slot.clone();
            POLLING.with_borrow(|polling| {
                let (state, req) = polling.as_ref().expect("read outside the PMTiles file source");
                state.request(*req, &resource, move |response: Response| {
                    if response.not_modified {
                        return;
                    }
                    // Only the first answer counts: a stale cached response
                    // can be followed by the revalidated one.
                    let mut slot = answer.lock().unwrap();
                    if slot.answered {
                        return;
                    }
                    slot.answered = true;
                    slot.response = Some(response);
                    let waker = slot.waker.take();
                    drop(slot);
                    if let Some(waker) = waker {
                        waker.wake();
                    }
                });
            });
            slot
        });

        let mut slot = slot.lock().unwrap();
        let Some(mut response) = slot.response.take() else {
            slot.waker = Some(cx.waker().clone());
            return Poll::Pending;
        };
        drop(slot);

        if let Some(e) = response.error.take() {
            return Poll::Ready(Err(PmtError::Reading(std::io::Error::other(FetchError(*e)))));
        }
        let Some(data) = response.data.take() else {
            let e = Error::new(Reason::Other, "response has no data");
            return Poll::Ready(Err(PmtError::Reading(std::io::Error::other(FetchError(e)))));
        };
        // The tile response takes the cache headers of the tile's read, the
        // request's last.
        POLLING.with_borrow(|polling| {
            if let Some((state, req)) = polling {
                state.last_responses.borrow_mut().insert(*req, response);
            }
        });
        Poll::Ready(Ok(BackendResponse::new(Bytes::from(Arc::unwrap_or_clone(data)))))
    }
}

/// The leaf directories of an archive, at most `MAX_DIRECTORY_CACHE_ENTRIES`
/// of them; the least recently used one goes first.
#[derive(Default)]
struct DirectoryLru(Mutex<VecDeque<(u64, Directory)>>);

impl DirectoryLru {
    /// The entry for `tile_id` in the cached directory at `offset`, if that
    /// directory is cached.
    fn find(&self, offset: u64, tile_id: TileId) -> Option<Option<DirEntry>> {
        let mut directories = self.0.lock().unwrap();
        let position = directories.iter().position(|(o, _)| *o == offset)?;
        let directory = directories.remove(position)?;
        let entry = directory.1.find_tile_id(tile_id).copied();
        directories.push_back(directory);
        Some(entry)
    }
}

impl DirectoryCache for DirectoryLru {
    async fn get_dir_entry_or_insert(
        &self,
        offset: u64,
        tile_id: TileId,
        fetcher: impl Future<Output = PmtResult<Directory>> + Send,
    ) -> PmtResult<Option<DirEntry>> {
        if let Some(entry) = self.find(offset, tile_id) {
            return Ok(entry);
        }
        let directory = fetcher.await?;
        let entry = directory.find_tile_id(tile_id).copied();
        let mut directories = self.0.lock().unwrap();
        if !directories.iter().any(|(o, _)| *o == offset) {
            directories.push_back((offset, directory));
            if directories.len() > MAX_DIRECTORY_CACHE_ENTRIES {
                directories.pop_front();
            }
        }
        Ok(entry)
    }
}

/// Wakes a request's future: polls it again on the file source's thread.
struct RequestWaker {
    thread: ActorRef<PMTilesFileSourceThread>,
    req: u64,
}

impl Wake for RequestWaker {
    fn wake(self: Arc<Self>) {
        let req = self.req;
        self.thread.invoke(move |thread: &PMTilesFileSourceThread| thread.state.poll(req));
    }
}

/// The TileJSON document of an archive: its JSON metadata `data`, completed
/// from the header.
fn tilejson(url: &str, header: &Header, data: &[u8]) -> String {
    let mut doc = if data.is_empty() { Document::default() } else { Document::parse_object(data) };

    doc.add_member("tilejson", Json::String("3.0.0".to_owned()));

    if !doc.has_member("scheme") {
        doc.add_member("scheme", Json::Null);
    }

    *doc.get_mut("scheme") = Json::String("xyz".to_owned());

    if !doc.has_member("tiles") {
        doc.add_member("tiles", Json::Null);
    }

    if !doc.get_mut("tiles").is_array() {
        *doc.get_mut("tiles") = Json::Array(vec![Json::String(format!("{PMTILES_PROTOCOL}{url}"))]);
    }

    // Translate tile type field to source encoding.
    if header.tile_type == TileType::Mlt {
        doc.add_member("encoding", Json::String("mlt".to_owned()));
    }

    if !doc.has_member("bounds") {
        doc.add_member("bounds", Json::Null);
    }

    if !doc.get_mut("bounds").is_array() {
        *doc.get_mut("bounds") = Json::Array(vec![
            json_double(header.min_longitude),
            json_double(header.min_latitude),
            json_double(header.max_longitude),
            json_double(header.max_latitude),
        ]);
    }

    if !doc.has_member("center") {
        doc.add_member("center", Json::Null);
    }

    if !doc.get_mut("center").is_array() {
        *doc.get_mut("center") = Json::Array(vec![
            json_double(header.center_longitude),
            json_double(header.center_latitude),
            Json::from(header.center_zoom),
        ]);
    }

    for (name, zoom) in [("minzoom", header.min_zoom), ("maxzoom", header.max_zoom)] {
        if !doc.has_member(name) {
            doc.add_member(name, Json::Null);
        }

        let value = doc.get_mut(name);

        if let Json::String(s) = value {
            *value = Json::from(strtol(s) as i32);
        }

        if !value.is_number() {
            *value = Json::from(zoom);
        }
    }

    doc.serialize()
}

/// The members of `PMTilesFileSource::Impl`.
struct State {
    thread: ActorRef<PMTilesFileSourceThread>,
    resource_options: Arc<Mutex<ResourceOptions>>,
    client_options: Arc<Mutex<ClientOptions>>,

    file_source: RefCell<Option<Arc<dyn FileSource>>>,
    /// The archives read so far, by URL (C++: the header and directory
    /// caches).
    readers: RefCell<BTreeMap<String, Rc<Reader>>>,
    metadata_cache: RefCell<BTreeMap<String, String>>,
    tasks: RefCell<BTreeMap<u64, Box<dyn AsyncRequest>>>,
    /// The futures of the requests in progress.
    futures: RefCell<BTreeMap<u64, Pin<Box<dyn Future<Output = ()>>>>>,
    /// Each request's last sub-response, without its data.
    last_responses: RefCell<BTreeMap<u64, Response>>,
}

impl State {
    fn get_file_source(&self) -> Arc<dyn FileSource> {
        let mut file_source = self.file_source.borrow_mut();
        file_source
            .get_or_insert_with(|| {
                let resource_options = self.resource_options.lock().unwrap().clone();
                let client_options = self.client_options.lock().unwrap().clone();
                FileSourceManager::get()
                    .get_file_source(FileSourceType::ResourceLoader, &resource_options, &client_options)
                    .expect("ResourceLoader file source")
            })
            .clone()
    }

    /// `tasks[req] = getFileSource()->request(resource, callback)`.
    fn request(self: &Rc<Self>, req: u64, resource: &Resource, callback: impl FnMut(Response) + 'static) {
        let file_source = self.get_file_source();
        let request = file_source.request(resource, Box::new(callback));
        let previous = self.tasks.borrow_mut().insert(req, request);
        drop(previous);
    }

    /// Runs `future` as request `req`.
    fn spawn(self: &Rc<Self>, req: u64, future: impl Future<Output = ()> + 'static) {
        self.futures.borrow_mut().insert(req, Box::pin(future));
        self.poll(req);
    }

    /// Polls request `req`'s future, if it hasn't finished.
    fn poll(self: &Rc<Self>, req: u64) {
        // Taken out while it runs, as it can use the state.
        let Some(mut future) = self.futures.borrow_mut().remove(&req) else {
            return;
        };
        let waker = Waker::from(Arc::new(RequestWaker { thread: self.thread.clone(), req }));
        let outer = POLLING.replace(Some((self.clone(), req)));
        let poll = future.as_mut().poll(&mut Context::from_waker(&waker));
        POLLING.set(outer);
        if poll.is_pending() {
            self.futures.borrow_mut().insert(req, future);
        } else {
            self.last_responses.borrow_mut().remove(&req);
        }
    }

    /// The archive at `url`, read on first use (C++: `getHeader`).
    async fn reader(&self, url: &str) -> Result<Rc<Reader>, Box<Error>> {
        if let Some(reader) = self.readers.borrow().get(url) {
            return Ok(reader.clone());
        }

        let reader =
            AsyncPmTilesReader::try_from_source_and_cache(Backend { url: url.to_owned() }, DirectoryLru::default())
                .await
                .map_err(|e| {
                    if let Some(response_error) = fetch_error(&e) {
                        let mut message = format!("Error fetching PMTiles header: {}", response_error.message);

                        if response_error.message.is_empty() && response_error.reason == Reason::NotFound {
                            if let Some(path) = url.strip_prefix(FILE_PROTOCOL) {
                                message += &format!("path not found: {path}");
                            } else {
                                message += &format!("url not found: {url}");
                            }
                        }

                        return Box::new(Error::new(response_error.reason, message));
                    }
                    let message = match e {
                        PmtError::UnsupportedCompression(_) => "Compression method not supported".to_owned(),
                        e => e.to_string(),
                    };
                    Box::new(Error::new(Reason::Other, format!("Error parsing PMTiles header: {message}")))
                })?;

        let header = reader.get_header();
        let supported = |c: Compression| c == Compression::None || c == Compression::Gzip;
        let message = if header.spec_version() != 3 {
            Some("pmtiles version: must be 3")
        } else if !supported(header.internal_compression()) || !supported(header.tile_compression) {
            Some("Compression method not supported")
        } else {
            None
        };
        if let Some(message) = message {
            return Err(Box::new(Error::new(Reason::Other, format!("Error parsing PMTiles header: {message}"))));
        }

        Ok(self.readers.borrow_mut().entry(url.to_owned()).or_insert(Rc::new(reader)).clone())
    }

    /// The TileJSON of the archive at `url` (C++: `getMetadata`).
    async fn get_metadata(&self, url: &str) -> Result<String, Box<Error>> {
        if let Some(metadata) = self.metadata_cache.borrow().get(url) {
            return Ok(metadata.clone());
        }

        let reader = self.reader(url).await?;
        let data = match reader.get_metadata().await {
            Ok(data) => data.into_bytes(),
            // The archive has no metadata.
            Err(PmtError::InvalidMetadata) => Vec::new(),
            // Parsed like any other bytes.
            Err(PmtError::InvalidMetadataUtf8Encoding(e)) => e.into_bytes(),
            Err(e) => {
                let error = if let Some(e) = fetch_error(&e) {
                    Error::new(e.reason, format!("Error fetching PMTiles metadata: {}", e.message))
                } else if let PmtError::Reading(e) = &e {
                    Error::new(Reason::Other, format!("Error decompressing PMTiles metadata: {e}"))
                } else {
                    Error::new(Reason::Other, format!("Error fetching PMTiles metadata: {e}"))
                };
                return Err(Box::new(error));
            }
        };

        let metadata = tilejson(url, reader.get_header(), &data);
        Ok(self.metadata_cache.borrow_mut().entry(url.to_owned()).or_insert(metadata).clone())
    }

    /// The response to request `req` for the tile `tile_data` of the archive
    /// at `url`.
    async fn get_tile(&self, req: u64, url: &str, tile_data: &TileData) -> Response {
        let mut response = Response::new();
        response.no_content = true;

        let reader = match self.reader(url).await {
            Ok(reader) => reader,
            Err(e) => {
                response.error = Some(e);
                return response;
            }
        };
        let header = reader.get_header();

        // `std::cmp_less(z, min_zoom) || std::cmp_greater(z, max_zoom)`.
        let z = i32::from(tile_data.z);
        if z < i32::from(header.min_zoom) || z > i32::from(header.max_zoom) {
            return response;
        }

        let coord = match TileCoord::new(tile_data.z as u8, tile_data.x as u32, tile_data.y as u32) {
            Ok(coord) => coord,
            Err(e) => {
                response.error = error(Reason::Other, format!("Invalid tile: {e}"));
                return response;
            }
        };

        let data = match reader.get_tile(coord).await {
            Ok(Some(data)) => Arc::new(Vec::from(data)),
            Ok(None) => return response,
            Err(e) => {
                response.error = match fetch_error(&e) {
                    Some(e) => error(e.reason, format!("Error fetching PMTiles tile: {}", e.message)),
                    None => error(Reason::Other, format!("Error reading PMTiles tile: {e}")),
                };
                return response;
            }
        };

        let tile_response = self.last_responses.borrow_mut().remove(&req).unwrap_or_default();
        response.data = Some(data.clone());
        response.no_content = false;
        response.modified = tile_response.modified;
        response.expires = tile_response.expires;
        response.etag = tile_response.etag;

        // Support uncompressed tiles
        if header.tile_compression == Compression::Gzip && is_compressed(&data) {
            match decompress(&data) {
                Ok(decompressed) => response.data = Some(Arc::new(decompressed)),
                Err(e) => response.error = error(Reason::Other, format!("Error decompressing PMTiles tile: {e}")),
            }
        }

        response
    }
}

/// `PMTilesFileSource::Impl`.
pub struct PMTilesFileSourceThread {
    state: Rc<State>,
}

impl PMTilesFileSourceThread {
    /// Generate a tilejson resource from .pmtiles file
    pub fn request_tilejson(&self, req: u64, resource: &Resource, r: &ActorRef<FileSourceRequestState>) {
        let (state, url, r) = (self.state.clone(), extract_url(&resource.url), r.clone());
        self.state.spawn(req, async move {
            let mut response = Response::new();
            match state.get_metadata(&url).await {
                Ok(metadata) => response.data = Some(Arc::new(metadata.into_bytes())),
                Err(e) => response.error = Some(e),
            }
            send(&r, response);
        });
    }

    /// Load data for specific tile
    pub fn request_tile(&self, req: u64, resource: &Resource, r: &ActorRef<FileSourceRequestState>) {
        let url = extract_url(&resource.url);
        let tile_data = resource.tile_data.clone().expect("tile resource without tile data");
        let (state, r) = (self.state.clone(), r.clone());
        self.state.spawn(req, async move {
            let response = state.get_tile(req, &url, &tile_data).await;
            send(&r, response);
        });
    }
}

impl Drop for PMTilesFileSourceThread {
    fn drop(&mut self) {
        // The futures and the sub-requests' callbacks hold the state; drop
        // them here.
        let futures = std::mem::take(&mut *self.state.futures.borrow_mut());
        drop(futures);
        let tasks = std::mem::take(&mut *self.state.tasks.borrow_mut());
        drop(tasks);
    }
}

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// `PMTilesFileSource`: the `Pmtiles` file source (`pmtiles://` URLs of
/// local or remote archives).
pub struct PMTilesFileSource {
    thread: Thread<PMTilesFileSourceThread>,
    resource_options: Arc<Mutex<ResourceOptions>>,
    client_options: Arc<Mutex<ClientOptions>>,
}

impl PMTilesFileSource {
    /// `PMTilesFileSource(const ResourceOptions&, const ClientOptions&)`.
    pub fn new(resource_options: &ResourceOptions, client_options: &ClientOptions) -> Self {
        let resource_options = Arc::new(Mutex::new(resource_options.clone()));
        let client_options = Arc::new(Mutex::new(client_options.clone()));
        let (ro, co) = (resource_options.clone(), client_options.clone());
        PMTilesFileSource {
            thread: Thread::new("PMTilesFileSource", move |thread| PMTilesFileSourceThread {
                state: Rc::new(State {
                    thread,
                    resource_options: ro,
                    client_options: co,
                    file_source: RefCell::new(None),
                    readers: RefCell::new(BTreeMap::new()),
                    metadata_cache: RefCell::new(BTreeMap::new()),
                    tasks: RefCell::new(BTreeMap::new()),
                    futures: RefCell::new(BTreeMap::new()),
                    last_responses: RefCell::new(BTreeMap::new()),
                }),
            }),
            resource_options,
            client_options,
        }
    }
}

impl FileSource for PMTilesFileSource {
    fn request(&self, resource: &Resource, callback: Callback) -> Box<dyn AsyncRequest> {
        let req = FileSourceRequest::new(callback);
        let req_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
        let req_ref = req.actor();
        let resource = resource.clone();

        // assume if there is a tile request, that the pmtiles file has been
        // validated
        if resource.kind == Kind::Tile {
            self.thread.actor().invoke(move |t: &PMTilesFileSourceThread| t.request_tile(req_id, &resource, &req_ref));
            return Box::new(req);
        }

        // return TileJSON
        self.thread.actor().invoke(move |t: &PMTilesFileSourceThread| t.request_tilejson(req_id, &resource, &req_ref));
        Box::new(req)
    }

    fn can_request(&self, resource: &Resource) -> bool {
        accepts_url(&resource.url)
    }

    fn set_resource_options(&self, options: ResourceOptions) {
        *self.resource_options.lock().unwrap() = options;
    }

    fn get_resource_options(&self) -> ResourceOptions {
        self.resource_options.lock().unwrap().clone()
    }

    fn set_client_options(&self, options: ClientOptions) {
        *self.client_options.lock().unwrap() = options;
    }

    fn get_client_options(&self) -> ClientOptions {
        self.client_options.lock().unwrap().clone()
    }
}
