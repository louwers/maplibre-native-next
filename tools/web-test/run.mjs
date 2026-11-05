// Runs the shared render tests in headless Chrome, on the browser's WebGPU:
// tools/render-test's runner built for wasm32-unknown-unknown with wgpu's
// WebGPU backend (tools/web-test/build.py, src/lib.rs, web/).
//
// Usage: node tools/web-test/run.mjs [options]
//   --no-build             use the module in target/web-test/pkg as it is
//   --manifest PATH        manifest, relative to the repository (default:
//                          tools/render-test/manifests/web-swiftshader-wgpu.json
//                          on SwiftShader, else web-wgpu.json)
//   --filter REGEX         test filter, as the runner's --filter
//   --swiftshader          WebGPU on SwiftShader, Chrome's software Vulkan
//                          (always on Linux, as the CI runners have no GPU;
//                          --use-webgpu-adapter=swiftshader elsewhere)
//   --chrome PATH          the browser (default: $CHROME, else the usual
//                          Chrome/Chromium locations)
//   --test-timeout SECONDS per test, then the browser is restarted (120)
//   --output DIR           actual/diff images, results.json
//                          (default target/web-test/output)
//
// The page fetches the test data from this script's server, under /fs/ in
// the Bazel runfiles layout the manifests expect: /fs/_main is this
// repository, /fs/maplibre+ the C++ checkout ($MAPLIBRE_NATIVE, default
// ../maplibre-native), /fs/out the output directory (the only one written).
//
// Prints a line per test and the native runner's summary; exits 1 when a
// test failed or errored (the native runner's exit status), 2 when the run
// itself failed.

import { spawn, spawnSync } from "node:child_process";
import { createReadStream, createWriteStream, existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "../..");

function fail(message) {
  console.error(`run.mjs: error: ${message}`);
  process.exit(2);
}

const options = {
  build: true,
  manifest: null,
  filter: "",
  swiftshader: false,
  chrome: process.env.CHROME,
  testTimeout: 120,
  output: path.join(repo, "target/web-test/output"),
};
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i += 1) {
  const value = () => argv[++i] ?? fail(`${argv[i - 1]} needs a value`);
  switch (argv[i]) {
    case "--no-build": options.build = false; break;
    case "--manifest": options.manifest = value(); break;
    case "--filter": options.filter = value(); break;
    case "--swiftshader": options.swiftshader = true; break;
    case "--chrome": options.chrome = value(); break;
    case "--test-timeout": options.testTimeout = Number(value()); break;
    case "--output": options.output = path.resolve(value()); break;
    default: fail(`unknown option ${argv[i]}`);
  }
}

// Linux: the CI runners have no GPU, and Chrome's WebGPU needs SwiftShader's
// Vulkan there.
if (process.platform === "linux") options.swiftshader = true;
options.manifest ??= options.swiftshader
  ? "tools/render-test/manifests/web-swiftshader-wgpu.json"
  : "tools/render-test/manifests/web-wgpu.json";

const cpp = path.resolve(process.env.MAPLIBRE_NATIVE ?? path.join(repo, "../maplibre-native"));
if (!existsSync(path.join(cpp, "metrics/integration"))) fail(`no C++ checkout with metrics/ at ${cpp} (set MAPLIBRE_NATIVE)`);

function findChrome() {
  if (options.chrome) return options.chrome;
  const candidates = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
  ];
  return candidates.find((candidate) => existsSync(candidate)) ?? fail("no Chrome found; pass --chrome or set CHROME");
}

if (options.build) {
  const built = spawnSync("python3", ["-I", path.join(here, "build.py")], { stdio: "inherit" });
  if (built.status !== 0) fail("build.py failed");
}
const pkg = path.join(repo, "target/web-test/pkg");
if (!existsSync(path.join(pkg, "web_test_bg.wasm"))) fail(`no module in ${pkg}: run without --no-build`);

// The output of an earlier run goes; anything else stays.
if (existsSync(options.output) && readdirSync(options.output).length) {
  if (!existsSync(path.join(options.output, "results.json")) && !options.output.startsWith(path.join(repo, "target") + path.sep)) {
    fail(`${options.output} is not empty and holds no earlier results; pick another --output`);
  }
  rmSync(options.output, { recursive: true, force: true });
}
mkdirSync(options.output, { recursive: true });

// --------------------------------------------------------------------------
// The server: the page, the module, the test data and the run's state.

const roots = { _main: repo, "maplibre+": cpp, out: options.output };
const config = {
  manifest: `/_main/${options.manifest}`,
  filter: options.filter,
  output: "/out",
  // Listed in one request each (WebFileSystem in src/lib.rs).
  walk: ["/maplibre+/metrics", "/_main/tools/render-test"],
};
const state = { tests: null, cache: null, results: new Map(), current: null, startedAt: 0 };
let settle;
const finished = new Promise((resolve) => { settle = resolve; });

const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json",
  ".wasm": "application/wasm",
  ".png": "image/png",
};

/** The file a /fs/ path names, or null outside the roots. */
function resolveFs(urlPath) {
  const parts = decodeURIComponent(urlPath).split("/").filter(Boolean);
  const root = roots[parts.shift()];
  if (!root || parts.includes("..")) return null;
  return path.join(root, ...parts);
}

function entries(dir) {
  return readdirSync(dir, { withFileTypes: true })
    .map((entry) => {
      const full = path.join(dir, entry.name);
      // Bazel runfiles are symlinks; std::filesystem follows them for
      // file types, but not for directory recursion.
      const stats = statSync(full, { throwIfNoEntry: false });
      return [entry.name, entry.isDirectory(), stats?.isFile() ? stats.size : 0];
    })
    .sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
}

function walk(dir, virtual, out) {
  const list = entries(dir);
  out[virtual] = list;
  for (const [name, isDir] of list) if (isDir) walk(path.join(dir, name), `${virtual}/${name}`, out);
  return out;
}

function readBody(request) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    request.on("data", (chunk) => chunks.push(chunk));
    request.on("end", () => resolve(Buffer.concat(chunks)));
    request.on("error", reject);
  });
}

const json = (response, status, value) =>
  response.writeHead(status, { "Content-Type": "application/json" }).end(JSON.stringify(value));

function serveFs(url, request, response) {
  const virtual = decodeURIComponent(url.pathname.slice("/fs".length)).replace(/\/+$/, "") || "/";
  const file = resolveFs(url.pathname.slice("/fs".length));
  if (!file) return json(response, 403, { error: "outside the roots" });
  if (request.method === "PUT") {
    if (!virtual.startsWith("/out/")) return json(response, 403, { error: "read-only" });
    mkdirSync(path.dirname(file), { recursive: true });
    request.pipe(createWriteStream(file)).on("finish", () => response.writeHead(204).end());
    return;
  }
  const stats = statSync(file, { throwIfNoEntry: false });
  if (!stats) return json(response, 404, { error: "not found" });
  if (url.searchParams.has("stat")) return json(response, 200, [stats.isFile(), stats.isFile() ? stats.size : 0]);
  if (url.searchParams.has("list")) return stats.isDirectory() ? json(response, 200, entries(file)) : json(response, 404, {});
  if (url.searchParams.has("walk")) return stats.isDirectory() ? json(response, 200, walk(file, virtual, {})) : json(response, 404, {});
  if (!stats.isFile()) return json(response, 404, { error: "not a file" });
  response.writeHead(200, { "Content-Type": types[path.extname(file)] ?? "application/octet-stream", "Content-Length": stats.size });
  createReadStream(file).pipe(response);
}

const now = () => performance.now();

const ids = new Map(); // test index -> id, for the summary
const streamed = new Map(); // test index -> log lines sent before the result
function record(index, result, lines) {
  const logs = [...(streamed.get(index) ?? []), ...(lines ?? [])];
  streamed.delete(index);
  const seconds = index === state.current ? (now() - state.startedAt) / 1000 : undefined;
  state.results.set(index, { ...result, seconds, logs });
  const style = state.tests[index];
  const tests = result.tests ?? [];
  if (result.crash !== undefined) {
    console.log(`* errored ${style}\n* error: ${result.crash}`);
  } else if (tests.length === 0) {
    console.log(`* skipped ${style}`);
  }
  for (const test of tests) {
    ids.set(index, test.id);
    console.log(`* ${test.status} ${test.id}${test.status === "errored" ? `\n* error: ${test.error}` : ""}`);
  }
  if ((result.crash !== undefined || result.code) && logs.length) console.log(logs.map((line) => `    ${line}`).join("\n"));
}

const server = createServer(async (request, response) => {
  const url = new URL(request.url ?? "/", "http://localhost");
  response.setHeader("Cache-Control", "no-store");
  try {
    if (url.pathname.startsWith("/fs/")) return serveFs(url, request, response);
    if (url.pathname === "/__state") {
      let next = 0;
      while (state.results.has(next)) next += 1;
      return json(response, 200, { config, tests: state.tests, cache: state.cache, next });
    }
    if (request.method === "POST" && url.pathname.startsWith("/__")) {
      const body = JSON.parse((await readBody(request)).toString("utf8") || "{}");
      response.writeHead(204).end();
      switch (url.pathname) {
        case "/__tests":
          state.tests = body.tests;
          state.cache = body.cache;
          console.log(`WebGPU adapter: ${body.adapter}`);
          console.log(`${body.tests.length} tests`);
          break;
        case "/__started":
          state.current = body.index;
          state.startedAt = now();
          break;
        case "/__log":
          streamed.set(body.index, [...(streamed.get(body.index) ?? []), ...body.lines]);
          state.beat = { at: now(), waits: body.waits, startWaits: state.beat?.index === body.index ? state.beat.startWaits : body.waits, index: body.index };
          break;
        case "/__result":
          record(body.index, body.result, body.logs);
          state.current = null;
          break;
        case "/__done":
          settle({ done: true });
          break;
        case "/__fatal":
          console.error(`page failed: ${body.message}`);
          if (body.logs?.length) console.error(body.logs.join("\n"));
          settle({ done: false });
          break;
      }
      return;
    }
    const page = { "/": "index.html", "/main.js": "main.js", "/mln_browser.js": "mln_browser.js" }[url.pathname];
    const file = page ? path.join(here, "web", page) : url.pathname.startsWith("/pkg/") ? path.join(pkg, path.basename(url.pathname)) : null;
    if (!file || !existsSync(file)) return json(response, 404, { error: "not found" });
    response.writeHead(200, { "Content-Type": types[path.extname(file)] ?? "application/octet-stream" });
    createReadStream(file).pipe(response);
  } catch (error) {
    if (!response.headersSent) json(response, 500, { error: String(error) });
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const pageUrl = `http://127.0.0.1:${server.address().port}/`;

// --------------------------------------------------------------------------
// The browser. A test that hangs gets the browser restarted; the page then
// continues after it.

const chrome = findChrome();
const webgpuFlags = ["--enable-unsafe-webgpu"];
if (process.platform === "linux") {
  // Dawn on SwiftShader's Vulkan, through Chrome's Vulkan support.
  webgpuFlags.push("--enable-features=Vulkan", "--enable-unsafe-swiftshader", "--use-angle=swiftshader", "--use-vulkan=swiftshader");
} else if (options.swiftshader) {
  // Dawn's SwiftShader adapter (forcing the Linux flags on macOS keeps
  // Dawn from creating a device).
  webgpuFlags.push("--enable-unsafe-swiftshader", "--use-webgpu-adapter=swiftshader");
}

let browser = null;
let profile = null;
function launch() {
  profile = mkdtempSync(path.join(tmpdir(), "mln-web-test-"));
  browser = spawn(
    chrome,
    [
      "--headless=new",
      "--no-sandbox",
      "--disable-dev-shm-usage",
      "--password-store=basic",
      "--use-mock-keychain",
      "--no-first-run",
      "--disable-background-timer-throttling",
      "--disable-renderer-backgrounding",
      `--user-data-dir=${profile}`,
      ...webgpuFlags,
      pageUrl,
    ],
    { stdio: ["ignore", "ignore", "pipe"] },
  );
  const current = browser;
  current.stderr.on("data", (chunk) => {
    for (const line of String(chunk).split("\n")) {
      // Chrome's own noise on a headless run without a GPU or a keychain.
      if (/FATAL|(ERROR|WARNING).*(webgpu|dawn|gpu|vulkan|swiftshader)/i.test(line)) {
        process.stderr.write(`[chrome] ${line}\n`);
      }
    }
  });
  current.on("exit", (code, signal) => {
    if (current === browser && !closing) {
      console.error(`chrome exited (code ${code}, signal ${signal})`);
      restart("the browser exited");
    }
  });
}

let closing = false;
let restarts = 0;
function stop() {
  const old = browser;
  browser = null;
  if (old && old.exitCode === null) old.kill("SIGKILL");
  const oldProfile = profile;
  setTimeout(() => rmSync(oldProfile, { recursive: true, force: true, maxRetries: 5 }), 2000);
}

function restart(reason) {
  if (state.current !== null) {
    record(state.current, { crash: `${reason} during the test` }, []);
    state.current = null;
  }
  restarts += 1;
  if (restarts > 20) {
    settle({ done: false, reason: "too many browser restarts" });
    return;
  }
  stop();
  launch();
}

// A gap between two ticks is time the machine slept (a laptop), which the
// test and the page's heartbeat don't get charged for.
let lastTick = now();
const watchdog = setInterval(() => {
  const tick = now();
  const slept = tick - lastTick - 1000;
  lastTick = tick;
  if (slept > 4000) {
    state.startedAt += slept;
    if (state.beat) state.beat.at += slept;
    return;
  }
  if (state.current !== null && now() - state.startedAt > options.testTimeout * 1000) {
    const beat = state.beat?.index === state.current ? state.beat : null;
    const diagnosis = beat
      ? `the page answered ${((now() - beat.at) / 1000).toFixed(0)} s ago, after ${beat.waits - beat.startWaits} more waits for the browser`
      : "the page never answered during the test";
    console.error(`test ${state.tests?.[state.current]} timed out after ${options.testTimeout} s; ${diagnosis}`);
    restart(`timed out after ${options.testTimeout} s (${diagnosis})`);
  }
}, 1000);

console.log(`${pageUrl} in ${chrome} ${webgpuFlags.join(" ")}`);
const startedAt = now();
launch();
const outcome = await finished;
closing = true;
clearInterval(watchdog);
stop();
server.close();

// --------------------------------------------------------------------------
// The summary, as runRenderTests prints it.

const counts = { passed: 0, "ignored passed": 0, "ignored failed": 0, failed: 0, errored: 0 };
const failures = [];
let returnCode = 0;
for (const [index, result] of [...state.results.entries()].sort((a, b) => a[0] - b[0])) {
  if (result.crash !== undefined) {
    counts.errored += 1;
    failures.push({ id: ids.get(index) ?? state.tests[index], status: "errored", error: result.crash });
    returnCode = 1;
    continue;
  }
  if (result.code) returnCode = 1;
  for (const test of result.tests ?? []) {
    counts[test.status] = (counts[test.status] ?? 0) + 1;
    if (test.status === "failed" || test.status === "errored") failures.push(test);
  }
}
const total = Object.values(counts).reduce((a, b) => a + b, 0);
const percent = (n) => `${((100 * n) / Math.max(total, 1)).toFixed(1)}%`;
console.log("");
if (counts.passed) console.log(`${counts.passed} passed (${percent(counts.passed)})`);
if (counts["ignored passed"]) console.log(`${counts["ignored passed"]} passed but were ignored (${percent(counts["ignored passed"])})`);
if (counts["ignored failed"]) console.log(`${counts["ignored failed"]} ignored (${percent(counts["ignored failed"])})`);
if (counts.failed) console.log(`${counts.failed} failed (${percent(counts.failed)})`);
if (counts.errored) console.log(`${counts.errored} errored (${percent(counts.errored)})`);
for (const test of failures) console.log(`  ${test.status}: ${test.id}${test.difference !== undefined ? ` (difference ${test.difference.toFixed(6)}, allowed ${test.allowed})` : ""}${test.error ? `: ${test.error}` : ""}`);
if (failures.length) {
  // For an ignore file, once the reasons are known.
  console.log(`\nAs ignore entries (${options.manifest}):`);
  console.log(failures.map((test) => `  ${JSON.stringify(test.id)}: "TODO: reason"`).join(",\n"));
}
console.log(`${state.tests?.length ?? 0} tests in ${((now() - startedAt) / 1000).toFixed(0)} s, ${restarts} browser restarts`);
writeFileSync(
  path.join(options.output, "results.json"),
  JSON.stringify({ counts, results: Object.fromEntries([...state.results].map(([i, r]) => [state.tests[i], r])) }, null, 1),
);
console.log(`Results at: ${options.output}`);

if (!outcome.done) {
  console.error(`run.mjs: the run did not finish${outcome.reason ? `: ${outcome.reason}` : ""}`);
  process.exit(2);
}
process.exit(returnCode);
