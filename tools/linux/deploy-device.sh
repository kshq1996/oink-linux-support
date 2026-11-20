#!/usr/bin/env bash
# Deploy a published Linux package to the test device.
#
# The device runs an immutable distribution (rpm-ostree), so the package is
# installed the way a user installs it: the binaries, the udev rules and the
# desktop entry all come from the package. Copying the binaries over the
# installed ones leaves the old rules and the old login item behind, which is
# how the hand-made rule file and the hand-made login item ended up on the
# device.
#
# `rpm-ostree install` stages a new deployment; it goes live after a reboot.
# Pass `--reboot` to reboot at the end of the install, otherwise the script
# stops before the reboot and prints the command.
#
# Usage: tools/linux/deploy-device.sh <arch: x64|arm64> <host> [--reboot]
set -euo pipefail

ARCH=${1:-}
HOST=${2:-}
REBOOT=no
for argument in "$@"; do
    case "$argument" in
    --reboot) REBOOT=yes ;;
    esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION=$(python3 -c "import json,sys;print(json.load(open(sys.argv[1]))['version'])" "$ROOT/packaging/identity.json")
REPO=${OINK_REPO:-kshq1996/oink-linux-support}
WORK=${WORK:-/tmp/oink-deploy-$ARCH}
# shellcheck source=tools/linux/proxy.sh
source "$ROOT/tools/linux/proxy.sh"

if [[ -z "$ARCH" || -z "$HOST" ]]; then
    echo "usage: $0 <arch: x64|arm64> <host> [--reboot]" >&2
    exit 2
fi

case "$ARCH" in
x64|arm64) ;;
*) echo "unknown architecture: $ARCH" >&2; exit 2 ;;
esac

PACKAGE="Oink-$VERSION-linux-$ARCH.rpm"

rm -rf "$WORK"
mkdir -p "$WORK"
cd "$WORK"

echo "Downloading $PACKAGE"
env ${OINK_LINUX_PROXY_HOST_ENV[@]+"${OINK_LINUX_PROXY_HOST_ENV[@]}"} \
    gh release download "v$VERSION" --repo "$REPO" --pattern "$PACKAGE" --clobber

echo "Copying the package to $HOST"
scp -q "$PACKAGE" "$HOST:/tmp/"

echo "Installing the package with rpm-ostree"
ssh "$HOST" "sudo rpm-ostree install /tmp/$PACKAGE"

if [[ "$REBOOT" == no ]]; then
    echo "The new deployment is staged. Reboot to make it live:"
    echo "  ssh $HOST sudo reboot"
    echo "Then run this script again to refresh the rules and the login item."
    exit 0
fi

echo "Rebooting"
ssh "$HOST" "sudo reboot" || true

# The device comes back on its own; wait until it answers again.
echo "Waiting for $HOST"
for _ in $(seq 1 60); do
    sleep 5
    if ssh -o ConnectTimeout=5 "$HOST" true 2>/dev/null; then
        break
    fi
done

# The packaged rule only reaches the devices that appear after the install;
# the ones that were already there need the rules to run over them again.
echo "Refreshing the udev rules"
ssh "$HOST" 'sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=input --action=change'

# The package ships the rules, so the hand-made copies in /etc are redundant.
echo "Dropping the hand-made udev copies"
ssh "$HOST" 'sudo rm -f /etc/udev/rules.d/65-oink-input.rules /etc/udev/rules.d/99-oink-no-gamepad-mouse.rules'

# The login item has to name the installed interface program. The product
# writes the entry itself, from the program it runs next to, so the entry is
# registered through the installed agent rather than written by hand.
echo "Pointing the login item at the installed interface program"
ssh "$HOST" '/usr/lib/opt/oink/oink-agent call set_launch_at_login "{\"enabled\":true}" || true'
ssh "$HOST" 'cat "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/oink.desktop"'

echo "Deployed Oink $VERSION ($ARCH) to $HOST"
