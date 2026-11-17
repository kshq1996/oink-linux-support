#!/usr/bin/env bash
# Test suite executed inside the Linux image.
#
# The source tree is copied into a writable work directory first, so the host
# checkout is never modified: no target/, node_modules/ or dist/ is left
# behind on the developer's machine.
set -euo pipefail

WORK=/work
SRC=/src
# The source tree lands next to the cargo target directory, which is a mounted
# volume: the tree must not share a directory with it, or the copy would wipe
# the cached build output.
TREE="$WORK/tree"

step() { printf '\n=== %s\n' "$1"; }

prepare() {
    rm -rf "$TREE"
    mkdir -p "$TREE"
    # Copy the tree plus the working-tree changes, minus the build
    # outputs and the Git metadata the tests never read.
    tar -C "$SRC" --exclude=./.git --exclude=./target --exclude=./node_modules \
        --exclude=./apps/oink/node_modules --exclude=./apps/oink/dist \
        --exclude=./dist -cf - . | tar -C "$TREE" -xf -
}

prepare
cd "$TREE"

# The clipboard, display and input tests drive a real X server, exactly as the
# macOS and Windows suites drive the pasteboard and the window server. Xvfb
# gives the container one without a desktop session.
start_display() {
    Xvfb :99 -screen 0 1280x1024x24 >/dev/null 2>&1 &
    DISPLAY_PID=$!
    trap 'kill "$DISPLAY_PID" 2>/dev/null || true' EXIT
    export DISPLAY=:99
    for _ in $(seq 1 100); do
        [[ -S /tmp/.X11-unix/X99 ]] && return 0
        sleep 0.2
    done
    echo "the virtual X server did not start" >&2
    return 1
}

step 'frontend'
npm ci
npm run -w apps/oink typecheck
npm run -w apps/oink test

step 'rust formatting'
cargo fmt --all --check

step 'rust build and tests'
start_display
python3 build-support/ci.py

step 'packaging tests'
python3 -m unittest discover -s packaging/tests -v

if [[ "${OINK_LINUX_DENY:-0}" == "1" ]]; then
    step 'dependency policy'
    if ! cargo deny --version 2>/dev/null | grep -q '0.20.2$'; then
        cargo install cargo-deny --version 0.20.2 --locked
    fi
    cargo deny check
fi

step 'done'
