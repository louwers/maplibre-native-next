# mln-rs-wgpu-webgpu-h

> **Warning: this is an internal crate of [mln-rs](https://crates.io/crates/mln-rs).**
> It is published only because mln-rs depends on it, and its API can change in
> any release without notice. Depend on [`mln-rs`](https://crates.io/crates/mln-rs)
> instead: it re-exports this crate as `mln_rs::mln_wgpu_webgpu_h`.

A `wgpu` backend over the WebGPU C API (`webgpu.h`) of Dawn's emdawnwebgpu port, so that wgpu renders with the browser's WebGPU in an Emscripten module.

Part of [maplibre-native-next](https://github.com/louwers/maplibre-native-next),
a Rust port of the [MapLibre Native](https://github.com/maplibre/maplibre-native)
map rendering core. All its crates are released together, at the same version.
