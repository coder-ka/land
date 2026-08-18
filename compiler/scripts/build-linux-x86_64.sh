#!/bin/sh
set -eu

COMPILER=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
ROOT=$(CDPATH= cd -- "$COMPILER/.." && pwd)

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo is required" >&2
    exit 1
fi

if [ -n "${LLVM_CONFIG:-}" ]; then
    if ! command -v "$LLVM_CONFIG" >/dev/null 2>&1; then
        echo "LLVM_CONFIG does not name an executable: $LLVM_CONFIG" >&2
        exit 1
    fi
else
    for candidate in llvm-config llvm-config-21 llvm-config-20 llvm-config-19 \
        llvm-config-18 llvm-config-17 llvm-config-16 llvm-config-15 llvm-config-14
    do
        if command -v "$candidate" >/dev/null 2>&1; then
            LLVM_CONFIG=$candidate
            break
        fi
    done
    if [ -z "${LLVM_CONFIG:-}" ]; then
        echo "llvm-config is required; set LLVM_CONFIG when it has a versioned name" >&2
        exit 1
    fi
fi
export LLVM_CONFIG

cargo build --manifest-path "$ROOT/Cargo.toml" --locked --release \
    -p land-compiler --features embedded-backend

mkdir -p "$COMPILER/dist"
cp "$ROOT/target/release/landc" "$COMPILER/dist/landc-linux-x86_64"
chmod +x "$COMPILER/dist/landc-linux-x86_64"
(
    cd "$COMPILER/dist"
    sha256sum landc-linux-x86_64 >landc-linux-x86_64.sha256
)
printf '%s\n' "$COMPILER/dist/landc-linux-x86_64"
