# maplibre-native-next

A Rust port of the MapLibre Native core, tested against the C++ core in
`../maplibre-native`. Start with [PLAN.md](PLAN.md); working rules are in
[PORTING.md](PORTING.md).

```sh
scripts/bazel test //crates/... //vendor/...     # unit tests
scripts/bazel test //difftest/...                # Rust vs. C++

# The GLFW app: OpenFreeMap Liberty in a window (needs a display and the
# system GLFW; wgpu picks Vulkan on Linux and Metal on macOS).
scripts/bazel run //platform/glfw:mln-glfw
```

The tests of the libraries in `crates/` and `vendor/` also run with Cargo.
The difftests and the tools' suites (render and expression tests) need Bazel.

```sh
cargo test --workspace                       # every library's tests
cargo test -p mln-rs-style                   # one library (package mln-rs-<name>)
cargo test -p mln-rs-util --test mln_util_gtest padding   # one test target, filtered
```

As under Bazel, the tests read the C++ core's test data from
`../maplibre-native`, with its `vendor/` submodules (`.cargo/config.toml`
points them there), and the GPU tests need an adapter: Metal on macOS,
Vulkan on Linux (without a GPU, lavapipe through `VK_DRIVER_FILES`).
