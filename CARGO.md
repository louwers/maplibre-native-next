# Cargo instead of Bazel: investigation

Status: investigated 2026-11-05, with a working prototype; no decision yet.
The prototype and its generator are in `target/cargo-investigation/`, which
is not tracked: regenerate it with §6 if it is gone.

## Recommendation: hybrid

Make **Cargo the build for all Rust code**: crates, vendor ports, tools, the
GLFW app, the render/expression suites and the difftests' Rust side. Keep
**Bazel only to build the C++ oracle** (`@maplibre//:mbgl-core`, its vendored
libraries and the four oracle binaries). The difftests link that oracle from a
`build.rs`. Replacing that last Bazel step with the C++ repo's CMake build is
possible later (2–4 days, see §3.2), but nothing forces it.

Why:

- The prototype is generated from the BUILD files with **zero source
  changes**. It builds and runs **117 of 118 test binaries** on macOS, and the
  failing one hangs the same way when built by Bazel. The **full render suite**
  gives the same result as Bazel: 1353 passed, 28 ignored-passed, 79 ignored,
  0 failed, in 76 s.
- Builds are fast and avoid the shared server: **cold 15–20 s** (all
  libraries and binaries), **19–27 s** with all 135 test targets, **0.2 s
  no-op**, **3–8 s** after editing `mln_util`. Each agent can have its own
  target directory (4.1 GB, about 20 s to build cold), so nobody queues on one
  Bazel server (PLAN §8 risk).
- rust-analyzer, `cargo test -p`, clippy, rustfmt and wasm targets work
  natively. `assemble.py` loses its Starlark and lockfile parsing.
- A Cargo-only build would also have to build the C++ core. Cargo cannot
  express that graph. A `cc`-crate rewrite of mbgl-core costs 1–2 weeks and
  needs ongoing maintenance. CMake from `build.rs` is feasible but changes the
  oracle configuration. The difftests also run mainly on Linux CI, where Bazel
  already builds the oracle with a disk cache.

A full switch is a follow-up (CMake oracle). **Staying on Bazel** costs about
2 s per command plus queueing behind other agents. Bazel also needs extra
work for IDE support (`gen_rust_project`) and wasm (`platform/web` lists open
toolchain gaps) that Cargo provides for free.

## 1. The prototype

`target/cargo-investigation/`:

| File | What it does |
|---|---|
| `gen_cargo.py` | Evaluates every `BUILD.bazel` under `crates/ vendor/ tools/ difftest/ platform/glfw` as Python with stub rules (handles `select()`, list comprehensions and `glob`). Writes `ws/`: one package per `rust_library` (63 packages), `[lib] path` / `[[test]] path` / `[[bin]] path` pointing at the tracked sources, `[workspace.dependencies]` from `MODULE.bazel` (features and default-features) and `MODULE.bazel.lock` (versions), and `[patch.crates-io] harfrust` to a copy downloaded from the lock's URL, checked against its sha256 and patched with `crates/mln_text/patches/*.patch`. Reuses `tools/release/assemble.py` (`load_module`, `load_lock`, `resolved_crate`, `toml_value`). `GEN_REPO`/`GEN_WS` point it at a copy. |
| `cc_support.py` | Turns `link_deps` `cc_library` targets into a `build.rs` (the `cc` crate). It maps `@maplibre//vendor:*` to include directories and `@maplibre//:mbgl-core` to the archives Bazel left in `bazel-bin` (link order from rules_rust's params file). |
| `pin_lock.py` | Pins `ws/Cargo.lock` to `MODULE.bazel.lock`. Result: all 271 shared crates have identical versions after one `cargo update --precise` (winnow 1.0.5 → 1.0.4). |
| `run_tests.py` | Runs every test binary in parallel, one process each, with a per-binary wall time. It stands in for cargo-nextest, whose download the sandbox did not allow. |
| `measure.sh` | Cold, no-op, touch and edit timings on `edit-copy/` (`git archive HEAD`, because other agents edit the working tree). Uses `caffeinate` and a monotonic clock (see §2). |
| `ws/.cargo/config.toml` | `[env] TEST_SRCDIR = RUNFILES_DIR = runfiles/`, `TEST_UNDECLARED_OUTPUTS_DIR`, the oracle's `bazel-bin`. |
| `ws/runfiles/` | `maplibre+` → `../maplibre-native` (symlink); `_main` = a Bazel-like forest (real directories along `tools/render-test`, symlinks elsewhere). |
| `ws/.config/nextest.toml` | Bazel `size` → `slow-timeout`; `manual`, `requires-network` and Linux-only → `default-filter`; `--test-threads=1` → `threads-required`. Not exercised (no nextest binary). |

Mapping applied by the generator:

| Bazel | Cargo (generated) |
|---|---|
| `rust_library` | package with `[lib] path = …`, `doctest = false` (Bazel never runs doctests), `test = false` unless a `rust_test(crate = …)` exists |
| `rust_test(crate = :lib, deps = …)` | `[lib] test = true` + `[dev-dependencies]` |
| `rust_test(srcs = …, crate_root = …)` | `[[test]] path = …` in the package of the same-directory library (or in `<pkg>_tests` when there is none). Dev-dependency cycles such as `mln_util`'s tests → `mln_style` → `mln_util` are allowed and work. |
| `use_libtest_harness = False` | `harness = false` |
| `rust_binary`, `crate_features` | `[[bin]]`, `[features] map = []` + `required-features` |
| `aliases = {"@next_crates//:earcut": "earcut_crate"}` | `earcut_crate = { package = "earcut", … }` |
| `tags = ["manual"]`, `["requires-network"]`, `target_compatible_with = linux` | `test = false` on that `[[test]]` (`cargo test --test <name>` still runs it); Linux-only targets are skipped when the generator runs on macOS |
| `args` (render-test `select()`-ed manifest, M1 filter) | generated `gen/tools/render-test/<name>.rs` that compiles the arguments in (`cfg!(target_os)`) and calls `render_test::run_render_tests` |
| `crate.annotation` libz-sys / glfw-sys (build script off, system library) | nothing: their build scripts find the system zlib and Homebrew's `libglfw.3.dylib` themselves |
| `crate.annotation` harfrust patch | `[patch.crates-io]` + a patched copy |
| `@maplibre//…` data deps | the `runfiles/` directory + `[env]` |
| `link_deps` → `cc_library` | `build.rs` (`cc`) + `[build-dependencies] cc` |

## 2. Measurements (this Mac: 16 cores, Apple Silicon, macOS 27)

The machine was shared with other agents' Bazel runs (load average 5–45), and
it went to sleep twice during the first runs (`pmset -g log`: 05:34:00–05:49:03,
903 s). The numbers below come from `caffeinate`d runs with a monotonic clock.
Earlier wall times, such as 2926 s for `cargo test`, included sleep.

| | Cargo | Bazel (reference) |
|---|---|---|
| Cold build, all libraries and binaries (340 units, 180 CPU-s; 141 of them third-party) | **15.3–19.8 s** | not run locally (shared server). CI Linux, cold, `bazel test //...` including the C++ oracle: **22–33 min** (1346–1988 s, critical path 366–391 s) |
| Cold build, libraries + binaries + all 135 test targets (463 units, 323 CPU-s) | **19.3–26.9 s** | CI macOS with disk cache: 328–580 s `Elapsed` for build plus tests (888–1539 disk cache hits of 2456 actions) |
| No-op (`build` / `test --no-run`) | **0.2 s / 0.2–0.5 s** | ~2 s per command on the warm server (local command profiles: 1.7–2.2 s), plus queueing behind other agents |
| `touch crates/mln_util/src/lib.rs` | 3.0–4.2 s (libraries, 23 crates rebuilt), 6.3–7.5 s (with tests) | no rebuild: Bazel uses content digests, Cargo uses mtimes |
| Edit `mln_util` (private fn / public fn) | 3.3–3.4 s libraries; 5.1–7.8 s with tests | not measured; rules_rust recompiles every dependent without rustc incremental (about 180 CPU-s of our crates, so roughly 15–25 s) |
| Edit `mln_renderer` (leaf) | 4.2 s all tests; **1.5 s** for `cargo test -p mln_renderer --lib --no-run` | — |
| `cargo clippy --workspace --all-targets` after a build | 4.3 s | not configured |
| Build output size | 4.1 GB `target/` (2.3 GB `deps`, 1.1 GB incremental) | 3.8 GB `bazel-out` (includes the C++ oracle) |
| Full render suite (`render_tests`) | **76.4 s**, 1353 / 28 / 79 / 0 failed | CI macOS-15: 199–216 s, same counts (PLAN.md 2026-11-04) |
| Render self-check | 22–26 s, 1336 pass | — |
| All test binaries, `cargo test` (sequential) | ~136 s (113 binaries, 1209 passed, 0 failed; excludes the two hangs below) | Bazel runs targets in parallel and caches results |
| All test binaries in parallel (`run_tests.py -j 12`) | wall time ≈ the longest binary, **26 s** (`self_check`); sum 169 s | — |
| C++ oracle (mbgl-core), from ninja logs of the C++ repo's CMake builds | 1450–2960 CPU-s per configuration (Debug), so 2–4 min on 16 cores | CI Linux: inside the 366–391 s critical path |

Other observations:

- Switching between `cargo test -p mln_renderer` and `--workspace` recompiles
  `mln_renderer` and `mln_map` once (2–10 s), because Cargo unifies features
  per selection. A workspace-hack crate (cargo-hakari) fixes this if it ever
  matters.
- Spotlight (`mds_stores`) indexes new `target/` directories. Excluding
  `target/` in System Settings → Spotlight → Privacy avoids that load.

## 3. Concern by concern

### 3.1 Crate graph and third-party dependencies

| | Bazel today | Cargo | Cost / risk |
|---|---|---|---|
| In-repo crates | 50 `rust_library` | one `Cargo.toml` per crate; `members = ["crates/*", "vendor/*", "tools/*", "platform/glfw", "difftest/*"]`, so adding a crate touches no shared file (same as PORTING.md's rule) | low; generated once by `gen_cargo.py` |
| Third-party | 18 `crate.spec` in `MODULE.bazel` | `[workspace.dependencies]` with the same features (identical to the specs `assemble.py` already emits); crates use `x = { workspace = true }` | low |
| Lockfile | `MODULE.bazel.lock` (290 repositories) | `Cargo.lock`; the prototype's matches it for all 271 shared crates | low |
| harfrust patch | `crate.annotation(patches = …, patch_tool = "patch")` | `[patch.crates-io] harfrust = { path = "third_party/harfrust" }` with the patched 0.15.0 source checked in (1.9 MB), or a git fork | low; publishing as `maplibre-native-next-harfrust` stays in `assemble.py` |
| libz-sys / glfw-sys annotations | build scripts off, `third_party/{zlib,glfw}` link the system libraries | not needed: the crates' own build scripts find system zlib (macOS: `-lz`; Linux: pkg-config) and GLFW (`/opt/homebrew/opt/glfw/lib/libglfw.3.dylib`) | low |
| Per-target dependencies | `tools/release/package.toml` `[target-dependencies]` (only used for the release) | `[target.'cfg(…)'.dependencies]` in the crates themselves (the generator emits them) | low |
| Toolchain | rules_rust: rustc 1.98.0, hermetic | `rust-toolchain.toml` with `channel = "1.98.0"` (rustup installs it) | low; less hermetic (system cc/linker, as Bazel on macOS already uses Xcode) |

### 3.2 The C++ oracle (difftests)

The prototype ran all 15 cross-platform difftest binaries on macOS:
`//difftest/util` (string, geo; links Bazel's `libmbgl-core.a`),
`//difftest/text` (ft, glyph_pbf, rasterize, shaping, ucd; mbgl-core,
freetype, harfbuzz), `//difftest/expression:type_difftest`,
`//difftest/nunicode`, and the header-only `//difftest/geometry` tests
(earcut_fixtures, geojson, kdbush, shelf_pack). 14 passed. `pbf_difftest`
hangs in `fixture_pbfs`, and the Bazel-built binary run directly hangs the
same way (>15 min; time goes to C++ exception unwinding in dyld), so it is
environmental.

| Option | How | Effort | Risk |
|---|---|---|---|
| **A. Bazel builds the oracle, Cargo links it** (prototyped) | `bazel build @maplibre//:mbgl-core` plus the oracle `cc_binary`s; each difftest crate's `build.rs` compiles its `reference.cpp` with `cc` and links the archives from `MLN_ORACLE_BAZEL_BIN` | 2–3 days: generalize `cc_support.py` into checked-in `build.rs` files, pass oracle binary paths by env, split packages by link set, style_codegen generation | low: same oracle, same flags. Cost: two build systems; `bazel-bin` flips to `-opt` under `--config=oracle-opt` (use `bazel cquery --output=files` or a stable output path) |
| **B. CMake from `build.rs`** (`cmake` crate on `../maplibre-native/CMakeLists.txt`) | build `mbgl-core` (`MLN_WITH_OPENGL`, HarfBuzz shaping, Debug or Release), link its archives | 2–4 days to match the Bazel oracle's configuration (renderer, defines, `-Wno-error` on newer clang, the Linux platform `impl` for the oracle binaries) | medium: a second oracle configuration that can drift; 2–4 min cold per target dir (use sccache/ccache); needs the Nix shell on NixOS |
| C. `cc` crate builds mbgl-core | reimplement `bazel/core.bzl` file lists, vendored deps (icu, harfbuzz, freetype, sqlite, parsedate, mlt_cpp, fastpfor) and the generated sources | 1–2 weeks, then ongoing upkeep | high: not recommended |
| D. Keep Bazel for `//difftest/...` | Bazel keeps the BUILD files of every crate a difftest needs | ~0 now | two descriptions of the Rust graph that drift; the generator (or crate_universe `from_cargo`) can keep them in sync, at a cost |

### 3.3 Test data and fixture paths

- Bazel: declared `data`, runfiles tree; about 50 helpers join
  `$TEST_SRCDIR|$RUNFILES_DIR` with `maplibre+/…` or `_main/…`
  (`crates/mln_test/src/lib.rs:12` `cpp_path` is the newest one).
- Prototype: zero source changes. `runfiles/` contains `maplibre+ →
  ../maplibre-native` and `_main`, and `.cargo/config.toml [env]` sets the
  variables, so all 52 test targets with `@maplibre` data run unchanged.
- Recommended for a real migration: one helper (`mln_test::cpp_path` and
  `repo_path`) that resolves `MLN_CPP_ROOT` → `$TEST_SRCDIR/maplibre+` →
  `CARGO_MANIFEST_DIR/<up to repo>/../maplibre-native`. Replace the ~50 copies
  with it (about half a day). Then tests run from `cargo test`, rust-analyzer's
  run buttons, and Bazel during the transition.
- Lost: hermetic data. A Cargo test sees the whole checkout, so an undeclared
  data dependency is not caught. Each Bazel test also gets its own
  `TEST_TMPDIR` and outputs directory; Cargo tests fall back to
  `std::env::temp_dir()`.

### 3.4 Per-OS manifests, tags, timeouts, sharding, sandboxing, caching

| Bazel | Cargo equivalent |
|---|---|
| `select()` on `@platforms//os:macos` for the render manifest | `cfg!(target_os = "macos")` in a checked-in `tests/render_tests.rs` (as the generated wrappers do) |
| `args` per test (`--filter …`, `--test-threads=1`) | no per-target arguments in Cargo. Use wrappers for `harness = false` tests; for `mln_actor_gtest`, use nextest (process per test, `threads-required`) or a serializing mutex in the test file |
| `tags = ["manual"]`, `["requires-network"]` | `test = false` (runs only with `--test <name>`) or `#[ignore]`, plus a nextest `default-filter`; CI's `--test_tag_filters=-requires-network` becomes a nextest profile |
| `target_compatible_with = linux` (21 difftest targets) | `#![cfg(target_os = "linux")]` as the first line of those files |
| `size` timeouts | nextest `slow-timeout` (generated `.config/nextest.toml`) |
| sharding | none configured today; nextest `--partition` if needed |
| sandboxing (darwin-sandbox / processwrapper) | none |
| test result caching (`(cached) PASSED`; CI macOS ran 15–41 of 133 tests) | none: every run executes all tests (~170 CPU-s plus the render suite) |
| disk / remote cache (setup-bazel disk cache) | `Swatinem/rust-cache` (target dir keyed by `Cargo.lock`); sccache for a shared cache |

### 3.5 GLFW app, wasm/browser, release crate

- **GLFW**: `mln-glfw` and `glfw_surface_smoke` build with Cargo as is.
  `glfw-sys` with `dynamic-link` links Homebrew's GLFW on macOS and
  `libglfw3-dev` on Linux, so `third_party/glfw` goes away.
- **wasm**: `cargo check --target wasm32-unknown-unknown -p mln_map` fails
  only on `getrandom 0.2` (via ureq → rustls → ring). `ureq` is gated to
  `not(target_os = "emscripten")`, which does not cover
  wasm32-unknown-unknown; the fix is `cfg(not(target_family = "wasm"))`.
  The browser render tests (`tools/web-test`, #4) already build with Cargo:
  `tools/web-test/build.py` generates its own workspace from the BUILD
  files, so a migration replaces two generators. `platform/web`'s manual
  Bazel targets (toolchain gaps for build scripts) become unnecessary.
- **Release crate**: `assemble.py` stays, because crates.io still gets one
  merged crate, but it can read the `Cargo.toml` files instead of Starlark
  and `MODULE.bazel.lock`. Its rewriting, the harfrust fork and `status` are
  unchanged.

### 3.6 CI on GitHub Actions

- Today: Linux cold `bazel test //...` takes 22–33 min; macOS with disk cache
  5.5–10 min. The critical path is the render suite (200–228 s), and fetching
  submodules adds 1–1.5 min.
- With Cargo (estimates scaled from 323 CPU-s cold for 4 vCPU runners):
  - build 2–3 min cold, about 1 min with rust-cache;
  - tests about 3 min, mostly the render suite;
  - Linux difftests: plus the Bazel oracle build with its disk cache (as
    today), or about 10 min cold.
- No test-result caching: unchanged tests still run (~3 min per job).

### 3.7 Developer experience

- rust-analyzer: the workspace is RA's native format. Bazel needs
  `@rules_rust//tools/rust_analyzer:gen_rust_project`, which this repo does
  not set up. Not measured: RA is not installed here.
- `cargo test -p mln_style`, `cargo test -p mln_util --test mln_util_gtest
  <filter>` and `cargo run --bin mln-glfw` work without `scripts/bazel`.
  `-p` builds only that package's graph.
- Incremental: 0.2 s no-op, 1.5–8 s per edit (§2), and no server queue. Each
  agent can set its own `CARGO_TARGET_DIR`. A shared target dir serializes
  through Cargo's lock, just as the Bazel server does.
- clippy and rustfmt are one command each. The code is clean for neither:
  - clippy: 3 deny-level lints (`vendor/pmtiles/src/lib.rs:156` never_loop,
    lib and its tests; `crates/mln_conversion/src/tests/conversion_impl.rs:71`
    almost_swapped) and 422 warnings;
  - rustfmt: 2,600–5,500 diff hunks for `max_width` 100–140.
  This is independent of the build system: Bazel has no clippy or rustfmt
  configured today.

### 3.8 What is lost

- Hermeticity: declared data, sandboxed tests, per-test `TEST_TMPDIR`, a
  pinned toolchain without rustup.
- One graph across Rust and C++, and `bazel test //...` as a single command
  that also builds the oracle.
- Test-result caching and remote action caching; `select()`; per-target
  `args`/`env`/link dependencies. Cargo applies `build.rs` link libraries to
  every target of a package that has no library: in the prototype,
  `pbf_difftest` links mbgl-core because `tile_difftest` in the same package
  does.

## 4. Blockers found (file:line → fix)

| # | Where | Problem | Fix |
|---|---|---|---|
| 1 | `tools/render-test/src/manifest_parser.rs:102-107`; `tools/render-test/manifests/*.json` | `get_valid_path` checks `exists()` on `<manifest dir>/../../../../maplibre+/…` before normalizing. Physically that is `<parent of repo>/maplibre+`, which only exists in Bazel's runfiles | manifests for Cargo that use `../../../../maplibre-native/…` (the real sibling checkout), or a runner `--root`/env override. Prototype: real directories along `_main/tools/render-test` |
| 2 | `tools/render-test/src/tests.rs:19-21` | unit tests use paths relative to Bazel's working directory (`$RUNFILES_DIR/_main`); Cargo runs tests in the package root | join them on the repo root (helper from §3.3). Prototype: the `render_test` package root is `runfiles/_main` |
| 3 | `tools/render-test/src/render_test.rs:311-322` | without `--output` or `TEST_UNDECLARED_OUTPUTS_DIR`, `self_check` fails ("No output directory") | default to `target/render-test-output`. Prototype: `[env]` |
| 4 | `tools/render-test/BUILD.bazel` (`MANIFEST_ARGS`, `M1_FILTER`), `crates/mln_actor/BUILD.bazel` (`--test-threads=1`) | Bazel `args` | checked-in `tests/render_tests.rs` and `tests/m1_render_tests.rs` wrappers; nextest override or a mutex for the actor gtests |
| 5 | `difftest/style_codegen/BUILD.bazel` (`difftest_sources`, `codegen.bzl`) | `generated.cpp`/`cases.rs` come from a Bazel action (excluded from the prototype) | `build.rs` with `style_codegen` as a build-dependency writing to `OUT_DIR`, or check the output in like the layer code |
| 6 | `difftest/layers/layers_difftest.rs:66`, `difftest/storage/storage_difftest.rs:54`, `difftest/sources/sources_difftest.rs:52`, `difftest/expression/expression_difftest.rs:29` | read Bazel-built `cc_binary` oracles from `_main/difftest/<x>/<x>_oracle` (all Linux-only) | path from env (`MLN_<X>_ORACLE`) set from the Bazel/CMake output |
| 7 | 21 Linux-only difftest targets (`target_compatible_with`) | no per-OS test selection in Cargo | `#![cfg(target_os = "linux")]` in each file |
| 8 | `difftest/{tile,…}/BUILD.bazel` | different `link_deps` per test in one package | one Cargo package per distinct C++ reference set |
| 9 | `tools/release/package.toml` `[target-dependencies] ureq` | `ring`/`getrandom 0.2` on wasm32-unknown-unknown | `cfg(not(target_family = "wasm"))` |
| 10 | `vendor/pmtiles/src/lib.rs:156`, `crates/mln_conversion/src/tests/conversion_impl.rs:71` | deny-level clippy lints (only if CI runs clippy) | fix or `#[allow]` |

Not caused by Cargo, but seen during the runs: `pbf_difftest`
`fixture_pbfs` hangs on that Mac, in both builds. (The `mln_storage` and
`gtest_map` failures it also saw at the time are fixed in #5.)

## 5. Migration plan (hybrid)

1. **Manifests** (1 day). Adapt `gen_cargo.py` to write `Cargo.toml` next to
   each `BUILD.bazel`, plus:
   - a root `Cargo.toml` with globbed members and `[workspace.dependencies]`;
   - `[patch.crates-io] harfrust` with the patched source vendored in
     `third_party/harfrust`;
   - `rust-toolchain.toml` with 1.98.0, and `Cargo.lock` pinned with
     `pin_lock.py`.

   Keep Bazel building in parallel and check in CI that both agree.
2. **Test paths** (half a day). One `mln_test` helper (§3.3) and blockers 1–3:
   Cargo manifests for the render runner, and working-directory-independent
   tests.
3. **Arguments and tags** (half a day): blockers 4 and 7, `#[ignore]` or
   `test = false` for `render_tests` and `liberty_style_test`, and a checked-in
   `.config/nextest.toml`.
4. **Difftests** (2–3 days). Option A with `build.rs` per difftest crate
   (blockers 5, 6, 8). Keep `MODULE.bazel` plus a `BUILD.bazel` that only
   builds the oracle archives and binaries. Exclude `difftest/*` from
   `default-members` so `cargo build` needs no oracle.
5. **CI** (half a day). Jobs:
   - `cargo nextest run` on Linux and macOS (Swatinem/rust-cache);
   - Linux difftests: `bazel build` of the oracle with the disk cache, then
     `cargo nextest run -p 'difftest_*'`;
   - release-crate: `assemble.py` reading `Cargo.toml`.
6. **Remove the Rust BUILD files** and `third_party/{zlib,glfw}`. Update
   PLAN §1, PORTING.md ("Build and test", the difftest template) and
   RELEASING.md.

   Optional: replace the remaining Bazel step with CMake (option B).

Total: about 5–6 days, about 3 of them in the difftests.

## 6. Reproduce

```sh
cd target/cargo-investigation
python3 -I gen_cargo.py && python3 -I pin_lock.py
cd ws && export CARGO_TARGET_DIR=$PWD/../target
cargo build --workspace                     # needs bazel-bin's mbgl-core for the difftest libs
cargo test --workspace --exclude difftest_tile_tests
cargo test -p render_test_map --test render_tests     # full suite (manual in Bazel)
cd .. && python3 -I run_tests.py -j 12      # parallel, per-binary timings
./measure.sh cold; ./measure.sh touch; ./measure.sh edit
```
