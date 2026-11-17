#!/usr/bin/env bash
# The container image carries the released version, so a version bump never
# needs a script edit and no run reuses an image built for another version.
#
# Source this file: it sets IMAGE_REPOSITORY and IMAGE_PREFIX. The callers
# append the architecture to the prefix.
OINK_LINUX_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OINK_LINUX_VERSION="$(sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
    "$OINK_LINUX_ROOT/packaging/identity.json" | head -1)"
if [[ -z "${OINK_LINUX_VERSION:-}" ]]; then
    echo "the product version is missing from packaging/identity.json" >&2
    exit 1
fi
IMAGE_REPOSITORY="oink-linux-build"
IMAGE_PREFIX="${OINK_LINUX_IMAGE_PREFIX:-$IMAGE_REPOSITORY:$OINK_LINUX_VERSION}"
