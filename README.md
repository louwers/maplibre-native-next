# maplibre-native-next

[![CI](https://github.com/louwers/maplibre-native-next/actions/workflows/ci.yml/badge.svg)](https://github.com/louwers/maplibre-native-next/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/mln-rs)](https://crates.io/crates/mln-rs)
[![docs.rs](https://img.shields.io/docsrs/mln-rs)](https://docs.rs/mln-rs)

A Rust port of the MapLibre Native core, tested against the C++ core in
`../maplibre-native`. Start with [PLAN.md](PLAN.md); working rules are in
[PORTING.md](PORTING.md).

The tasks are in the [justfile](justfile) (`just` lists them; install
[just](https://github.com/casey/just#readme) with `cargo install just` or
your package manager):

```sh
just test                                    # every test: Bazel (library tests,
                                             # difftests, tools), the render
                                             # test suite, and Cargo
just bazel test //crates/... //vendor/...    # unit tests
just bazel test //difftest/...               # Rust vs. C++
just fmt                                     # format every Rust file

# The GLFW app: OpenFreeMap Liberty in a window (needs a display and the
# system GLFW; wgpu picks Vulkan on Linux and Metal on macOS).
just glfw
```

Bazel always runs through `just bazel`: on NixOS that runs it inside the
C++ repo's `#linux` dev shell.

The tests of the libraries in `crates/` and `vendor/` also run with Cargo.
The difftests and the tools' suites (render and expression tests) need Bazel.

```sh
just test-cargo                              # every library's tests
cargo test -p mln-rs-style                   # one library (package mln-rs-<name>)
cargo test -p mln-rs-util --test mln_util_gtest padding   # one test target, filtered
```

As under Bazel, the tests read the C++ core's test data from
`../maplibre-native`, with its `vendor/` submodules (`.cargo/config.toml`
points them there), and the GPU tests need an adapter: Metal on macOS,
Vulkan on Linux (without a GPU, lavapipe through `VK_DRIVER_FILES`).

CI runs the justfile's `ci-*` recipes: `just ci-lint` checks the
formatting, and runs clippy and rustdoc with warnings as errors.
