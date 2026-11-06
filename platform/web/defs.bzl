"""Builds targets for wasm32-unknown-unknown without changing --platforms.

A rule-level transition keeps the shared Bazel server's top-level options
unchanged, so other agents' builds don't lose their analysis cache.
"""

load(":local_probe.bzl", "PROBE_WASM32_AR", "PROBE_WASM32_CC", "PROBE_WASM32_CFLAGS")

_PLATFORMS = "//command_line_option:platforms"
_SHELL = "//command_line_option:shell_executable"
_ACTION_ENV = "//command_line_option:action_env"
_PER_CRATE = "@rules_rust//rust/settings:per_crate_rustc_flag"

def _wasm32_settings():
    return {
        _PLATFORMS: str(Label("//platform/web:wasm32")),
        # There is no C toolchain for wasm32, so rules_rust gives every
        # cargo_build_script placeholder CC/AR tools, made by a run_shell copy
        # that needs --shell_executable (default /bin/bash, which NixOS lacks).
        _SHELL: "/bin/sh",
    }

def _wasm32_transition_impl(_settings, _attr):
    return _wasm32_settings()

_wasm32_transition = transition(
    implementation = _wasm32_transition_impl,
    inputs = [],
    outputs = [_PLATFORMS, _SHELL],
)

def _wasm32_probe_transition_impl(settings, _attr):
    out = _wasm32_settings()

    # cc-rs in build scripts (SQLite for rusqlite's sqlite-wasm-rs, ring)
    # prefers the target-specific variables over rules_rust's CC=no_cc.
    out[_ACTION_ENV] = list(settings[_ACTION_ENV]) + [
        "CC_wasm32_unknown_unknown=" + PROBE_WASM32_CC,
        "AR_wasm32_unknown_unknown=" + PROBE_WASM32_AR,
        "CFLAGS_wasm32_unknown_unknown=" + PROBE_WASM32_CFLAGS,
    ]

    # getrandom has no browser backend unless a feature is enabled
    # (Cargo.toml). For compile checks only, pick backends that need no
    # extra deps: 0.2's "custom" (an extern fn resolved at link time) and
    # 0.4's "unsupported" (returns an error).
    out[_PER_CRATE] = list(settings[_PER_CRATE]) + [
        "external/rules_rust++crate+next_crates__getrandom-0.2.@--cfg=feature=\"custom\"",
        "external/rules_rust++crate+next_crates__getrandom-0.4.@--cfg=getrandom_backend=\"unsupported\"",
    ]
    return out

_wasm32_probe_transition = transition(
    implementation = _wasm32_probe_transition_impl,
    inputs = [_ACTION_ENV, _PER_CRATE],
    outputs = [_PLATFORMS, _SHELL, _ACTION_ENV, _PER_CRATE],
)

def _forward_files_impl(ctx):
    files = depset(transitive = [d[DefaultInfo].files for d in ctx.attr.deps])
    return [DefaultInfo(files = files)]

wasm32_build = rule(
    implementation = _forward_files_impl,
    doc = "Builds `deps` for wasm32-unknown-unknown; the outputs are the deps' default outputs.",
    attrs = {
        "deps": attr.label_list(cfg = _wasm32_transition),
    },
)

wasm32_probe_build = rule(
    implementation = _forward_files_impl,
    doc = """Like wasm32_build, plus a local (non-hermetic) C compiler for
    build scripts and compile-only getrandom backends. For evaluation only.""",
    attrs = {
        "deps": attr.label_list(cfg = _wasm32_probe_transition),
    },
)
