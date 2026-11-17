#!/usr/bin/env bash
# Refresh Cargo.lock inside the Linux image and copy it back to the checkout.
#
# The image owns the toolchain, so the lock file is resolved there and written
# back; the host needs no Rust installation. The lock file is shared by every
# platform, so one resolution covers macOS, Windows and Linux.
#
# Usage:
#   tools/linux/lock.sh [--arch amd64|arm64]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ARCH="${OINK_LINUX_ARCH:-amd64}"
IMAGE="${OINK_LINUX_IMAGE_PREFIX:-oink-linux-build:0.3.7}-$ARCH"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --arch) ARCH="$2"; shift 2 ;;
        -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    docker buildx build --platform "linux/$ARCH" \
        --file "$ROOT/tools/linux/Dockerfile" --tag "$IMAGE" --load "$ROOT/tools/linux"
fi

docker run --rm --platform "linux/$ARCH" \
    --volume "$ROOT:/src" \
    --volume "oink-linux-cargo-$ARCH:/opt/cargo" \
    --env CARGO_TARGET_DIR=/work/target \
    --env OINK_LINUX_UID="$(id -u)" \
    --entrypoint bash "$IMAGE" -lc '
        set -euo pipefail
        rm -rf /work/src-copy
        mkdir -p /work/src-copy
        tar -C /src --exclude=./.git --exclude=./target --exclude=./node_modules \
            --exclude=./apps/oink/node_modules --exclude=./apps/oink/dist -cf - . \
            | tar -C /work/src-copy -xf -
        cd /work/src-copy
        cargo metadata --format-version 1 >/dev/null
        cp Cargo.lock /src/Cargo.lock
    '

echo "Cargo.lock refreshed for linux/$ARCH"
