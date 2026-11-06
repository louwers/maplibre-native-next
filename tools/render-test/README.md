# mln-rs-render-test

> **Warning: this is an internal crate of [mln-rs](https://crates.io/crates/mln-rs).**
> It is published only for mln-rs's differential tests against the C++ core
> ([mln-rs-difftests](https://github.com/louwers/mln-rs-difftests)), and its
> API can change in any release without notice.

Ports MapLibre Native's render test runner (`render-test/`), which runs the shared render and query tests.

`cargo test -p mln-rs-render-test --test render_tests` runs them, in one
worker process per CPU (`RENDER_TEST_JOBS=<n>` sets the number; 1 runs them
in the test's own process), and writes the HTML report and the actual and
diff images to `target/tmp/render_tests/`.
`cargo run -p mln-rs-render-test -- --help` lists the runner's options,
such as `--filter` and `--jobs`.

Part of [mln-rs](https://github.com/louwers/mln-rs),
a Rust port of the [MapLibre Native](https://github.com/maplibre/maplibre-native)
map rendering core. All its crates are released together, at the same version.
