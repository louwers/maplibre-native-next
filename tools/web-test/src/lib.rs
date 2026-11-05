// The shared render tests in the browser: tools/render-test's runner built for
// wasm32-unknown-unknown, rendering through wgpu's WebGPU backend, i.e. the
// browser's own WebGPU (tools/web-test/run.mjs drives headless Chrome).
// Port-only: the C++ core has no browser render-test runner.
//
// The module runs on the page's thread (mln_actor's thread pools and
// util::Thread objects run on its RunLoop there). The page (web/main.js)
// calls `list` and `run` through `WebAssembly.promising`, so that the
// synchronous runner can wait for the browser with JSPI
// (mln_util::browser): for GPU readbacks, and for the test files, which come
// from run.mjs's server (`WebFileSystem`).
//
// - `list`: parses the manifest (ignores, test filter) and returns the tests'
//   style paths in run order; imports the cache database the render tests
//   load their resources from (`cache_path`, metrics/cache-style.db) into
//   SQLite's memory VFS under that path, where mln_storage's database file
//   source opens it.
// - `run`: `runRenderTests` with a filter, normally for one test, so that a
//   test that panics (which aborts the module: wasm32-unknown-unknown has no
//   unwinding) costs only itself; the page then starts a new module.
//
// Requests and responses are JSON, passed with `set_request` and
// `take_response` (the JSPI entry points take and return plain integers).

use mln_util::browser::block_on;
use render_test::filesystem;
use render_test::manifest_parser::ManifestParser;
use render_test::render_test::{Arguments, run_render_tests_with};
use render_test::runner::UpdateResults;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    /// The page's sink for log records and panic messages (web/main.js).
    #[wasm_bindgen(js_name = mlnWebTestLog)]
    fn page_log(level: &str, message: &str);
}

thread_local! {
    static REQUEST: RefCell<String> = const { RefCell::new(String::new()) };
    static RESPONSE: RefCell<String> = const { RefCell::new(String::new()) };
}

#[wasm_bindgen]
pub fn set_request(request: String) {
    REQUEST.with(|r| *r.borrow_mut() = request);
}

#[wasm_bindgen]
pub fn take_response() -> String {
    RESPONSE.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

fn request() -> Value {
    REQUEST.with(|r| serde_json::from_str(&r.borrow()).unwrap_or(Value::Null))
}

fn respond(value: Value) {
    RESPONSE.with(|r| *r.borrow_mut() = value.to_string());
}

/// Forwards mln::Log records to the page (native: stderr).
struct PageLogObserver;

impl mln_util::logging::Observer for PageLogObserver {
    fn on_record(
        &self,
        severity: mln_util::logging::EventSeverity,
        event: mln_util::logging::Event,
        code: i64,
        msg: &str,
    ) -> bool {
        use mln_util::enum_::Enum;
        let code = if code >= 0 { format!("({code})") } else { String::new() };
        page_log(severity.to_string(), &format!("[{}]{code}: {msg}", event.to_string()));
        true
    }
}

/// Sets up the module: the panic message goes to the page before the module
/// aborts, the log to the page's console.
#[wasm_bindgen]
pub fn init() {
    std::panic::set_hook(Box::new(|info| page_log("PANIC", &info.to_string())));
    mln_util::logging::Log::set_observer(Box::new(PageLogObserver));
}

fn js_error(value: JsValue) -> io::Error {
    io::Error::other(format!("{value:?}"))
}

/// `fetch(url)` from the page's origin, waited for with JSPI.
fn fetch(method: &str, url: &str, body: Option<&[u8]>) -> io::Result<(u16, Vec<u8>)> {
    let init = web_sys::RequestInit::new();
    init.set_method(method);
    if let Some(body) = body {
        init.set_body(&js_sys::Uint8Array::from(body));
    }
    let window = web_sys::window().ok_or_else(|| io::Error::other("no window"))?;
    let response = block_on(JsFuture::from(window.fetch_with_str_and_init(url, &init))).map_err(js_error)?;
    let response: web_sys::Response = response.dyn_into().map_err(js_error)?;
    let status = response.status();
    let buffer = block_on(JsFuture::from(response.array_buffer().map_err(js_error)?)).map_err(js_error)?;
    Ok((status, js_sys::Uint8Array::new(&buffer).to_vec()))
}

fn url_of(path: &str) -> String {
    let encoded: String = path
        .split('/')
        .map(|segment| js_sys::encode_uri_component(segment).as_string().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("/");
    format!("/fs{encoded}")
}

fn status_error(status: u16, path: &str) -> io::Error {
    match status {
        404 => io::Error::new(io::ErrorKind::NotFound, format!("{path}: not found")),
        _ => io::Error::other(format!("{path}: HTTP {status}")),
    }
}

/// The runner's file system in the browser: run.mjs's `/fs/` routes, where
/// `/_main` is this repository and `/maplibre+` the C++ checkout (the Bazel
/// runfiles layout the manifests' relative paths expect), and `/out` the
/// output directory. Directory trees named in `walk` are listed in one
/// request up front; their listings and sizes are then answered here.
#[derive(Default)]
struct WebFileSystem {
    /// Normalized directory path -> entries (name, is_dir, size).
    dirs: RefCell<HashMap<String, Vec<(String, bool, u64)>>>,
    /// Roots whose listings `dirs` holds completely.
    walked: RefCell<Vec<String>>,
    /// JSON files read before (manifests, ignore lists): every test reads
    /// them again.
    json: RefCell<HashMap<String, Vec<u8>>>,
}

fn normalize(path: &str) -> String {
    let normal = filesystem::lexically_normal(path);
    match normal.strip_suffix('/') {
        Some(stripped) if !stripped.is_empty() => stripped.to_string(),
        _ => normal,
    }
}

fn split_parent(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(0) => ("/", &path[1..]),
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    }
}

impl WebFileSystem {
    fn walk(&self, root: &str) -> io::Result<()> {
        let root = normalize(root);
        let (status, body) = fetch("GET", &format!("{}?walk", url_of(&root)), None)?;
        if status != 200 {
            return Err(status_error(status, &root));
        }
        let tree: HashMap<String, Vec<(String, bool, u64)>> =
            serde_json::from_slice(&body).map_err(io::Error::other)?;
        self.dirs.borrow_mut().extend(tree);
        self.walked.borrow_mut().push(root);
        Ok(())
    }

    fn is_walked(&self, path: &str) -> bool {
        self.walked
            .borrow()
            .iter()
            .any(|root| path == root || path.strip_prefix(root.as_str()).is_some_and(|rest| rest.starts_with('/')))
    }
}

impl filesystem::Host for WebFileSystem {
    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        let path = normalize(path);
        if let Some(data) = self.json.borrow().get(&path) {
            return Ok(data.clone());
        }
        let (status, body) = fetch("GET", &url_of(&path), None)?;
        if status != 200 {
            return Err(status_error(status, &path));
        }
        if path.ends_with(".json") && !path.starts_with("/out/") {
            self.json.borrow_mut().insert(path, body.clone());
        }
        Ok(body)
    }

    fn write(&self, path: &str, data: &[u8]) -> io::Result<()> {
        let path = normalize(path);
        let (status, _) = fetch("PUT", &url_of(&path), Some(data))?;
        if status / 100 != 2 {
            return Err(status_error(status, &path));
        }
        Ok(())
    }

    /// run.mjs creates the directories of the files it writes.
    fn create_dir_all(&self, _path: &str) -> io::Result<()> {
        Ok(())
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<(String, bool)>> {
        let path = normalize(path);
        if let Some(entries) = self.dirs.borrow().get(&path) {
            return Ok(entries.iter().map(|(name, is_dir, _)| (name.clone(), *is_dir)).collect());
        }
        if self.is_walked(&path) {
            return Err(io::Error::new(io::ErrorKind::NotFound, format!("{path}: not a directory")));
        }
        let (status, body) = fetch("GET", &format!("{}?list", url_of(&path)), None)?;
        if status != 200 {
            return Err(status_error(status, &path));
        }
        let entries: Vec<(String, bool, u64)> = serde_json::from_slice(&body).map_err(io::Error::other)?;
        Ok(entries.into_iter().map(|(name, is_dir, _)| (name, is_dir)).collect())
    }

    fn metadata(&self, path: &str) -> io::Result<(bool, u64)> {
        let path = normalize(path);
        if self.is_walked(&path) {
            if self.dirs.borrow().contains_key(&path) {
                return Ok((false, 0));
            }
            let (parent, name) = split_parent(&path);
            return self
                .dirs
                .borrow()
                .get(parent)
                .and_then(|entries| entries.iter().find(|(n, _, _)| n == name))
                .map(|(_, is_dir, size)| (!is_dir, *size))
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{path}: not found")));
        }
        let (status, body) = fetch("GET", &format!("{}?stat", url_of(&path)), None)?;
        if status != 200 {
            return Err(status_error(status, &path));
        }
        let (is_file, size): (bool, u64) = serde_json::from_slice(&body).map_err(io::Error::other)?;
        Ok((is_file, size))
    }
}

/// Copies the database at `path` (from run.mjs) into SQLite's memory VFS
/// under the same name, so that opening `path` opens it.
fn import_database(path: &str) -> Result<(), String> {
    use sqlite_wasm_rs::vfs::memvfs::MemVfsUtil;
    use sqlite_wasm_rs::vfs::VfsFilesManager;
    use sqlite_wasm_rs::vfs::transfer::{DbTransfer, ImportTarget};

    let (status, data) = fetch("GET", &url_of(&normalize(path)), None).map_err(|e| e.to_string())?;
    if status != 200 {
        return Err(format!("{path}: HTTP {status}"));
    }
    // SAFETY: SQLite runs on this thread only (the browser build has one),
    // and nothing else registers or removes VFSes.
    let util = unsafe { MemVfsUtil::get() }.map_err(|e| e.to_string())?;
    if util.contains(path).unwrap_or(false) {
        return Ok(());
    }
    let mut target = util.create_import(path, data.len() as u64).map_err(|e| e.to_string())?;
    target.write_at(0, &data).map_err(|e| e.to_string())?;
    target.commit().map_err(|e| e.to_string())
}

fn string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Installs the file system, once per module instance.
fn install_file_system(request: &Value) -> Result<(), String> {
    thread_local! {
        static INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if INSTALLED.with(|installed| installed.replace(true)) {
        return Ok(());
    }
    let fs = WebFileSystem::default();
    for root in string_list(&request["walk"]) {
        fs.walk(&root).map_err(|e| format!("listing {root}: {e}"))?;
    }
    filesystem::set_host(Box::new(fs));
    Ok(())
}

/// `{manifest, filter, walk}` -> `{tests: [style paths], cache}`.
#[wasm_bindgen]
pub fn list() -> i32 {
    let request = request();
    if let Err(e) = install_file_system(&request) {
        respond(json!({ "error": e }));
        return 1;
    }
    let manifest_path = request["manifest"].as_str().unwrap_or_default();
    let filter = request["filter"].as_str().unwrap_or_default().to_string();
    let Some(manifest) = ManifestParser::parse_manifest(manifest_path, filter) else {
        respond(json!({ "error": format!("cannot parse manifest {manifest_path}") }));
        return 1;
    };
    let cache = manifest.get_cache_path().to_string();
    if !cache.is_empty() {
        if let Err(e) = import_database(&cache) {
            respond(json!({ "error": format!("importing the cache database: {e}") }));
            return 1;
        }
    }
    let tests: Vec<&str> = manifest.get_test_paths().iter().map(|paths| paths.style_path.as_str()).collect();
    respond(json!({ "tests": tests, "cache": cache }));
    0
}


/// The map factory, with its RunLoop, once per module instance: the objects
/// util::Thread establishes (file sources, which FileSourceManager shares
/// between maps) run on the RunLoop current when they were created, so it
/// has to stay the thread's RunLoop from one `run` to the next, as it does
/// for a native run of the whole suite.
fn factory() -> render_test::runner::ImplFactory {
    thread_local! {
        static FACTORY: std::rc::Rc<render_test::runner::ImplFactory> =
            std::rc::Rc::new(render_test_map::factory());
    }
    let factory = FACTORY.with(std::rc::Rc::clone);
    Box::new(move |metadata, resource_options| factory(metadata, resource_options))
}

/// `{manifest, filter, output, walk}` -> `{code, tests: [{id, status, error,
/// difference, allowed}]}`: `runRenderTests` over the tests that match.
#[wasm_bindgen]
pub fn run() -> i32 {
    let request = request();
    if let Err(e) = install_file_system(&request) {
        respond(json!({ "error": e }));
        return 1;
    }
    let manifest_path = request["manifest"].as_str().unwrap_or_default();
    let cache = request["cache"].as_str().unwrap_or_default();
    if !cache.is_empty() {
        if let Err(e) = import_database(cache) {
            respond(json!({ "error": format!("importing the cache database: {e}") }));
            return 1;
        }
    }
    let arguments = Arguments {
        recycle_map: false,
        shuffle: false,
        online: false,
        seed: 1,
        manifest_path: manifest_path.to_string(),
        update_results: UpdateResults::No,
        test_filter: request["filter"].as_str().unwrap_or_default().to_string(),
        self_check: false,
        output: request["output"].as_str().map(str::to_string),
        // No watchdog thread in the browser; run.mjs times the page out.
        timeout_millis: 0,
    };
    let result = run_render_tests_with(&arguments, Some(factory()));
    let tests: Vec<Value> = result
        .metadatas
        .iter()
        .map(|metadata| {
            json!({
                "id": metadata.id,
                "status": metadata.status,
                "error": metadata.error_message,
                "difference": metadata.difference,
                "allowed": metadata.allowed,
            })
        })
        .collect();
    respond(json!({ "code": result.return_code, "tests": tests }));
    0
}
