#!/usr/bin/env bash
# Run the frontend checks inside the Linux image.
#
# The tree is copied into the container first, so `npm ci` never writes
# node_modules into the checkout and a finished run leaves nothing behind on
# the host. The npm cache lives in a named volume, so repeated runs install
# from the cache instead of the network.
#
# Usage: tools/linux/frontend.sh [npm arguments...]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ARCH="${OINK_LINUX_ARCH:-amd64}"
IMAGE="${OINK_LINUX_IMAGE_PREFIX:-oink-linux-build:0.3.7}-$ARCH"

if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    docker buildx build --platform "linux/$ARCH" \
        --file "$ROOT/tools/linux/Dockerfile" --tag "$IMAGE" --load "$ROOT/tools/linux"
fi

exec docker run --rm --platform "linux/$ARCH" \
    --volume "$ROOT:/src:ro" \
    --volume "oink-linux-npm-$ARCH:/work/npm-cache" \
    --env npm_config_cache=/work/npm-cache \
    --env OINK_LINUX_UID="$(id -u)" \
    --entrypoint bash "$IMAGE" -lc '
        set -euo pipefail
        rm -rf /work/src-copy
        mkdir -p /work/src-copy
        tar -C /src --exclude=./.git --exclude=./target --exclude=./node_modules \
            --exclude=./apps/oink/node_modules --exclude=./apps/oink/dist \
            -cf - . | tar -C /work/src-copy -xf -
        cd /work/src-copy
        npm ci
        npm run -w apps/oink typecheck
        npm run -w apps/oink test
        npm run -w apps/oink build
    '
