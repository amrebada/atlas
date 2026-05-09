#!/usr/bin/env bash
# Build the atlas-helper Swift sidecar into src-tauri/binaries/ with the
# Tauri-required <name>-<target-triple> filename. By default builds only
# for the host arch; pass `--universal` to build a fat binary suitable
# for distribution.
#
# Tauri's bundle.externalBin lookup expects:
#   binaries/atlas-helper-aarch64-apple-darwin
#   binaries/atlas-helper-x86_64-apple-darwin
# at config time; missing files fail `tauri dev` / `tauri build` for
# that architecture only.
#
# Usage:
#   ./build.sh             # host arch only (fast — for dev)
#   ./build.sh --universal # both arches (for release)

set -euo pipefail

cd "$(dirname "$0")"
SRC="main.swift"
OUT_DIR="$(cd ../../binaries 2>/dev/null && pwd || (mkdir -p ../../binaries && cd ../../binaries && pwd))"

build_one() {
    local triple="$1"
    local sw_target="$2"
    local out="$OUT_DIR/atlas-helper-$triple"
    echo "→ $out"
    swiftc \
        -O \
        -target "$sw_target" \
        -framework AppKit \
        -framework ScreenCaptureKit \
        -framework Foundation \
        -o "$out" \
        "$SRC"
    chmod +x "$out"
}

case "${1:-}" in
    --universal)
        build_one "aarch64-apple-darwin" "arm64-apple-macos13.0"
        build_one "x86_64-apple-darwin"  "x86_64-apple-macos13.0"
        ;;
    "")
        host_arch="$(uname -m)"
        case "$host_arch" in
            arm64)  build_one "aarch64-apple-darwin" "arm64-apple-macos13.0" ;;
            x86_64) build_one "x86_64-apple-darwin"  "x86_64-apple-macos13.0" ;;
            *) echo "unknown host arch: $host_arch" >&2; exit 1 ;;
        esac
        ;;
    *)
        echo "usage: $0 [--universal]" >&2
        exit 2
        ;;
esac

echo "ok"
