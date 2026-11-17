#!/usr/bin/env bash
# Remove everything the Linux toolchain created on this machine.
#
# The test runs leave the checkout untouched: the build output lives in
# container volumes and the images are rebuilt on demand. This script removes
# the images, the volumes and any container the runs left behind, so a machine
# that only ran the Linux suite is back to its previous state.
#
# Usage: tools/linux/clean.sh [--all]
#   --all   also remove the shared cargo and target volumes
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=tools/linux/image.sh
source "$ROOT/tools/linux/image.sh"
CARGO_VOLUME_PREFIX="${OINK_LINUX_CARGO_VOLUME_PREFIX:-oink-linux-cargo}"
TARGET_VOLUME_PREFIX="${OINK_LINUX_TARGET_VOLUME_PREFIX:-oink-linux-target}"

ALL=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --all) ALL=1; shift ;;
        -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

# Every version of the toolchain image goes, not only the current one.
while read -r image; do
    docker image rm "$image" >/dev/null 2>&1 || true
done < <(docker image ls --format '{{.Repository}}:{{.Tag}}' | grep "^${IMAGE_REPOSITORY}:" || true)

if [[ "$ALL" == "1" ]]; then
    for arch in amd64 arm64; do
        docker volume rm "$CARGO_VOLUME_PREFIX-$arch" >/dev/null 2>&1 || true
        docker volume rm "$TARGET_VOLUME_PREFIX-$arch" >/dev/null 2>&1 || true
    done
    # The first image revision cached the volumes without an architecture
    # suffix; they are the same toolchain state.
    docker volume rm "$CARGO_VOLUME_PREFIX" >/dev/null 2>&1 || true
    docker volume rm "$TARGET_VOLUME_PREFIX" >/dev/null 2>&1 || true
fi

# Containers only survive a killed run; a finished run removes itself.
docker ps -a --filter "ancestor=$IMAGE_REPOSITORY" \
    --format '{{.ID}}' | while read -r id; do
    docker rm --force "$id" >/dev/null 2>&1 || true
done

echo "Linux toolchain images removed. Cached build volumes: $(docker volume ls -q --filter "name=$CARGO_VOLUME_PREFIX" | tr '\n' ' ')"
