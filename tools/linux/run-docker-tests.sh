#!/usr/bin/env bash
# Run the Linux test suite in the container image for one or both CPU
# architectures.
#
# The host checkout is mounted read-only and the container works on a copy, so
# a finished run leaves nothing behind on the host: no containers, no build
# outputs in the tree. The cargo registry and the target directory live in
# named volumes, so repeated runs reuse the compiled dependencies.
#
# Usage:
#   tools/linux/run-docker-tests.sh [--arch amd64|arm64|both] [--deny]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=tools/linux/image.sh
source "$ROOT/tools/linux/image.sh"
CARGO_VOLUME_PREFIX="${OINK_LINUX_CARGO_VOLUME_PREFIX:-oink-linux-cargo}"
TARGET_VOLUME_PREFIX="${OINK_LINUX_TARGET_VOLUME_PREFIX:-oink-linux-target}"

ARCHES=amd64
DENY=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --arch) ARCHES="$2"; shift 2 ;;
        --deny) DENY=1; shift ;;
        -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

# One image per architecture: a locally built image carries a single platform
# manifest, so an amd64 run needs an amd64 image. An image that already exists
# is reused, so a repeated run costs no build time.
build_image() {
    local arch="$1"
    if docker image inspect "$IMAGE_PREFIX-$arch" >/dev/null 2>&1; then
        return
    fi
    docker buildx build --platform "linux/$arch" \
        --file "$ROOT/tools/linux/Dockerfile" \
        --tag "$IMAGE_PREFIX-$arch" --load "$ROOT/tools/linux"
}

run_one() {
    local arch="$1"
    echo "=== Linux test suite: linux/$arch"
    docker run --rm --platform "linux/$arch" \
        --volume "$ROOT:/src:ro" \
        --volume "$CARGO_VOLUME_PREFIX-$arch:/opt/cargo" \
        --volume "$TARGET_VOLUME_PREFIX-$arch:/work/target" \
        --env OINK_LINUX_DENY="$DENY" \
        --env CARGO_TARGET_DIR=/work/target \
        --env OINK_LINUX_UID="$(id -u)" \
        "$IMAGE_PREFIX-$arch" /src/tools/linux/test.sh
}

if [[ "$ARCHES" == "both" ]]; then
    build_image amd64
    build_image arm64
    run_one amd64
    run_one arm64
else
    build_image "$ARCHES"
    run_one "$ARCHES"
fi
