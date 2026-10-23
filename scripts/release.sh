#!/usr/bin/env bash
# Build release artifacts for the wlel compiler.
#
#   scripts/release.sh                 # host target only
#   scripts/release.sh TARGET [...]    # one or more Rust targets
#
# Each target produces dist/wlel-<version>-<target>.tar.gz containing the
# compiler binary and README.md, plus a .sha256 next to it. Cross targets
# need their Rust std installed (`rustup target add <target>`) and a linker
# for that target (see docs/cross-compile.md); the script fails loudly with
# cargo's own message when one is missing.
#
# The compiler itself is pure Rust, so no C toolchain is needed to *build*
# it. A C compiler is only needed on the machine that *runs* it, to compile
# the generated C99.
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [ -z "$VERSION" ]; then
    echo "release: cannot read the version from Cargo.toml" >&2
    exit 1
fi

if [ "$#" -gt 0 ]; then
    TARGETS=("$@")
else
    TARGETS=("$(rustc -vV | sed -n 's/^host: //p')")
fi

DIST="dist"
rm -rf "$DIST"
mkdir -p "$DIST"

for target in "${TARGETS[@]}"; do
    echo "release: building $VERSION for $target"
    cargo build --release --locked --target "$target"

    bindir="target/$target/release"
    name="wlel-$VERSION-$target"
    stage="$DIST/$name"
    mkdir -p "$stage"

    if [ -f "$bindir/wlel.exe" ]; then
        cp "$bindir/wlel.exe" "$stage/wlel.exe"
    elif [ -f "$bindir/wlel" ]; then
        cp "$bindir/wlel" "$stage/wlel"
    else
        echo "release: no wlel binary at $bindir" >&2
        exit 1
    fi
    cp README.md "$stage/README.md"

    # native builds are smoke-tested here; cross builds run on their target
    if [ "$target" = "$(rustc -vV | sed -n 's/^host: //p')" ]; then
        got="$("$stage/wlel" --version)"
        if [ "$got" != "wlel $VERSION" ]; then
            echo "release: version mismatch: expected 'wlel $VERSION', got '$got'" >&2
            exit 1
        fi
        echo "release: $got"
    fi

    tar -C "$DIST" -czf "$DIST/$name.tar.gz" "$name"
    rm -rf "$stage"
    (cd "$DIST" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
    echo "release: wrote $DIST/$name.tar.gz"
done
