#!/usr/bin/env bash
# Build a standalone Freight Fate copy on macOS or Linux.
#
# The Unix counterpart of build-release.ps1: check the prerequisites, install
# the pinned Rust toolchain, then hand over to `cargo tools build-release`,
# which fetches the music pack, builds the
# game, bakes the world, smokes the packaged build and writes the archive to
# dist/. Extra arguments (for example --tag or --cargo-target-dir) pass
# through to the builder.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

need() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "Freight Fate needs $1 before it can build. See README.md under Build a standalone copy." >&2
        exit 1
    fi
}

need git
need rustup
need rustc
need cargo
# Prism is compiled from C++ source through CMake on every platform, and on
# macOS and Linux SDL2 is compiled in from source the same way.
need cmake

case "$(uname -s)" in
    Darwin)
        need codesign
        need otool
        need install_name_tool
        ;;
    Linux)
        need pkg-config
        need readelf
        need file
        # Prism is C++23 and links libstdc++ statically, so the C++ compiler
        # has to be GCC 13 or newer. Ubuntu 22.04 (what the release is built
        # on) ships GCC 11 by default and GCC 13 as gcc-13/g++-13; pick those
        # up automatically, the way the nightly workflow does, unless the
        # caller already chose a compiler.
        if [ -z "${CC:-}" ] && [ -z "${CXX:-}" ]; then
            default_gcc_major="$( (gcc -dumpfullversion -dumpversion 2>/dev/null || echo 0) | cut -d. -f1)"
            if [ "$default_gcc_major" -lt 13 ] 2>/dev/null; then
                if command -v gcc-13 >/dev/null 2>&1 && command -v g++-13 >/dev/null 2>&1; then
                    export CC=gcc-13 CXX=g++-13
                    linker_var="CARGO_TARGET_$(uname -m | tr '[:lower:]' '[:upper:]')_UNKNOWN_LINUX_GNU_LINKER"
                    export "$linker_var=gcc-13"
                else
                    echo "Freight Fate needs GCC 13 or newer to build Prism (found gcc $default_gcc_major). Install gcc-13 and g++-13, or set CC and CXX." >&2
                    exit 1
                fi
            fi
        fi
        ;;
    *)
        echo "build-release.sh supports macOS and Linux; on Windows run build-release.ps1." >&2
        exit 1
        ;;
esac

# rust-toolchain.toml pins the compiler; this installs it (with rustfmt and
# Clippy) when it is not there yet, instead of failing inside cargo.
rustup show >/dev/null

exec cargo tools build-release --smoke "$@"
