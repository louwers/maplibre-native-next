// Runs the render tests in the page (see tools/web-test/src/lib.rs) and
// reports each result to tools/web-test/run.mjs's server.
//
// The server holds the run's state: the test list and the results so far. A
// page that starts (again, after the server restarted a hung browser) picks
// up at the first test without a result. Each test runs in a call of its
// own; one that panics aborts the module (wasm32-unknown-unknown has no
// unwinding), so the page reports it as errored and continues on a new
// module instance.

const status = document.getElementById("status");

const post = (path, body) =>
  fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

// The module's log records (mln::Log, panics). They go to the server with
// the test's result, and meanwhile every second while the module waits for
// the browser, so that a test that hangs leaves its log behind.
let logs = [];
let panic = null;
let current = null;
globalThis.mlnWebTestLog = (level, message) => {
  if (level === "PANIC") panic = message;
  logs.push(`[${level}] ${message}`);
};
// Also a heartbeat: when it stops, the module is busy without returning to
// the browser; while it goes on, the count of waits tells whether the module
// is waiting for the browser.
import { stats } from "mln_browser";
setInterval(() => {
  if (current === null) return;
  post("/__log", { index: current, lines: logs, waits: stats.waits });
  logs = [];
}, 1000);

const wasm = await WebAssembly.compileStreaming(fetch("./pkg/web_test_bg.wasm"));
let instances = 0;

// A new module instance: a fresh import of the wasm-bindgen glue (its
// module state is per import URL) over the compiled module.
async function instantiate() {
  instances += 1;
  const glue = await import(`./pkg/web_test.js?instance=${instances}`);
  const exports = await glue.default({ module_or_path: wasm });
  glue.init();
  return {
    glue,
    // The entry points may suspend (JSPI): call them through `promising`,
    // not through the glue's synchronous wrappers.
    list: WebAssembly.promising(exports.list),
    run: WebAssembly.promising(exports.run),
  };
}

async function call(instance, entry, request) {
  instance.glue.set_request(JSON.stringify(request));
  const code = await instance[entry]();
  const response = JSON.parse(instance.glue.take_response());
  if (code !== 0) throw new Error(response.error ?? `${entry} returned ${code}`);
  return response;
}

// The Rust regex crate's syntax: escape the metacharacters.
const exactly = (path) => `^${path.replace(/[\\.+*?()|[\]{}^$]/g, "\\$&")}$`;

try {
  const state = await (await fetch("/__state")).json();
  const { config } = state;
  let instance = await instantiate();
  let { tests, cache } = state;
  if (!tests) {
    status.textContent = "listing tests";
    ({ tests, cache } = await call(instance, "list", {
      manifest: config.manifest,
      filter: config.filter,
      walk: config.walk,
    }));
    // What the browser says about the adapter the tests render on.
    const info = (await navigator.gpu?.requestAdapter())?.info;
    const adapter = info ? [info.vendor, info.architecture, info.device, info.description].filter(Boolean).join(" ") : "none";
    await post("/__tests", { tests, cache, adapter });
  }
  for (let index = state.next; index < tests.length; index += 1) {
    status.textContent = `${index + 1}/${tests.length} ${tests[index]}`;
    await post("/__started", { index });
    logs = [];
    panic = null;
    current = index;
    let result;
    try {
      result = await call(instance, "run", {
        manifest: config.manifest,
        filter: exactly(tests[index]),
        output: config.output,
        walk: config.walk,
        cache,
      });
    } catch (error) {
      result = { crash: panic ?? String(error?.stack ?? error) };
      instance = await instantiate();
    }
    current = null;
    await post("/__result", { index, result, logs });
  }
  status.textContent = "done";
  await post("/__done", {});
} catch (error) {
  status.textContent = `failed: ${error}`;
  await post("/__fatal", { message: String(error?.stack ?? error), logs });
}
