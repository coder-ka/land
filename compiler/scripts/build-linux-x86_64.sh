#!/bin/sh
set -eu

COMPILER=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
ROOT=$(CDPATH= cd -- "$COMPILER/.." && pwd)
mkdir -p "$COMPILER/dist"

docker build \
    --platform linux/amd64 \
    --file "$COMPILER/docker/linux-x86_64.Dockerfile" \
    --output "type=local,dest=$COMPILER/dist" \
    "$ROOT"

chmod +x "$COMPILER/dist/landc-linux-x86_64"
