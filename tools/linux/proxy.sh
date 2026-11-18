#!/usr/bin/env bash
# The download proxy for the Linux toolchain.
#
# The image build and the container runs fetch their packages from the
# network, and a workstation in China reaches the upstream registries
# slowly. The proxy that runs on the workstation fixes that, so the scripts
# hand it to the container and to the image build.
#
# The proxy is a transport detail, not a source replacement: cargo and npm
# still resolve every package from its canonical registry, so the lock file
# and the licence inventory keep the upstream sources.
#
# The proxy is found in this order:
#   1. OINK_LINUX_PROXY, when it is set: the URL, for example
#      http://127.0.0.1:7890. The value "none" turns the proxy off.
#   2. A proxy listening on the loopback interface of the workstation, on
#      the port in OINK_LINUX_PROXY_PORT (7890 by default), when one
#      answers there. This is the usual workstation setup, and it keeps the
#      scripts working without configuration.
# A CI runner has no such proxy, so there the scripts download directly.
#
# Source this file: it sets the flag arrays that the docker calls use.
# An array is expanded as "${array[@]+"${array[@]}"}", because the scripts
# run under the bash 3.2 of macOS, whose empty array expansion fails under
# "set -u".

OINK_LINUX_PROXY_PORT="${OINK_LINUX_PROXY_PORT:-7890}"

# The loopback addresses of the container itself and of the workstation.
# Requests to them never leave the machine, so they never go through a
# proxy: the tests bind local servers on them.
OINK_LINUX_PROXY_NO_PROXY="localhost,127.0.0.1,::1,host.docker.internal"

# True when the address carries a working HTTP proxy. The check makes a
# real request through it, because a port that accepts connections can
# still belong to something that is not a proxy, and a broken proxy would
# fail every download with a confusing error.
proxy_works() {
    local url="$1"
    curl --silent --show-error --fail --output /dev/null --max-time 15 \
        --proxy "$url" https://registry.npmjs.org/ >/dev/null 2>&1
}

# The URL the container uses for the proxy. The proxy listens on the
# loopback interface of the workstation, which is the container itself
# from the inside, so the address becomes host.docker.internal. A proxy on
# any other address is already reachable from the container.
proxy_container_url() {
    local url="$1"
    case "$url" in
        http://127.0.0.1:*|http://localhost:*)
            echo "http://host.docker.internal:${url##*:}" ;;
        https://127.0.0.1:*|https://localhost:*)
            echo "https://host.docker.internal:${url##*:}" ;;
        *) echo "$url" ;;
    esac
}

OINK_LINUX_PROXY_URL=""
if [[ -n "${OINK_LINUX_PROXY:-}" && "${OINK_LINUX_PROXY}" != "none" ]]; then
    OINK_LINUX_PROXY_URL="$OINK_LINUX_PROXY"
elif [[ -z "${OINK_LINUX_PROXY:-}" ]] && proxy_works "http://127.0.0.1:$OINK_LINUX_PROXY_PORT"; then
    OINK_LINUX_PROXY_URL="http://127.0.0.1:$OINK_LINUX_PROXY_PORT"
fi

# The flags for the docker calls. The image build needs the build args,
# because the toolchain is downloaded while the image is built. The
# container run needs the environment, because the workspace is downloaded
# while the tests run. Both carry the address the container uses.
#
# NODE_USE_ENV_PROXY makes the fetch of Node honour the proxy: the licence
# inventory downloads the npm tarballs it records, and Node ignores the
# proxy environment otherwise.
#
# The environment for the commands that run on the workstation itself, such
# as the release download, keeps the original address: they reach the proxy
# on the loopback interface.
OINK_LINUX_PROXY_BUILD_FLAGS=()
OINK_LINUX_PROXY_RUN_FLAGS=()
OINK_LINUX_PROXY_HOST_ENV=()
if [[ -n "$OINK_LINUX_PROXY_URL" ]]; then
    container_url="$(proxy_container_url "$OINK_LINUX_PROXY_URL")"
    OINK_LINUX_PROXY_BUILD_FLAGS=(
        --build-arg "HTTP_PROXY=$container_url"
        --build-arg "http_proxy=$container_url"
        --build-arg "HTTPS_PROXY=$container_url"
        --build-arg "https_proxy=$container_url"
        --build-arg "NO_PROXY=$OINK_LINUX_PROXY_NO_PROXY"
        --build-arg "no_proxy=$OINK_LINUX_PROXY_NO_PROXY"
        --add-host "host.docker.internal:host-gateway"
    )
    OINK_LINUX_PROXY_RUN_FLAGS=(
        --env "HTTP_PROXY=$container_url"
        --env "HTTPS_PROXY=$container_url"
        --env "http_proxy=$container_url"
        --env "https_proxy=$container_url"
        --env "NO_PROXY=$OINK_LINUX_PROXY_NO_PROXY"
        --env "no_proxy=$OINK_LINUX_PROXY_NO_PROXY"
        --env "NODE_USE_ENV_PROXY=1"
        --add-host "host.docker.internal:host-gateway"
    )
    OINK_LINUX_PROXY_HOST_ENV=(
        "HTTP_PROXY=$OINK_LINUX_PROXY_URL"
        "HTTPS_PROXY=$OINK_LINUX_PROXY_URL"
        "http_proxy=$OINK_LINUX_PROXY_URL"
        "https_proxy=$OINK_LINUX_PROXY_URL"
        "NO_PROXY=localhost,127.0.0.1,::1"
        "no_proxy=localhost,127.0.0.1,::1"
    )
fi
