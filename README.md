# maplibre-native-next

[![CI](https://github.com/louwers/maplibre-native-next/actions/workflows/ci.yml/badge.svg)](https://github.com/louwers/maplibre-native-next/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/mln-rs)](https://crates.io/crates/mln-rs)
[![docs.rs](https://img.shields.io/docsrs/mln-rs)](https://docs.rs/mln-rs)
[![Codecov](https://img.shields.io/codecov/c/github/louwers/maplibre-native-next)](https://app.codecov.io/gh/louwers/maplibre-native-next)

A Rust port of the MapLibre Native core. Start with [PLAN.md](PLAN.md);
working rules are in [PORTING.md](PORTING.md). The differential tests
against the C++ core (`//difftest/<area>` in the docs and comments) are in
[mln-rs-difftests](https://github.com/louwers/mln-rs-difftests), which tests
the released crates.

The tasks are in the [justfile](justfile) (`just` lists them; install
[just](https://github.com/casey/just#readme) with `cargo install just` or
your package manager):

```sh
just test                                    # every test
cargo test -p mln-rs-style                   # one library (package mln-rs-<name>)
cargo test -p mln-rs-util --test mln_util_gtest padding   # one test target, filtered
cargo test -p mln-rs-render-test --test render_tests      # the render and query tests
cargo test -p mln-rs-expression-test         # the expression tests
just coverage                                # their coverage, as HTML (CI
                                             # uploads it to Codecov)
just fmt                                     # format every Rust file

# The GLFW app: OpenFreeMap Liberty in a window (needs a display and the
# system GLFW; wgpu picks Vulkan on Linux and Metal on macOS).
just glfw
```

The render suite writes its report and the actual and diff images to
`target/tmp/render_tests/`. `cargo run -p mln-rs-render-test -- --help`
runs the runner itself, e.g. with a `--filter`.

## Test data

The tests read files of the C++ repository (fixtures, the render, query and expression suites, the style spec) from copies in this repository: each package's `testdata/` holds the files its tests read, at their paths in maplibre-native. They were copied from [johncarmack1984/maplibre-native@d171bf4](https://github.com/johncarmack1984/maplibre-native/commit/d171bf4b99343a27d0bfebf9ddf1ae79e5a616aa) (the globe and sky branches merged into [louwers/maplibre-native@4e03abe](https://github.com/louwers/maplibre-native/commit/4e03abe94cf562b10562c528578d109c7273694d), which is maplibre/maplibre-native 8e144d0 plus build changes), under its licenses. `just update-testdata [checkout]` copies them again from a maplibre-native checkout (by default `../maplibre-native`); the render, query and expression suites are copied whole, so new tests come along.

The GPU tests need an adapter: Metal on macOS, Vulkan on Linux (without a
GPU, lavapipe through `VK_DRIVER_FILES`). On NixOS, the justfile runs the
tests and the apps inside the C++ repo's `#linux` dev shell.

CI runs the justfile's `ci-*` recipes: `just ci-lint` checks the
formatting, and runs clippy and rustdoc with warnings as errors.
