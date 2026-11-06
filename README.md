# maplibre-native-next

[![CI](https://github.com/louwers/maplibre-native-next/actions/workflows/ci.yml/badge.svg)](https://github.com/louwers/maplibre-native-next/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/mln-rs)](https://crates.io/crates/mln-rs)
[![docs.rs](https://img.shields.io/docsrs/mln-rs)](https://docs.rs/mln-rs)
[![Codecov](https://img.shields.io/codecov/c/github/louwers/maplibre-native-next)](https://app.codecov.io/gh/louwers/maplibre-native-next)

A Rust port of the MapLibre Native core, tested against the C++ core in
`../maplibre-native`. Start with [PLAN.md](PLAN.md); working rules are in
[PORTING.md](PORTING.md).

The tasks are in the [justfile](justfile) (`just` lists them; install
[just](https://github.com/casey/just#readme) with `cargo install just` or
your package manager):

```sh
just test                                    # every test: Bazel (library tests,
                                             # difftests) and Cargo (library
                                             # tests, render, query and
                                             # expression tests)
just bazel test //crates/... //vendor/...    # unit tests
just bazel test //difftest/...               # Rust vs. C++
just fmt                                     # format every Rust file

# The GLFW app: OpenFreeMap Liberty in a window (needs a display and the
# system GLFW; wgpu picks Vulkan on Linux and Metal on macOS).
just glfw
```

Bazel always runs through `just bazel`: on NixOS that runs it inside the
C++ repo's `#linux` dev shell.

The tests of the libraries in `crates/` and `vendor/` also run with Cargo,
and the shared C++ test suites run only with Cargo: the render and query
tests (`tools/render-test`) and the expression tests
(`tools/expression-test`). The difftests need Bazel.

```sh
just test-cargo                              # every Cargo test
cargo test -p mln-rs-style                   # one library (package mln-rs-<name>)
cargo test -p mln-rs-util --test mln_util_gtest padding   # one test target, filtered
cargo test -p mln-rs-render-test --test render_tests      # the render and query tests
cargo test -p mln-rs-expression-test         # the expression tests
just coverage                                # their coverage, as HTML (CI
                                             # uploads it to Codecov)
```

The render suite writes its report and the actual and diff images to
`target/tmp/render_tests/`. `cargo run -p mln-rs-render-test -- --help`
runs the runner itself, e.g. with a `--filter`.

As under Bazel, the tests read the C++ core's test data from
`../maplibre-native`, with its `vendor/` submodules (`.cargo/config.toml`
points them there), and the GPU tests need an adapter: Metal on macOS,
Vulkan on Linux (without a GPU, lavapipe through `VK_DRIVER_FILES`).

CI runs the justfile's `ci-*` recipes: `just ci-lint` checks the
formatting, and runs clippy and rustdoc with warnings as errors.
