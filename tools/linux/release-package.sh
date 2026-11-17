#!/usr/bin/env bash
# Build the public Linux packages inside the project image.
#
# The image carries the exact toolchain and system libraries the release
# needs, so a runner and a developer produce the same packages. The checkout
# is mounted read-only; every build output lands in the container volumes and
# in the output directory, never in the source tree.
#
# The release sequence travels in the environment, so the workflow never has
# to repeat the release argument for the container path.
#
# Usage: tools/linux/release-package.sh --arch amd64|arm64 --output-root DIR
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
IMAGE_PREFIX="${OINK_LINUX_IMAGE_PREFIX:-oink-linux-build:0.3.6}"
CARGO_VOLUME_PREFIX="${OINK_LINUX_CARGO_VOLUME_PREFIX:-oink-linux-cargo}"
TARGET_VOLUME_PREFIX="${OINK_LINUX_TARGET_VOLUME_PREFIX:-oink-linux-target}"

ARCH=""
OUTPUT_ROOT=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --arch) ARCH="$2"; shift 2 ;;
        --output-root) OUTPUT_ROOT="$2"; shift 2 ;;
        -h|--help) sed -n '2,14p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done
: "${ARCH:?--arch is required}"
: "${OUTPUT_ROOT:?--output-root is required}"
: "${SEQUENCE:?the release sequence must be in the environment}"
case "$ARCH" in
    amd64|arm64) ;;
    *) echo "--arch must be amd64 or arm64" >&2; exit 2 ;;
esac

IMAGE="$IMAGE_PREFIX-$ARCH"
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    docker buildx build --platform "linux/$ARCH" \
        --file "$ROOT/tools/linux/Dockerfile" \
        --tag "$IMAGE" --load "$ROOT/tools/linux"
fi

mkdir -p "$OUTPUT_ROOT"
OUTPUT_ROOT="$(cd "$OUTPUT_ROOT" && pwd)"

docker run --rm --platform "linux/$ARCH" \
    --volume "$ROOT:/src:ro" \
    --volume "$CARGO_VOLUME_PREFIX-$ARCH:/opt/cargo" \
    --volume "$TARGET_VOLUME_PREFIX-$ARCH:/work/target" \
    --volume "$OUTPUT_ROOT:/out" \
    --env CARGO_TARGET_DIR=/work/target \
    --env TMPDIR=/opt/cargo/tmp \
    --env OINK_LINUX_UID="$(id -u)" \
    --env OINK_LINUX_ARCH="$ARCH" \
    --env OINK_LINUX_SEQUENCE="$SEQUENCE" \
    --env OINK_REPORT_DSN \
    --env OINK_REPORT_REGION \
    --env GIT_CONFIG_COUNT=1 \
    --env GIT_CONFIG_KEY_0=safe.directory \
    --env GIT_CONFIG_VALUE_0='*' \
    "$IMAGE" /src/tools/linux/release-package-inside.sh
