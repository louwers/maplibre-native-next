# mln-rs-libstdcxx

> **Warning: this is an internal crate of [mln-rs](https://crates.io/crates/mln-rs).**
> It is published only because mln-rs depends on it, and its API can change in
> any release without notice. Depend on [`mln-rs`](https://crates.io/crates/mln-rs)
> instead: it re-exports this crate as `mln_rs::libstdcxx`.

Ports the libstdc++ (GCC 16) algorithms whose results the C++ code depends on beyond what the C++ standard specifies: `std::sort`, `std::stable_sort`, the heap functions and `std::priority_queue`.

Part of [maplibre-native-next](https://github.com/louwers/maplibre-native-next),
a Rust port of the [MapLibre Native](https://github.com/maplibre/maplibre-native)
map rendering core. All its crates are released together, at the same version.
