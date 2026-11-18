#!/usr/bin/env bash
# Deploy a published Linux package to the test device.
#
# The device runs an immutable distribution, so the packaged binaries are
# unpacked on the workstation and copied over the installed ones. The package
# itself stays the source of truth for the file list.
#
# Usage: tools/linux/deploy-device.sh <arch: x64|arm64> [host]
set -euo pipefail

ARCH=${1:-x64}
HOST=${2:-kangshaoqi@192.168.22.55}
VERSION=$(python3 -c "import json;print(json.load(open('packaging/identity.json'))['version'])")
REPO=${OINK_REPO:-kshq1996/oink-linux-support}
WORK=${WORK:-/tmp/oink-deploy-$ARCH}
IMAGE=${OINK_LINUX_IMAGE:-oink-linux-build:0.3.6-arm64}

case "$ARCH" in
x64|arm64) ;;
*) echo "unknown architecture: $ARCH" >&2; exit 2 ;;
esac

rm -rf "$WORK"
mkdir -p "$WORK/pkg"
cd "$WORK"

echo "Downloading Oink $VERSION for $ARCH"
gh release download "v$VERSION" --repo "$REPO" \
    --pattern "Oink-$VERSION-linux-$ARCH.rpm" --clobber

echo "Unpacking the package"
docker run --rm -v "$WORK:/work" --entrypoint sh "$IMAGE" \
    -c "rpm2cpio /work/Oink-$VERSION-linux-$ARCH.rpm > /work/package.cpio"
tar -xf package.cpio -C pkg

echo "Copying the binaries to $HOST"
scp -q pkg/opt/oink/oink pkg/opt/oink/oink-agent pkg/opt/oink/oink-backend "$HOST:/tmp/"
ssh "$HOST" 'sudo cp /tmp/oink /tmp/oink-agent /tmp/oink-backend /opt/oink/ && sudo chmod 0755 /opt/oink/oink /opt/oink/oink-agent /opt/oink/oink-backend && rm -f /tmp/oink /tmp/oink-agent /tmp/oink-backend'

echo "Deployed Oink $VERSION ($ARCH) to $HOST"
