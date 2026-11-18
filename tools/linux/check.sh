#!/usr/bin/env bash
# Run a cargo command against the working tree inside the Linux container.
#
# Usage: tools/linux/check.sh [cargo arguments...]
#   tools/linux/check.sh check -p oink-input
#
# The tree is copied into a writable work directory, so the host checkout keeps
# no build output. The cargo registry and the target directory are cached in
# named volumes between runs. The command runs as the unprivileged container
# user, because the product has tests that only mean something without root.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=tools/linux/image.sh
source "$ROOT/tools/linux/image.sh"
# shellcheck source=tools/linux/proxy.sh
source "$ROOT/tools/linux/proxy.sh"
ARCH="${OINK_LINUX_ARCH:-amd64}"
IMAGE="$IMAGE_PREFIX-$ARCH"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    docker buildx build --platform "linux/$ARCH" \
        --file "$ROOT/tools/linux/Dockerfile" --tag "$IMAGE" --load \
        ${OINK_LINUX_PROXY_BUILD_FLAGS[@]+"${OINK_LINUX_PROXY_BUILD_FLAGS[@]}"} \
        "$ROOT/tools/linux"
fi

exec docker run --rm --platform "linux/$ARCH" \
    --volume "$ROOT:/src:ro" \
    --volume "oink-linux-cargo-$ARCH:/opt/cargo" \
    --volume "oink-linux-target-$ARCH:/work/target" \
    --env CARGO_TARGET_DIR=/work/target \
    --env OINK_LINUX_UID="$(id -u)" \
    ${OINK_LINUX_PROXY_RUN_FLAGS[@]+"${OINK_LINUX_PROXY_RUN_FLAGS[@]}"} \
    "$IMAGE" /src/tools/linux/check-in-container.sh "$@"
