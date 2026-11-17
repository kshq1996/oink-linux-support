#!/usr/bin/env bash
# Build, audit and stage the public Linux packages.
#
# Runs inside the Linux image, with the checkout mounted read-only at /src and
# the release output mounted at /out. The build installs npm packages and
# writes build products, so it works on a copy of the checkout in the work
# volume: the developer's tree is never modified.
set -euo pipefail

WORK=/work
SRC=/src
# The source tree lands next to the cargo target directory, which is a mounted
# volume: the tree must not share a directory with it, or the copy would wipe
# the cached build output.
TREE="$WORK/tree"

# The attribution step caches the optional npm tarballs it fetches; keeping
# them in the temporary directory makes a repeated release run cheap.
mkdir -p "${TMPDIR:-/tmp}"

ARCH="$OINK_LINUX_ARCH"
SEQUENCE="$OINK_LINUX_SEQUENCE"
PACKAGES=/out/packages

# The container architecture names the image; the product names the package.
case "$ARCH" in
    amd64) PACKAGE_ARCH=x64; RELEASE_PLATFORM=linux-x64 ;;
    arm64) PACKAGE_ARCH=arm64; RELEASE_PLATFORM=linux-arm64 ;;
    *) echo "unsupported architecture: $ARCH" >&2; exit 2 ;;
esac
DISTRIBUTION=/out/distribution

rm -rf "$PACKAGES" "$DISTRIBUTION" "$TREE"
mkdir -p "$TREE"
# The copy keeps the Git metadata: the source inventory and its file modes
# come from the index, and the release records the commit it was built from.
tar -C "$SRC" --exclude=./node_modules --exclude=./apps/oink/node_modules \
    --exclude=./apps/oink/dist --exclude=./dist --exclude=./target -cf - . \
    | tar -C "$TREE" -xf -
cd "$TREE"

python3 build-support/package.py --public-version --platform linux --arch "$PACKAGE_ARCH" \
    --sequence "$SEQUENCE" --output-root "$PACKAGES" --tool-cache /work/tool-cache
python3 build-support/check-report-configuration.py --packages "$PACKAGES" --platform "$RELEASE_PLATFORM"
python3 build-support/public_release.py stage --packages "$PACKAGES" --platform "$RELEASE_PLATFORM" \
    --sequence "$SEQUENCE" --output "$DISTRIBUTION"
