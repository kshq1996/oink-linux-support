#!/usr/bin/env bash
# Check the Oink install on the test device.
#
# The checks are the ones that decide whether the shared pointer survives the
# gamepad's right stick: the gamepad's mouse node must not be a mouse for the
# desktop, the rules must come from the package, and the login item must name
# the installed interface program.
#
# Usage: tools/linux/verify-device.sh <host>
set -uo pipefail

HOST=${1:-}
if [[ -z "$HOST" ]]; then
    echo "usage: $0 <host>" >&2
    exit 2
fi

failed=0

# A device that does not answer must not make the script hang.
ssh_to() {
    ssh -o ConnectTimeout=10 -o BatchMode=yes "$HOST" "$@"
}

check() {
    local title=$1
    shift
    echo "== $title"
    if "$@"; then
        echo "   ok"
    else
        echo "   FAILED"
        failed=1
    fi
}

# The installed package carries the version and the build sequence, so a
# device that still runs the old package is told apart from a patched one.
package_version() {
    ssh_to 'rpm -q oink 2>/dev/null || dpkg-query -W -f="${Package} ${Version}\n" oink 2>/dev/null'
}

# The rules have to come from the package: a hand-made copy in /etc is the
# reason the old build kept treating the stick as a mouse.
rules_from_the_package() {
    ssh_to '
        status=0
        if [ ! -f /usr/lib/udev/rules.d/65-oink-input.rules ]; then
            echo "the packaged rule is missing from /usr/lib/udev/rules.d"
            status=1
        fi
        for copy in /etc/udev/rules.d/*oink*; do
            [ -e "$copy" ] || continue
            echo "hand-made copy: $copy"
            status=1
        done
        if [ $status -eq 0 ]; then
            echo "the rules come from the package"
        fi
        exit $status'
}

# The gamepad's mouse node is the one that drags the shared pointer back.
# The packaged rule clears its ID_INPUT_MOUSE, so the desktop stops taking it
# for a mouse.
gamepad_node_is_not_a_mouse() {
    ssh_to '
        status=0
        for node in /dev/input/event*; do
            properties=$(udevadm info -q property -n "$node" 2>/dev/null)
            name=$(echo "$properties" | sed -n "s/^NAME=//p")
            case "$name" in
            *Controller*Mouse*|*Gamepad*Mouse*|*Joystick*Mouse*) ;;
            *) continue ;;
            esac
            mouse=$(echo "$properties" | sed -n "s/^ID_INPUT_MOUSE=//p")
            if [ -n "$mouse" ]; then
                echo "$node ($name) still says ID_INPUT_MOUSE=$mouse"
                status=1
            else
                echo "$node ($name) is not a mouse"
            fi
        done
        exit $status'
}

# libinput is what the desktop actually reads; the node must not be in its
# list as a pointer.
libinput_does_not_list_the_gamepad_pointer() {
    ssh_to '
        if libinput list-devices 2>/dev/null | grep -iE "controller.*mouse|gamepad.*mouse|joystick.*mouse"; then
            exit 1
        fi
        echo "no gamepad pointer in the device list"'
}

# The login item has to name the installed interface program. On an
# rpm-ostree system the package lands in /usr/lib/opt/oink.
login_item_points_at_the_installed_ui() {
    ssh_to 'cat "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/oink.desktop" 2>/dev/null' |
        grep -Eq 'Exec="(/usr/lib)?/opt/oink/oink" --autostart'
}

# The backend has to run for the capture to work at all.
backend_is_running() {
    ssh_to 'pgrep -af "opt/oink/oink-backend" || pgrep -a oink-backend'
}

check "the installed package" package_version
check "the rules come from the package" rules_from_the_package
check "the gamepad's mouse node is not a mouse" gamepad_node_is_not_a_mouse
check "libinput does not list the gamepad pointer" libinput_does_not_list_the_gamepad_pointer
check "the login item names the installed interface program" login_item_points_at_the_installed_ui
check "the backend runs" backend_is_running

if [[ "$failed" == 0 ]]; then
    echo "All checks passed."
else
    echo "Some checks failed."
fi
exit "$failed"
