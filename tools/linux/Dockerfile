# Linux build/test environment for Oink.
#
# One image serves three jobs, so CI and local runs exercise the same
# toolchain: compile the workspace, run the Rust/Python test suites, and
# produce the release packages. Pinned to the versions the project requires
# (rust-toolchain.toml, package.json engines).
FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
ARG RUST_VERSION=1.99.0
ARG NODE_VERSION=24.14.1
ARG NPM_VERSION=11.11.0

ENV LANG=C.UTF-8 \
    LC_ALL=C.UTF-8 \
    TZ=UTC \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    PYTHONUTF8=1 \
    CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=0 \
    CARGO_PROFILE_DEV_DEBUG=0 \
    CARGO_PROFILE_TEST_DEBUG=0 \
    CARGO_PROFILE_RELEASE_DEBUG=1 \
    CARGO_PROFILE_RELEASE_STRIP=none \
    CMAKE_POLICY_VERSION_MINIMUM=3.5

# Toolchain and the Tauri v2 system dependencies. webkit2gtk-4.1 is the
# WebKitGTK release Tauri v2 requires; libayatana-appindicator3 provides the
# StatusNotifier tray icon on desktops without a legacy tray.
RUN apt-get update \
 && apt-get install --no-install-recommends -y \
      build-essential \
      ca-certificates \
      cmake \
      curl \
      desktop-file-utils \
      file \
      git \
      gosu \
      libasound2-dev \
      libayatana-appindicator3-dev \
      librsvg2-dev \
      libssl-dev \
      libwebkit2gtk-4.1-dev \
      patchelf \
      pkg-config \
      python3 \
      python3-pip \
      python3-venv \
      unzip \
      xdg-utils \
      xvfb \
      zip \
 && rm -rf /var/lib/apt/lists/*

# Rust. The toolchain file pins 1.99.0, so the default toolchain here only
# has to exist for rustup to resolve it.
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo
ENV PATH=/opt/cargo/bin:/opt/node/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION" \
 && rustup component add rustfmt clippy \
 && chmod -R a+w "$RUSTUP_HOME" "$CARGO_HOME"

# Node and npm, pinned exactly like the workspace manifests require. Node
# names its Intel archive x64, while Debian calls the same architecture
# amd64, so the download name is mapped instead of reused.
RUN set -eux; \
    case "$(dpkg --print-architecture)" in \
      amd64) node_arch=x64 ;; \
      arm64) node_arch=arm64 ;; \
      *) echo "unsupported Debian architecture" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-${node_arch}.tar.xz" \
      -o /tmp/node.tar.xz; \
    mkdir -p /opt/node; \
    tar -xJf /tmp/node.tar.xz -C /opt/node --strip-components=1; \
    rm /tmp/node.tar.xz; \
    npm install --global "npm@${NPM_VERSION}"

# Packaging tools that are not needed to compile the workspace live in their
# own layer so adding one never invalidates the toolchain downloads above.
RUN apt-get update \
 && apt-get install --no-install-recommends -y rpm \
 && rm -rf /var/lib/apt/lists/*

COPY entrypoint.sh /usr/local/bin/linux-entrypoint.sh
RUN chmod 0755 /usr/local/bin/linux-entrypoint.sh
WORKDIR /src
ENTRYPOINT ["/usr/local/bin/linux-entrypoint.sh"]
CMD ["bash"]
