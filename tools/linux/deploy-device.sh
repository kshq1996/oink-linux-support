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
# Pass `--reboot` to reboot at the end of the install. Without it the script
# stops after the install and prints what is left to do; `--after-reboot`
# runs the rest (the rules and the login item) without installing again.
#
# Usage: tools/linux/deploy-device.sh <arch: x64|arm64> <host> [--reboot] [--after-reboot]
set -euo pipefail

ARCH=${1:-}
HOST=${2:-}
REBOOT=no
AFTER_REBOOT=no
for argument in "$@"; do
    case "$argument" in
    --reboot) REBOOT=yes ;;
    --after-reboot) AFTER_REBOOT=yes ;;
    esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION=$(python3 -c "import json,sys;print(json.load(open(sys.argv[1]))['version'])" "$ROOT/packaging/identity.json")
REPO=${OINK_REPO:-kshq1996/oink-linux-support}
WORK=${WORK:-/tmp/oink-deploy-$ARCH}
# shellcheck source=tools/linux/proxy.sh
source "$ROOT/tools/linux/proxy.sh"

if [[ -z "$ARCH" || -z "$HOST" ]]; then
    echo "usage: $0 <arch: x64|arm64> <host> [--reboot] [--after-reboot]" >&2
    exit 2
fi

case "$ARCH" in
x64|arm64) ;;
*) echo "unknown architecture: $ARCH" >&2; exit 2 ;;
esac

PACKAGE="Oink-$VERSION-linux-$ARCH.rpm"

# The device's sudo asks for a password. The password can be handed over
# through a file (`OINK_SUDO_PASSWORD_FILE`), so it never lands in a command
# line; without it the script asks the remote sudo directly.
SUDO_PASSWORD_FILE=${OINK_SUDO_PASSWORD_FILE:-}

# Run a command on the device with root rights.
#
# sudo does not run the command through a shell, so the command is handed to
# `sh -c`; that way a chain like `a && b` stays one command.
remote_sudo() {
    local command=$1
    if [[ -n "$SUDO_PASSWORD_FILE" ]]; then
        printf '%s\n' "$(cat "$SUDO_PASSWORD_FILE")" |
            ssh "$HOST" "sudo -S -p '' sh -c $(printf '%q' "$command")"
    else
        ssh "$HOST" "sudo sh -c $(printf '%q' "$command")"
    fi
}

install_package() {
    rm -rf "$WORK"
    mkdir -p "$WORK"
    cd "$WORK"

    echo "Downloading $PACKAGE"
    env ${OINK_LINUX_PROXY_HOST_ENV[@]+"${OINK_LINUX_PROXY_HOST_ENV[@]}"} \
        gh release download "v$VERSION" --repo "$REPO" --pattern "$PACKAGE" --clobber

    echo "Copying the package to $HOST"
    scp -q "$PACKAGE" "$HOST:/tmp/"

    echo "Installing the package with rpm-ostree"
    # The running deployment was changed by hand (the binaries were copied
    # over the installed ones), and rpm-ostree refuses to build a new
    # deployment on top of a changed one without `--allow-inactive`.
    #
    # The old package is a layered package, and installing the new one alone
    # conflicts with it ("cannot install both oink-0.3.6-3 and oink-0.3.6-1"),
    # so the old one leaves in the same transaction.
    remote_sudo "rpm-ostree install --allow-inactive --uninstall=oink /tmp/$PACKAGE"
}

reboot_into_the_new_deployment() {
    echo "Rebooting"
    remote_sudo reboot || true

    # The device comes back on its own; wait until it answers again. A
    # reboot also restarts the device's own VPN client, so it can take a
    # while before the corporate network carries it again.
    echo "Waiting for $HOST"
    for _ in $(seq 1 120); do
        sleep 5
        if ssh -o ConnectTimeout=5 "$HOST" true 2>/dev/null; then
            return 0
        fi
    done
    echo "$HOST did not come back; run this script again with --after-reboot." >&2
    return 1
}

# The packaged rule only reaches the devices that appear after the install;
# the ones that were already there need the rules to run over them again.
refresh_rules() {
    echo "Refreshing the udev rules"
    remote_sudo "udevadm control --reload && udevadm trigger --subsystem-match=input --action=change"

    # The package ships the rules, so the hand-made copies in /etc are
    # redundant.
    echo "Dropping the hand-made udev copies"
    remote_sudo "rm -f /etc/udev/rules.d/65-oink-input.rules /etc/udev/rules.d/99-oink-no-gamepad-mouse.rules"
}

# The login item has to name the installed interface program.
#
# The product writes the entry itself, but only from the backend that runs in
# the desktop session; over SSH there is no session, and the backend that does
# answer may be a development copy whose entry would point at the wrong
# program. The product reads the entry from the file, so the file is what
# matters: it is written here in the format the product uses.
#
# `rpm-ostree` moves the package's `/opt/oink` to `/usr/lib/opt/oink`, so the
# installed program is looked up on the device.
register_login_item() {
    echo "Pointing the login item at the installed interface program"
    ssh "$HOST" bash -s <<'REMOTE'
set -e
entry="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/oink.desktop"
ui=/usr/lib/opt/oink/oink
[ -x "$ui" ] || ui=/opt/oink/oink
mkdir -p "$(dirname "$entry")"
cat > "$entry" <<ENTRY
[Desktop Entry]
Type=Application
Name=Oink
Exec="$ui" --autostart
Icon=oink
Terminal=false
Categories=Utility;RemoteAccess;
X-GNOME-Autostart-enabled=true
Hidden=false
ENTRY
cat "$entry"
REMOTE
}

if [[ "$AFTER_REBOOT" == no ]]; then
    install_package
    if [[ "$REBOOT" == no ]]; then
        echo "The new deployment is staged. Reboot to make it live:"
        echo "  ssh $HOST sudo reboot"
        echo "Then run: $0 $ARCH $HOST --after-reboot"
        exit 0
    fi
    reboot_into_the_new_deployment
fi

refresh_rules
register_login_item

echo "Deployed Oink $VERSION ($ARCH) to $HOST"
