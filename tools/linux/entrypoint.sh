#!/usr/bin/env bash
# Shared entrypoint for the Linux image.
#
# The container runs as the unprivileged `oink` user. The cargo home and the
# work directory are handed to it, because both are mounted volumes that
# Docker creates owned by root.
set -euo pipefail

export CARGO_HOME="${CARGO_HOME:-/opt/cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-/opt/rustup}"

if ! id -u oink >/dev/null 2>&1; then
    # The host user id keeps the files the container writes owned by the
    # person who started it, so a finished run leaves nothing behind that
    # only an administrator can remove.
    if [[ -n "${OINK_LINUX_UID:-}" ]]; then
        useradd --create-home --shell /bin/bash --uid "$OINK_LINUX_UID" oink
    else
        useradd --create-home --shell /bin/bash oink
    fi
fi

mkdir -p /work
chown -R oink:oink /work "$CARGO_HOME"

if [[ "$(id -u)" -eq 0 ]]; then
    exec gosu oink "$@"
fi
exec "$@"
