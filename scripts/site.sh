#!/bin/sh
# Build the documentation site (and the WASM playground behind it).
#
#   scripts/site.sh [OUT_DIR]      # default: _site
#
# Needs python3 and the wasm32-unknown-unknown Rust target; both are already
# present on CI and in a normal dev setup. The site is static: HTML, one CSS
# file, a logo and a ~470 KiB .wasm module.
set -eu

cd "$(dirname "$0")/.."
OUT="${1:-_site}"

if ! rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
    rustup target add wasm32-unknown-unknown
fi

cargo build --release --locked \
    --manifest-path playground/wasm/Cargo.toml \
    --target wasm32-unknown-unknown

WASM=playground/wasm/target/wasm32-unknown-unknown/release/wlel_playground.wasm
rm -rf "$OUT"
python3 scripts/docsite.py "$OUT" --wasm "$WASM"

size=$(wc -c < "$OUT/playground/wlel.wasm" | tr -d ' ')
echo "site: wrote $OUT (playground.wasm ${size} bytes)"
