#!/usr/bin/env bash
# Run a cargo command against a copy of the working tree inside the Linux
# container.
#
# Usage: tools/linux/check-in-container.sh [cargo arguments...]
#
# The image entrypoint already dropped to the unprivileged container user: some
# product tests only mean something without root, and the host checkout must
# keep no build output.
set -euo pipefail

WORK=/work
SRC=/src
# The source tree lands next to the cargo target directory, which is a mounted
# volume: the tree must not share a directory with it, or the copy would wipe
# the cached build output.
TREE="$WORK/tree"

rm -rf "$TREE"
mkdir -p "$TREE"
tar -C "$SRC" --exclude=./.git --exclude=./target --exclude=./node_modules \
    --exclude=./apps/oink/node_modules --exclude=./apps/oink/dist --exclude=./dist \
    -cf - . | tar -C "$TREE" -xf -
cd "$TREE"
exec cargo "$@"
