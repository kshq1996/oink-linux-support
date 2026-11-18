---
name: linux-gamepad-pointer-conflict
description: Why the shared pointer on the Bazzite handheld was stuck and dragged back by the right stick, and what the fix changes
type: project
---

Facts found while fixing the pointer on the Bazzite handheld (2026-11-18):

- **The device is a Bazzite 45 x86_64 (Legion Go S class) running Plasma 6 Wayland with
  XWayland on `:0`.** The Mac workstation is `10.204.96.27`; the handheld is `10.204.96.191`.
- **Two nodes drive the desktop cursor with a stick:**
  - `/dev/input/event5` "  Xbox360 Controller for Windows Mouse" — the xpad-style mouse node of
    the built-in controller (vendor `0db0:1901`). udev tags it `ID_INPUT_MOUSE=1`, so libinput
    treats it as a pointer.
  - `/dev/input/event18` "InputPlumber Mouse" — SteamOS's virtual mouse, made from the right
    stick (the triggers arrive as `REL_WHEEL`/`REL_HWHEEL`).
- **libinput honours `ID_INPUT_MOUSE=""`.** A udev rule
  `SUBSYSTEM=="input", ATTRS{name}=="*Controller*Mouse*", ENV{ID_INPUT_MOUSE}=""`
  makes the controller's mouse node disappear from `libinput list-devices`
  (SteamOS does the same in `60-steam-input.rules`).
- **The controller's keyboard node (`event4`) reports `KEY_A`**, so a capture that only excludes
  the pointer role turns the gamepad's buttons into keyboard keys for the peer.
- **The session's input nodes have no `uaccess` ACL** (the controller was plugged in before the
  session started), so a plain user process cannot open `event2`/`event5`/`event18`. The
  compositor reads them through the file descriptors systemd passes it. A udev rule
  `SUBSYSTEM=="input", ENV{ID_INPUT}=="1", TAG+="uaccess"` gives the seat user that access.
- **A Wayland session needs the environment of the desktop session.** Running the UI with the
  environment exported from `/proc/<plasmashell>/environ` still fails ("Failed to initialize
  GTK"); `systemd-run --user --unit=... <ui>` works, because the user manager carries the
  session variables.

**Why:** the pointer looked stuck and was dragged back by the stick, which is two different
faults: the deployed build injected through XTEST into XWayland (XWayland cannot move the
compositor's cursor), and the stick's pointer nodes fight the injected pointer in the same
cursor.

**How to apply:** the Wayland backend injects through uinput, leaves the gamepad's nodes out of
the capture, and takes the gamepad's pointer nodes away from the desktop (`EVIOCGRAB`) while the
peer holds the pointer; the Linux package installs the udev rule. See also
[[linux-wayland-input]] and [[linux-uinput-ioctl-numbers]].
