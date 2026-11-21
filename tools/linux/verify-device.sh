#!/usr/bin/env bash
# Check the deployed Linux build on the test device.
#
# Usage: tools/linux/verify-device.sh <host>
#
# The checks answer the questions the pointer and scroll fixes raise:
#
# - which input path the backend takes (Wayland/uinput or X11/XTEST): the
#   environment of the running processes decides it, and the X11 path cannot
#   move the compositor's cursor;
# - whether the gamepad's pointer nodes are taken away from the desktop while
#   a peer holds the pointer (the log line "手柄指针节点的独占");
# - whether the mode decisions settle: they used to flap between remote and
#   local every few hundred milliseconds while the right stick fought the
#   injected pointer;
# - which values the virtual pointer device receives: one scroll notch is one
#   `REL_WHEEL` unit (a uinput device is a legacy wheel for libinput).
set -euo pipefail

HOST=${1:-}
if [[ -z "$HOST" ]]; then
    echo "usage: $0 <host>" >&2
    exit 2
fi

ssh "$HOST" bash -s <<'REMOTE'
set -u
LOG_DIR="$HOME/.local/state/oink/Logs"
LOG=$(ls -t "$LOG_DIR"/oink-2*.log 2>/dev/null | head -n 1)

echo "=== the package"
rpm -q oink 2>/dev/null || echo "oink is not installed as a package"

echo "=== the processes and their session environment"
for pid in $(pgrep -f 'oink-backend' ; pgrep -x oink); do
    printf '%s: %s\n' "$pid" "$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null)"
    tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null |
        grep -E '^(WAYLAND_DISPLAY|DISPLAY|XDG_SESSION_TYPE|XDG_CURRENT_DESKTOP)=' |
        tr '\n' ' '
    echo
done

echo "=== the session service"
systemctl --user is-active oink-ui.service 2>/dev/null || true
unit="$HOME/.config/systemd/user/oink-ui.service"
if [ -f "$unit" ]; then
    grep '^ExecStart=' "$unit" || true
else
    echo "no unit at $unit"
fi

echo "=== the gamepad pointer nodes"
if [ -n "${LOG:-}" ]; then
    grep -a '手柄指针节点的独占' "$LOG" | tail -n 5 || echo "no grab line in $LOG"
else
    echo "no agent log in $LOG_DIR"
fi

echo "=== the mode decisions (the last five)"
if [ -n "${LOG:-}" ]; then
    grep -a 'ModeDecision' "$LOG" | tail -n 5 || echo "no mode decision in $LOG"
fi

echo "=== the virtual pointer device"
node=""
for dir in /sys/class/input/event*; do
    name="${dir}/device/name"
    [ -r "$name" ] || continue
    if grep -q 'Oink' "$name"; then
        node="$dir"
        break
    fi
done
if [ -n "$node" ]; then
    echo "$node ($(cat "${node}/device/name"))"
    echo "the events it receives (one scroll notch is one REL_WHEEL unit):"
    echo "  timeout 5 cat /dev/input/$(basename "$node") | head -40"
else
    echo "no Oink virtual pointer registered"
fi
REMOTE
