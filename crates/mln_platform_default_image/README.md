# mln-rs-platform-default-image

> **Warning: this is an internal crate of [mln-rs](https://crates.io/crates/mln-rs).**
> It is published only because mln-rs depends on it, and its API can change in
> any release without notice. Depend on [`mln-rs`](https://crates.io/crates/mln-rs)
> instead: it re-exports this crate as `mln_rs::mln_platform_default_image`.

Ports the default platform's image codecs (`decodeImage`, `encodePNG`) and the `util::compress` they use.

Part of [maplibre-native-next](https://github.com/louwers/maplibre-native-next),
a Rust port of the [MapLibre Native](https://github.com/maplibre/maplibre-native)
map rendering core. All its crates are released together, at the same version.
