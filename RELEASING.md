# Releasing

Bazel builds and tests the port. The root `Cargo.toml` builds the same
libraries with Cargo so they can go to crates.io, all at one version:

- **mln-rs**, the package in the repository root. It depends on every
  library and re-exports each under its crate name: `mln_rs::mln_map`,
  `mln_rs::mln_util`, `mln_rs::earcut`, and so on. Users depend on this one.
- **`mln-rs-<name>`**, one package per library under `crates/` and
  `vendor/` (for example `mln-rs-util` for `mln_util`, `mln-rs-earcut` for
  `earcut`). Their library names are unchanged, so the code still says
  `mln_util::`. They exist for mln-rs only.

The ports nothing in the release uses (`vendor/cheap-ruler`,
`vendor/pixelmatch`) are left out: `exclude` in `Cargo.toml`.

## Cutting a release

1. Bump the version in `Cargo.toml`: `[workspace.package] version` and the
   `=<version>` of every in-repo entry in `[workspace.dependencies]`. A missed
   one fails every cargo command. Run `cargo check` to update `Cargo.lock`,
   and merge that change to main. crates.io never replaces a published
   version.
2. Run the **release** workflow on main: Actions → release → Run workflow, or
   `gh workflow run release.yml --repo louwers/maplibre-native-next --ref main`.

If the workflow fails part way, run it again on the same commit: packages
that crates.io already has at this version are skipped, and so is an existing
GitHub release. If the fix changes a package, bump the version instead.

crates.io limits how many new crates an account can publish in a short time.
The first release after adding libraries runs into it: the workflow waits and
retries until every package is published, up to its 6-hour timeout. Running
it again continues. To avoid the wait, ask help@crates.io to raise the limit
first.

## What the workflow does

`.github/workflows/release.yml`:

1. Checks out this repository. It needs no C++ checkout: the WGSL shaders are
   vendored in `crates/mln_webgpu/src/shaders/wgsl_sources.rs` (see below).
2. Installs stable Rust and runs `cargo package --workspace --locked`. It
   packages every crate and builds each one from its packaged sources. The
   packages resolve each other locally, so this works before any of them is
   on crates.io.
3. Publishes the packages crates.io does not have at this version
   (`cargo publish --workspace --exclude …`), in dependency order. When
   crates.io answers 429, it waits 10 minutes and continues.
4. Creates the tag `v<version>` on the workflow's commit and a GitHub release
   with the `.crate` files.

CI (`release-crate` in `ci.yml`) runs step 2 on every push and PR. It
catches a dependency that is in a `BUILD.bazel` but missing from the
`Cargo.toml` next to it, and a manifest crates.io would reject.

## Locally

```sh
cargo package --workspace
```

On NixOS, run it inside `nix develop path:.nix-shell#linux` if a build script
cannot find a C compiler or zlib.

## Adding a library

A new library under `crates/` or `vendor/` needs, next to its `BUILD.bazel`:

- a `Cargo.toml`: copy one from another library and set the package name
  (`mln-rs-<name>`), `[lib] name` and the dependencies (the same ones as in
  `BUILD.bazel`, as `<crate>.workspace = true`). Its tests: a `[[test]]` per
  `rust_test` with sources, and their extra dependencies in
  `[dev-dependencies]`, in-repo ones by path without a version
  (`mln_style = { package = "mln-rs-style", path = "../mln_style" }`):
  Cargo drops those from the published package, so they may form cycles;
- an entry in `[workspace.dependencies]` in the root `Cargo.toml`;
- a dependency of mln-rs (root `[dependencies]`) and a `pub use` in
  `src/lib.rs`.

A library that should not be released (a test helper) gets
`publish = false` instead of the last two steps. A new
third-party crate goes into `[workspace.dependencies]` as well as
`MODULE.bazel`, with the same features.

## Secrets and settings

- **`CARGO_REGISTRY_TOKEN`** (repository secret): a crates.io API token of
  the account that will own the crates. It needs the `publish-new` and
  `publish-update` scopes, and can be limited to the crates `mln-rs*`.
  crates.io tokens can expire, so renew it when it does. The account needs a
  verified email address to publish. After the first release,
  `cargo owner --add <user>` adds co-owners.
- **Workflow permissions**: the workflow asks for `contents: write` to push
  the tag and create the release. That works with the default repository
  setting (Settings → Actions → General). An organization or enterprise
  policy that caps `GITHUB_TOKEN` at read-only would block it.
- **Tag rules**: a ruleset or tag protection that covers `v*` must let GitHub
  Actions create tags.
- The workflow refuses to run on any branch other than the default branch.

## Shaders

The WGSL shaders come from the C++ core's `include/mln/shaders/webgpu/*.hpp`.
`scripts/vendor-shaders [maplibre-native checkout]` extracts them, unchanged,
into `crates/mln_webgpu/src/shaders/wgsl_sources.rs` (default checkout:
`../maplibre-native`). Rerun it when the C++ shaders change, and commit the
result.
