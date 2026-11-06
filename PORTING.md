# Porting rules

Read `PLAN.md` first. The C++ source is in `../maplibre-native` (read-only for
everyone working in this repo).

## Porting

- **Port, don't redesign.** One Rust module per C++ file where practical, same
  type names (`RenderBackgroundLayer`, `TransformState`), methods in
  snake_case (`getZoom` → `get_zoom`), same algorithms in the same order.
- Start every file with the C++ file(s) it ports:
  `// Ports src/mln/renderer/layers/render_background_layer.cpp`.
- **Match C++ where it is observable** (render, query and expression tests
  with the C++ baselines; public API behaviour). Keep float types
  (`float` → `f32`) and the order of float operations, because they decide
  pixels. Port quirks that change those outcomes and comment them.
- **Don't reproduce** undefined behaviour, libstdc++ internals (sort tie
  order, container iteration order) or x86-64-specific conversion results,
  unless a test outcome depends on it. Record differences in `DEVIATIONS.md`.
- **Prefer a crate** when a comparison against the C++ (a difftest, run once)
  shows it doesn't change observable outcomes; port only when it does or
  when there is no crate. Ask the coordinator for new crates in
  `COORDINATION.md`.
- Difftests are a tool, not a gate for every function: write them where a
  crate or a port is in doubt, or to debug a render-test difference.
- Port only what the current milestone needs, but port those code paths
  completely. For a C++ branch you skip, leave a `// TODO(port): <what>`
  comment. Don't add `todo!()` on paths that run.
- No `unsafe` outside FFI (`//difftest`) and GPU buffer casts (use `bytemuck`).
- Errors: where C++ throws, return `Result` with the same message; where C++
  returns `optional`/`expected`, use `Option`/`Result`.

## Build and test

- Bazel only, always through `just bazel` (for example
  `just bazel test //crates/mln_style/...`). Wrap long commands in `timeout`.
  `just` lists the other tasks (justfile).
- **One Bazel server is shared by everyone.** Commands queue on it, so:
  - build and test only your own targets, plus targets you depend on;
  - never build `//...` (other people's packages may be mid-edit);
  - never `clean`, `shutdown`, `sync`, or pass startup flags, `--output_base`,
    or new `--config`s. Only `--config=oracle-opt` exists.
- `cargo test --workspace` runs the same library tests without the shared
  server (README.md); the difftests and the tools' suites are Bazel only.
  Every library also has a `Cargo.toml` (the crates.io release). When you
  change a library's dependencies or tests in `BUILD.bazel`, change its
  `Cargo.toml` the same way: `[dependencies]`, a `[[test]]` per `rust_test`
  with sources, test-only dependencies in `[dev-dependencies]`. A new
  library needs the steps in RELEASING.md ("Adding a library"). CI's
  `release-crate` job checks the manifests.
- Format with `just fmt` (rustfmt, `rustfmt.toml`); leave generated
  files ("Do not edit") as their generator writes them, and put
  `#[rustfmt::skip]` on a new one's `mod` declaration. CI's `lint` job also
  runs clippy (`cargo clippy --workspace --all-targets`) and rustdoc with
  warnings as errors. The root `Cargo.toml` allows the clippy lints whose
  fixes would make the port read or behave less like the C++; for another
  such case, `#[allow]` it where it occurs and say why.
- Third-party crates available as `@next_crates//:<name>`: the ones in the
  root `Cargo.toml`'s `[workspace.dependencies]` (with their features) and,
  for Bazel-only targets, in `third_party/bazel_crates/Cargo.toml`
  (`earcut`, `glfw`, `proptest`, `serde`). If you need another one, ask in
  `COORDINATION.md`; don't add it yourself.
- Package pattern:

  ```starlark
  load("@rules_rust//rust:defs.bzl", "rust_library", "rust_test")

  # Ports src/mln/<module>.
  rust_library(
      name = "mln_x",
      srcs = glob(["src/**/*.rs"]),
      visibility = ["//visibility:public"],
      deps = ["//crates/mln_util"],
  )

  rust_test(
      name = "mln_x_test",
      crate = ":mln_x",
  )
  ```

- Shared C++ test data as Bazel data deps:
  `data = ["@maplibre//metrics:render-test-files"]` (and
  `:expression-test-files`); find them at runtime through the runfiles
  directory (`RUNFILES_DIR`/`TEST_SRCDIR`, repository directory
  `maplibre+`). Under `cargo test`, `TEST_SRCDIR` is `.cargo/runfiles`,
  whose `maplibre+` links to `../maplibre-native`.
- GPU tests: `user.bazelrc` passes `LD_LIBRARY_PATH` (Vulkan loader) and
  `VK_DRIVER_FILES` (lavapipe) to tests.

## Differential tests (`//difftest/<area>`)

Rust must behave exactly like C++. For a ported function, add a difftest:

```starlark
cc_library(
    name = "reference",
    srcs = ["reference.cpp"],          # extern "C" wrappers around C++ calls
    copts = ["-std=c++20"],
    deps = ["@maplibre//:mbgl-core"],
)

rust_test(
    name = "util_difftest",
    srcs = ["util_difftest.rs"],
    link_deps = [":reference"],
    deps = [
        "//crates/mln_util",
        "@next_crates//:proptest",
    ],
)
```

- Pass POD values and `(ptr, len)` byte strings across the boundary, or JSON
  for structured values. C++ exceptions are caught in the wrapper and returned
  as error strings.
- Inputs: proptest strategies plus a hand-written list of edge cases
  (±0, NaN, ±inf, subnormals, boundaries, malformed strings).
- Compare bit-exactly (`to_bits`); any NaN equals any NaN.
- The oracle is built at `-c fastbuild`, so C++ `assert`s are live. Don't
  generate inputs that violate a C++ precondition; note the case instead.
- Also run them once with `--config=oracle-opt`.
- A comparison pinned to the Linux oracle (glibc libm/strtod, libstdc++
  ordering and hashing, x86-64 conversions, arm64 FMA contraction, or an
  oracle linking `@maplibre//platform/linux`) gets
  `target_compatible_with = ["@platforms//os:linux"]`, so wildcard runs skip
  it on macOS; leave genuinely cross-platform comparisons unrestricted — they
  run on macOS CI too.

## Ownership

Each agent edits only its own directories (see the PLAN.md workstream table).
Exceptions:

- `crates/mln_util/src`: any agent may **add** a new file for a C++ `util/`
  file it needs (for example `mat4.rs`) and append one `pub mod` /
  `pub use` line to `lib.rs`. Existing files belong to `expression`; ask in
  `COORDINATION.md` for changes to them. New files must not need new deps in
  `crates/mln_util/BUILD.bazel` without asking first.
- `COORDINATION.md`: everyone appends; never rewrite others' entries.
- Shared files (`MODULE.bazel`, `.bazelrc`, `user.bazelrc`, `justfile`,
  `PLAN.md`, `PORTING.md`, the root `BUILD.bazel`, `../maplibre-native`)
  belong to the coordinator. Request changes in `COORDINATION.md`.
- Don't commit. Others are editing the same working tree.

## Interfaces for M1

The M1 crates are written at the same time against each other's C++-shaped
APIs. These are the agreed shapes. Owners land compiling skeletons of them
early, so dependents can build, then fill them in.

- `mln_util::image` (runner): `PremultipliedImage` and `UnassociatedImage` as
  in `util/image.hpp` (`size: Size { width, height }`, `data: Vec<u8>` RGBA),
  plus PNG encode/decode as in `util/png_writer`/`png_reader`.
- `mln_util::size` (render): `Size { width: u32, height: u32 }`.
- `mln_platform_default` (render): `HeadlessFrontend::new(size, pixel_ratio)`,
  `HeadlessFrontend::render(&mut self, map: &mut Map) -> RenderResult`
  (`RenderResult { image: PremultipliedImage }`), as in
  `platform/default/include/mln/gfx/headless_frontend.hpp`.
- `mln_map` (render): `Map::new(frontend, observer, MapOptions)` with
  `MapOptions` builder methods as in `map_options.hpp` (`with_map_mode`,
  `with_size`, `with_pixel_ratio`), `Map::get_style_mut().load_json(&str)`,
  `Map::jump_to(&CameraOptions)`, `MapMode::{Continuous, Static, Tile}`.
- `mln_style` (style): `Style`, `style::Impl` holding
  `Vec<Immutable<layer::Impl>>` (`Immutable<T>` = `Arc<T>`), `BackgroundLayer`
  and its `Impl` with generated-style `BackgroundPaintProperties`
  (`Unevaluated`, `PossiblyEvaluated`, `Evaluated`), `Transitionable` →
  `Transitioning` → `PossiblyEvaluated` as in `style/properties.hpp`,
  `PropertyEvaluationParameters`, `TransitionOptions`. The renderer calls them
  as `render_background_layer.cpp` does.
- `mln_conversion` (expression): `Convertible` as in `style/conversion.hpp`
  and `conversion_impl.hpp` (`is_undefined`, `is_array`, `array_length`,
  `array_member`, `is_object`, `object_member`, `each_member`, `to_bool`,
  `to_number`, `to_double`, `to_string`, `to_value`), implemented for
  `serde_json::Value`, plus `conversion::Error { message }`.
- `mln_expression` (expression): `Expression` trait, `ParsingContext`,
  `ParseResult`, `type_::Type`, `EvaluationContext`, `EvaluationResult`,
  `Value`, `dsl` as in `style/expression/*`. `PropertyExpression<T>` and the
  legacy function conversion (`style/conversion/function.cpp`) are ported in
  `mln_style` on top of these.
- `mln_gfx` (gfx): the `gfx/` abstractions `render_background_layer.cpp` and
  `background_layer_tweaker.cpp` use on the drawable path: `Context`,
  `RendererBackend`, `HeadlessBackend`, `CommandEncoder`, `RenderPass`,
  `Drawable`, `DrawableBuilder`, `LayerGroup`/`TileLayerGroup`,
  `UniformBuffer(Array)`, `ShaderRegistry`/`ShaderProgramBase`, offscreen
  readback to `PremultipliedImage`. `mln_webgpu` implements them on `wgpu`.

When an interface needs to change, write the change in `COORDINATION.md`
before making it.
