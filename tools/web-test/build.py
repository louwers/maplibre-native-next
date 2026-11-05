#!/usr/bin/env python3
"""Builds the render-test runner for the browser: wasm32-unknown-unknown with
wgpu's own WebGPU backend, through wasm-bindgen (no Emscripten).

Run with `python3 -I tools/web-test/build.py` (tools/web-test/run.mjs runs it
first unless given --no-build). It writes, under target/web-test:

  ws/                  a Cargo workspace generated from the BUILD.bazel files:
                       one package per rust_library of crates/, vendor/ and
                       tools/render-test (and the patched harfrust fork from
                       tools/release/assemble.py), each pointing at the
                       tracked sources, plus `web_test` (tools/web-test/src).
                       tools/web-test/Cargo.lock seeds its lockfile.
  pkg/                 web_test.js and web_test_bg.wasm (wasm-bindgen --target web)
  tools/bin/           the wasm-bindgen CLI of the version Cargo.lock pins

Third-party crates get the version MODULE.bazel.lock resolved and the
features of their crate.spec, as in the release crate. Crates that only the
browser build needs are listed in WASM_DEPS below; the code uses them under
`cfg(all(target_family = "wasm", target_os = "unknown"))` only, so the Bazel
builds never see them.

Needs: Rust with the wasm32-unknown-unknown target and the llvm-tools
component (llvm-ar; added if missing), a clang that targets wasm32 (SQLite,
bundled by rusqlite through sqlite-wasm-rs, and ring; Apple clang and
Ubuntu's clang do), Python 3.11+, GNU patch (the harfrust fork), and
network access to crates.io on the first run.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
HERE = REPO / "tools/web-test"
OUT = REPO / "target/web-test"
WS = OUT / "ws"
TARGET = "wasm32-unknown-unknown"
WEB = 'cfg(all(target_family = "wasm", target_os = "unknown"))'

sys.path.insert(0, str(REPO / "tools/release"))
import assemble  # noqa: E402  (BUILD/MODULE.bazel parsing, the harfrust fork)

# Packages whose rust_library targets become Cargo packages.
LIBRARY_PACKAGES = ["crates/*", "vendor/*", "tools/render-test"]

# Dependencies of the browser build only, per library (Cargo dependency
# tables, under the WEB target cfg).
WASM_DEPS = {
    # std::time's clocks panic on wasm32-unknown-unknown (mln_util::time).
    "mln_util": {"web-time": {"version": "1"}},
}

# In-repo dependencies the browser build leaves out: (library, dependency).
NATIVE_ONLY = {
    # webgpu.h for Emscripten; mln_webgpu uses it under
    # cfg(target_os = "emscripten") only.
    ("mln_webgpu", "mln_wgpu_webgpu_h"),
}

# Third-party crates whose features differ in the browser build (Cargo
# dependency overrides under the WEB target cfg).
WASM_OVERRIDES = {
    # zlib (libz-sys) needs a C library that wasm32-unknown-unknown lacks.
    "flate2": {"default-features": False, "features": ["rust_backend"]},
    # wgpu's WebGPU backend only (no WebGL, no Emscripten `custom`). Its
    # objects are not Send there unless the build says it has no threads.
    "wgpu": {
        "default-features": False,
        "features": ["fragile-send-sync-non-atomic-wasm", "parking_lot", "std", "webgpu", "wgsl"],
    },
}

# The entry crate (tools/web-test/src/lib.rs).
ENTRY_LOCAL_DEPS = [
    "mln_util",
    "render_test",
    "render_test_map",
]
ENTRY_DEPS = {
    "serde_json": {},
    "js-sys": {"version": "0.3"},
    "wasm-bindgen": {"version": "0.2"},
    "wasm-bindgen-futures": {"version": "0.4"},
    "web-sys": {
        "version": "0.3",
        "features": ["RequestInit", "Response", "Window"],
    },
    # rusqlite's bundled SQLite on wasm32-unknown-unknown: its host functions
    # (clock, randomness) through wasm-bindgen, and the memory VFS the cache
    # database is imported into.
    "sqlite-wasm-rs": {"version": "0.6", "features": ["wasm-bindgen"]},
    # mln_storage's HTTP file source (ureq, rustls, ring) compiles for the
    # browser once its crates are told they run there: randomness and the
    # clock from JavaScript. It has no sockets there; the render tests run
    # offline and never use it.
    "getrandom-02": {"package": "getrandom", "version": "0.2", "features": ["js"]},
    "ring": {"version": "0.17", "features": ["wasm32_unknown_unknown_js"]},
    "rustls-pki-types": {"version": "1", "features": ["web"]},
}


def fail(msg):
    sys.exit(f"build.py: error: {msg}")


def toml_dep(spec):
    return assemble.toml_value(spec)


class Library:
    def __init__(self, name, package, root):
        self.name = name
        self.package = package
        self.root = root  # crate root, relative to REPO
        self.local = []  # (package, target) labels
        self.external = []  # @next_crates names


def load_libraries():
    libs = {}
    for pattern in LIBRARY_PACKAGES:
        for build in sorted(REPO.glob(f"{pattern}/BUILD.bazel")):
            package = str(build.parent.relative_to(REPO))
            constants = assemble.top_level_constants(build)
            for kind, call in assemble.calls(build):
                if kind != "rust_library":
                    continue
                kw = assemble.kwargs(call, constants)
                if kw.get("testonly") is True:
                    continue
                target = kw["name"]
                root = kw.get("crate_root", "src/lib.rs")
                lib = Library(kw.get("crate_name", target.replace("-", "_")), package, f"{package}/{root}")
                for dep in kw.get("deps", []):
                    if dep.startswith(f"@{assemble.CRATES_REPO}//:"):
                        lib.external.append(dep.split(":", 1)[1])
                    else:
                        lib.local.append(assemble.parse_label(dep, package))
                libs[(package, target)] = lib
    by_label = {key: lib for key, lib in libs.items()}
    for lib in libs.values():
        lib.local = [by_label[key].name if key in by_label else fail(f"//{lib.package}: unknown dependency {key}") for key in lib.local]
    return {lib.name: lib for lib in libs.values()}


def write_if_changed(path, text):
    """Leaves unchanged files alone, so Cargo's fingerprints stay valid."""
    if not path.exists() or path.read_text() != text:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)


def fork(crate, attrs, version, fork_cfg, package_name, out):
    """assemble.build_fork, unless its output is there for the same
    upstream version and patches (rebuilding it would touch every file)."""
    patches = [assemble.parse_label(label.removeprefix("@@"), "") for label in attrs.get("patches", [])]
    key = "\n".join([version, fork_cfg["version"], *((REPO / pkg / target).read_text() for pkg, target in patches)])
    name = f"{package_name}-{crate}"
    stamp = out / name / ".web-test-stamp"
    if stamp.exists() and stamp.read_text() == key:
        return name, fork_cfg["version"]
    result = assemble.build_fork(crate, attrs, version, fork_cfg, package_name, out)
    stamp.write_text(key)
    return result


def closure(libs, roots):
    seen, stack = set(), list(roots)
    while stack:
        name = stack.pop()
        if name in seen:
            continue
        seen.add(name)
        stack += libs[name].local
    return sorted(seen)


def generate():
    libs = load_libraries()
    specs, patched, edition = assemble.load_module()
    lock = assemble.load_lock()
    config = tomllib.loads(assemble.CONFIG.read_text())
    names = closure(libs, ENTRY_LOCAL_DEPS)

    crates_dir = WS / "crates"
    crates_dir.mkdir(parents=True, exist_ok=True)

    external = sorted({dep for name in names for dep in libs[name].external})
    forks = {}
    third_party = {}
    for dep in external:
        if dep not in specs:
            fail(f"@{assemble.CRATES_REPO}//:{dep} has no crate.spec in MODULE.bazel")
        spec = specs[dep]
        version, attrs = assemble.resolved_crate(lock, dep)
        entry = {"version": version}
        if spec.get("default_features") is False:
            entry["default-features"] = False
        if spec.get("features"):
            entry["features"] = sorted(spec["features"])
        if dep in patched:
            fork_cfg = config["forks"][dep]
            fork_name, fork_version = fork(dep, attrs, version, fork_cfg, config["package"]["name"], crates_dir)
            forks[dep] = fork_name
            entry = {"package": fork_name, "version": fork_version, "path": f"../{fork_name}", **{k: v for k, v in entry.items() if k != "version"}}
        third_party[dep] = entry

    def rel(path):
        return os.path.relpath(REPO / path, crates_dir / "x")

    def manifest(name, lib_path, local, extern, wasm, crate_type=None):
        lines = [
            "# Generated by tools/web-test/build.py.",
            "[package]",
            f'name = "{name}"',
            'version = "0.0.0"',
            f'edition = "{edition}"',
            "publish = false",
            "",
            "[lib]",
            f'path = "{lib_path}"',
        ]
        if crate_type:
            lines.append(f"crate-type = {toml_dep(crate_type)}")
        lines += ["", "[dependencies]"]
        native, web = {}, {}
        for dep in sorted(set(local)):
            if (name, dep) in NATIVE_ONLY:
                native[dep] = {"path": f"../{dep}"}
            else:
                lines.append(f'{dep} = {{ path = "../{dep}" }}')
        for dep in sorted(set(extern)):
            entry = third_party[dep]
            if dep in WASM_OVERRIDES:
                native[dep] = entry
                base = {k: v for k, v in entry.items() if k not in ("features", "default-features")}
                web[dep] = base | WASM_OVERRIDES[dep]
            else:
                lines.append(f"{assemble.toml_key(dep)} = {toml_dep(entry)}")
        for dep, extra in wasm.items():
            web[dep] = third_party.get(dep, {}) | extra if dep not in web else web[dep] | extra
        if native:
            lines += ["", "[target.'cfg(not(all(target_family = \"wasm\", target_os = \"unknown\")))'.dependencies]"]
            lines += [f"{assemble.toml_key(k)} = {toml_dep(v)}" for k, v in native.items()]
        if web:
            lines += ["", f"[target.'{WEB}'.dependencies]"]
            lines += [f"{assemble.toml_key(k)} = {toml_dep(v)}" for k, v in sorted(web.items())]
        return "\n".join(lines) + "\n"

    members = sorted([*names, "web_test", *forks.values()])
    for stale in crates_dir.iterdir():
        # Libraries that are gone; download/ caches the forks' crates.
        if stale.name not in members and stale.name != "download":
            shutil.rmtree(stale)
    for name in names:
        lib = libs[name]
        path = crates_dir / name / "Cargo.toml"
        path.parent.mkdir(parents=True, exist_ok=True)
        write_if_changed(path, manifest(name, rel(lib.root), lib.local, lib.external, WASM_DEPS.get(name, {})))

    entry = crates_dir / "web_test" / "Cargo.toml"
    entry.parent.mkdir(parents=True, exist_ok=True)
    write_if_changed(
        entry, manifest("web_test", rel("tools/web-test/src/lib.rs"), ENTRY_LOCAL_DEPS, [], ENTRY_DEPS, ["cdylib"])
    )

    workspace = [
        "# Generated by tools/web-test/build.py.",
        "[workspace]",
        'resolver = "3"',
        f"members = {toml_dep(['crates/' + m for m in members])}",
        "",
        # Optimized, but with the checks of Bazel's default fastbuild (debug
        # assertions, overflow checks), so a test panics where it does natively.
        "[profile.release]",
        "opt-level = 2",
        "debug-assertions = true",
        "overflow-checks = true",
        "codegen-units = 16",
        "incremental = true",
    ]
    write_if_changed(WS / "Cargo.toml", "\n".join(workspace) + "\n")

    lock_src = HERE / "Cargo.lock"
    lock_dst = WS / "Cargo.lock"
    if lock_src.exists() and (not lock_dst.exists() or lock_dst.read_bytes() != lock_src.read_bytes()):
        shutil.copy(lock_src, lock_dst)
    print(f"{WS}: {len(names)} libraries, {len(external)} third-party crates", file=sys.stderr)


def locked_version(package):
    lock = tomllib.loads((WS / "Cargo.lock").read_text())
    versions = {p["version"] for p in lock["package"] if p["name"] == package}
    if len(versions) != 1:
        fail(f"Cargo.lock: expected one {package}, found {sorted(versions)}")
    return versions.pop()


def wasm_bindgen_cli(version):
    root = OUT / "tools"
    exe = root / "bin" / "wasm-bindgen"
    if exe.exists():
        out = subprocess.run([exe, "--version"], capture_output=True, text=True).stdout.split()
        if out[-1:] == [version]:
            return exe
    subprocess.run(
        ["cargo", "install", "wasm-bindgen-cli", "--version", version, "--locked", "--root", str(root), "--force"],
        check=True,
    )
    return exe


def llvm_ar():
    """llvm-ar from rustup's llvm-tools: the C libraries (SQLite, ring) need
    an archiver that writes a symbol index for wasm objects, which Apple's
    and GNU ar don't."""
    sysroot = subprocess.run(["rustc", "--print", "sysroot"], capture_output=True, text=True, check=True).stdout.strip()
    host = re.search(r"^host: (.+)$", subprocess.run(["rustc", "-vV"], capture_output=True, text=True, check=True).stdout, re.M).group(1)
    exe = Path(sysroot) / "lib/rustlib" / host / "bin/llvm-ar"
    if not exe.exists():
        subprocess.run(["rustup", "component", "add", "llvm-tools"], check=True)
    return exe


def build(args):
    env = dict(os.environ)
    env.setdefault("CARGO_TARGET_DIR", str(OUT / "cargo"))
    env.setdefault("AR_wasm32_unknown_unknown", str(llvm_ar()))
    env.setdefault("CC_wasm32_unknown_unknown", "clang")
    cmd = ["cargo", "build", "--release", "--target", TARGET, "-p", "web_test"]
    if args.locked:
        cmd.append("--locked")
    subprocess.run(cmd, cwd=WS, env=env, check=True)
    if args.update_lock:
        shutil.copy(WS / "Cargo.lock", HERE / "Cargo.lock")
    wasm = Path(env["CARGO_TARGET_DIR"]) / TARGET / "release" / "web_test.wasm"
    exe = wasm_bindgen_cli(locked_version("wasm-bindgen"))
    pkg = OUT / "pkg"
    subprocess.run([exe, "--target", "web", "--out-dir", pkg, "--out-name", "web_test", wasm], check=True)
    print(f"{pkg}: {(pkg / 'web_test_bg.wasm').stat().st_size / 1e6:.1f} MB", file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--generate-only", action="store_true", help="only write the Cargo workspace")
    parser.add_argument("--locked", action="store_true", help="fail if Cargo.lock needs changes")
    parser.add_argument("--update-lock", action="store_true", help="copy the resolved Cargo.lock back to tools/web-test")
    args = parser.parse_args()
    generate()
    if not args.generate_only:
        build(args)


if __name__ == "__main__":
    main()
