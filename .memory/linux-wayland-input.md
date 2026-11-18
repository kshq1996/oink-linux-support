---
name: linux-wayland-input
description: How the Oink Wayland input backend injects (uinput) and captures (evdev) input, and why the shared pointer's speed is only approximate
type: project
---

Facts about the Wayland input backend (`crates/oink-input/src/linux/`, 2026-11-18):

- **X11 is only a guest under Wayland.** XTEST cannot move the compositor's cursor and XRecord
  never sees the pointer, so on a Plasma session the pointer looked "stuck in a region": the local
  cursor stayed where it was while the peer kept driving it. The backend now picks the Wayland
  implementation when `WAYLAND_DISPLAY` is set (`linux/mod.rs::wayland_session`) and keeps XTEST +
  XRecord for X11 sessions.
- **Injection** (`linux/uinput.rs`): a virtual pointer device in `/dev/uinput` whose absolute range
  (`ABS_X`/`ABS_Y`) is the **desktop size in logical pixels**, with `INPUT_PROP_DIRECT` set so the
  compositor maps it one-to-one instead of treating it as a touchscreen. The device is rebuilt when
  the desktop size changes (the range is fixed when the device is created). The wheel unit is
  1/120 of a detent: a full detent is `REL_WHEEL = 120`, not 1.
- **Capture** (`linux/evdev.rs`, `linux/wayland.rs`): the kernel's `/dev/input/event*` devices are
  read directly. Key presses are exact (no acceleration), pointer motion is not.
- **A gamepad is a mouse to the kernel.** The Xbox controller on the test device opens a
  `mouse0`/`event5` node with `REL_X`/`REL_Y` and `BTN_LEFT`, so a capture that takes every
  relative device would let the right stick drive the shared pointer and the gamepad's buttons
  click it. udev cannot help: that node is labelled `ID_INPUT_MOUSE=1`. The capture therefore
  drops the pointer and button roles of devices whose name looks like a gamepad
  (`linux/evdev.rs::is_gamepad`).
- **The backend needs the desktop session's `XAUTHORITY`.** The display and clipboard backends
  still talk to XWayland, and without the session's cookie the X connection fails with
  "Authorization required, but no authentication protocol specified" — the input backend reports
  it and the whole agent exits with code 1 before any log file exists (the Linux service path
  never calls `logs::init_service`). A service started over SSH without
  `XAUTHORITY=/run/user/<uid>/xauth_*` therefore dies silently; the environment has to be copied
  from the session (`/proc/<plasmashell>/environ`).
- **Device counts are not pixels.** The kernel reports the mouse's own units (its DPI), the desktop
  uses logical pixels (96 DPI by definition). The factor is `mouse DPI / 96`
  (`linux/scale.rs`), the DPI comes from the udev hardware database (`MOUSE_DPI`) and defaults to
  1000. `OINK_LINUX_POINTER_COUNTS_PER_PIXEL` overrides the factor.
- **The compositor's pointer acceleration is not reproducible.** The counts are the raw ones, the
  cursor moves by the accelerated amount, and the acceleration is speed dependent. KDE's setting
  `~/.config/kcminputrc` `[Libinput] PointerAccelerationProfile` (2 = adaptive, 0 = none) decides
  how wrong the shared pointer's speed is; with the acceleration off the conversion is exact.
- **There is no absolute pointer position under Wayland, and XWayland does not help.** On the test
  device (Bazzite, Plasma 6, XWayland on `:0`) a uinput move of the compositor's cursor left the X11
  pointer at its old place (`xdotool getmouselocation` unchanged): the X11 pointer is a guest of its
  own and the compositor never updates it. `KWin`'s D-Bus interface has no cursor position either.
  The capture therefore tracks the position itself, and every injected move re-seeds it
  (`move_to`): the injection is the only accurate source of where the compositor put the cursor.
  The tracked position starts at the desktop centre, so the *first* edge handoff fires early; after
  the first parking it is accurate.
- **The devices are reachable without the `input` group.** logind's `uaccess` ACL already gives the
  user of the active seat `rw` on `/dev/uinput` and `/dev/input/event*` (`getfacl` shows
  `user:<name>:rw-`), so no udev rule and no group membership are needed; a plain SSH session is
  enough to open both.
- **The test device's own input devices.** Bazzite 45 (x86_64) with a `NVTK0603` touchpad
  (`ABS_X`/`ABS_Y`, no `BTN_LEFT` — an absolute device, so it is not a pointer source) and an
  `InputPlumber Mouse` virtual node (`REL_X`/`REL_Y`/`REL_WHEEL`, no `BTN_LEFT`). The keyboard is
  the `AT Translated Set 2 keyboard` node.
- **The RemoteDesktop portal works on the device** (`CreateSession`, `SelectDevices` and `Start`
  all succeed, the consent dialog appears and can be approved), but the v2 API (`ConnectToEIS`)
  needs the EIS protocol, whose sources live on `gitlab.freedesktop.org/libei` (not mirrored on
  GitHub). A pure Rust implementation exists as the `reis` crate, so the exact capture is possible
  with that dependency plus a D-Bus client; the cost is a consent dialog at every start.

**Why:** the pointer's speed and the handoff timing depend on the counts-to-pixels factor, and the
only exact source of the cursor's position under Wayland is the portal's EIS stream.
**How to apply:** verify with
`OINK_LINUX_ARCH=arm64 tools/linux/check.sh {test,clippy} -p oink-input`; on the test machine the
injection can be checked without a user by reading the virtual device
(`cat /dev/input/eventN` of the "Oink Virtual Pointer" device).
See also [[linux-x11-input-backend]] and [[linux-test-suite]].
