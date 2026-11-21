#!/usr/bin/env bash
# Check the Oink install on the test device.
#
# The checks are the ones that decide whether the shared pointer survives the
# gamepad's right stick: the gamepad's mouse node must not be a mouse for the
# desktop, the rules must come from the package, and the login item must name
# the installed interface program.
#
# The other checks answer the rest of the pointer and scroll fixes: the
# backend has to run the Wayland path (the X11 path injects through XTEST,
# which cannot move the compositor's cursor), the session unit has to keep
# the shell's dollar signs (systemd expands `${socket##*/}` as its own
# variable and hands the backend an empty display name), the gamepad's pointer
# nodes have to be taken away from the desktop while a peer holds the
# pointer, and the virtual pointer has to receive whole scroll detents (a
# uinput device is a legacy wheel for libinput: one `REL_WHEEL` unit is one
# detent).
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

# The backend has to run for the capture to work at all, and it has to be the
# installed one. The process name has to match exactly: a pattern over the
# command line also matches the shell that runs this very check.
backend_is_running() {
    ssh_to 'pid=$(pgrep -x oink-backend | head -1); [ -n "$pid" ] && readlink /proc/$pid/exe | grep -q "/opt/oink/oink-backend"'
}

# The backend has to take the Wayland path. Without `WAYLAND_DISPLAY` it
# falls back to XTEST into XWayland, and XTEST cannot move the compositor's
# cursor: the visible pointer then only moves when the stick drags it.
backend_uses_the_wayland_path() {
    ssh_to '
        pid=$(pgrep -x oink-backend | head -1)
        if [ -z "$pid" ]; then
            echo "the backend is not running"
            exit 1
        fi
        if tr "\0" "\n" < "/proc/$pid/environ" | grep -q "^WAYLAND_DISPLAY="; then
            echo "the backend runs with $(tr "\0" "\n" < "/proc/$pid/environ" | grep "^WAYLAND_DISPLAY=")"
            exit 0
        fi
        echo "the backend has no WAYLAND_DISPLAY: it injects through XTEST"
        exit 1'
}

# The session unit has to keep the shell's dollar signs: systemd expands
# `${socket##*/}` as its own variable and hands the backend an empty display
# name.
session_service_keeps_the_display_name() {
    ssh_to '
        unit="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/oink-ui.service"
        if [ ! -f "$unit" ]; then
            echo "no unit at $unit"
            exit 1
        fi
        if ! grep -q "ExecStart=.*\\$\\${socket##\*/}" "$unit"; then
            echo "the unit does not escape the socket variable: systemd would expand it"
            exit 1
        fi
        echo "the unit keeps the display name"'
}

# The nodes that move the desktop cursor have to be taken away while a peer
# holds the pointer; the log says which ones were grabbed.
gamepad_pointer_nodes_are_taken_away() {
    ssh_to '
        log=$(ls -t "$HOME"/.local/state/oink/Logs/oink-2*.log 2>/dev/null | head -n 1)
        if [ -z "$log" ]; then
            echo "no agent log"
            exit 1
        fi
        line=$(grep -a "手柄指针节点的独占" "$log" | tail -n 1)
        if [ -z "$line" ]; then
            echo "no grab line in $log (no peer held the pointer yet?)"
            exit 1
        fi
        echo "$line"
        case "$line" in
        *"take_over=true"*) ;;
        *) echo "the last grab did not take the nodes over"; exit 1 ;;
        esac
        # The nodes that are taken over have to be named: the stick drags the
        # pointer back through every node the desktop reads as a mouse.
        case "$line" in
        *"names=[]"*) echo "no node was taken over"; exit 1 ;;
        esac
        # A name is not a grab: the node has to be *held*. The count is the
        # number of nodes the kernel gave us; it is zero when every grab was
        # refused (the desktop keeps the stick, and the pointer is dragged
        # back).
        held=$(printf "%s" "$line" | grep -ao "held=[0-9]*" | head -n 1 | cut -d= -f2)
        if [ -z "$held" ] || [ "$held" -eq 0 ]; then
            echo "no node is actually held (held=${held:-?})"
            exit 1
        fi
        echo "$held node(s) are held away from the desktop"'
}

# The release has to reach the desktop as well. The grab argument is the
# decision itself, so a "release" that still passes a pointer is a second
# grab: the kernel answers EBUSY and the node stays with Oink, and the
# desktop never gets the stick back.
gamepad_pointer_nodes_are_released_again() {
    ssh_to '
        log=$(ls -t "$HOME"/.local/state/oink/Logs/oink-2*.log 2>/dev/null | head -n 1)
        if [ -z "$log" ]; then
            echo "no agent log"
            exit 1
        fi
        line=$(grep -a "手柄指针节点的独占" "$log" | grep -a "take_over=false" | tail -n 1)
        if [ -z "$line" ]; then
            echo "no release line in $log (the peer never gave the pointer back?)"
            exit 1
        fi
        echo "$line"
        held=$(printf "%s" "$line" | grep -ao "held=[0-9]*" | head -n 1 | cut -d= -f2)
        if [ -n "$held" ] && [ "$held" -ne 0 ]; then
            echo "$held node(s) are still held after the release"
            exit 1
        fi
        echo "the nodes went back to the desktop"'
}


# The virtual pointer has to be registered, and it has to receive whole
# scroll detents: `REL_WHEEL` is the legacy wheel, one unit is one detent.
virtual_pointer_is_registered() {
    ssh_to '
        node=""
        for dir in /sys/class/input/event*; do
            name="${dir}/device/name"
            [ -r "$name" ] || continue
            if grep -q "Oink" "$name"; then
                node="$dir"
                break
            fi
        done
        if [ -z "$node" ]; then
            echo "no Oink virtual pointer registered"
            exit 1
        fi
        event="/dev/input/$(basename "$node")"
        echo "$event ($(cat "${node}/device/name"))"
        echo "read the events while a peer scrolls (one detent is one REL_WHEEL unit):"
        echo "  timeout 5 cat $event | head -40"'
}

# The mode decisions have to settle. They used to flap between remote and
# local every few hundred milliseconds while the stick fought the injected
# pointer.
mode_decisions_settle() {
    ssh_to '
        log=$(ls -t "$HOME"/.local/state/oink/Logs/oink-2*.log 2>/dev/null | head -n 1)
        if [ -z "$log" ]; then
            echo "no agent log"
            exit 1
        fi
        grep -a "ModeDecision" "$log" | tail -n 5'
}

check "the installed package" package_version
check "the rules come from the package" rules_from_the_package
check "the gamepad's mouse node is not a mouse" gamepad_node_is_not_a_mouse
check "libinput does not list the gamepad pointer" libinput_does_not_list_the_gamepad_pointer
check "the login item names the installed interface program" login_item_points_at_the_installed_ui
check "the backend runs" backend_is_running
check "the backend uses the Wayland path" backend_uses_the_wayland_path
check "the session unit keeps the display name" session_service_keeps_the_display_name
check "the gamepad's pointer nodes are taken away" gamepad_pointer_nodes_are_taken_away
check "the gamepad's pointer nodes are released again" gamepad_pointer_nodes_are_released_again
check "the virtual pointer is registered" virtual_pointer_is_registered
check "the mode decisions settle" mode_decisions_settle

if [[ "$failed" == 0 ]]; then
    echo "All checks passed."
else
    echo "Some checks failed."
fi
exit "$failed"
