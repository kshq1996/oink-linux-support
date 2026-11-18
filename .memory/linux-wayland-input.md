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
- **Device counts are not pixels.** The kernel reports the mouse's own units (its DPI), the desktop
  uses logical pixels (96 DPI by definition). The factor is `mouse DPI / 96`
  (`linux/scale.rs`), the DPI comes from the udev hardware database (`MOUSE_DPI`) and defaults to
  1000. `OINK_LINUX_POINTER_COUNTS_PER_PIXEL` overrides the factor.
- **The compositor's pointer acceleration is not reproducible.** The counts are the raw ones, the
  cursor moves by the accelerated amount, and the acceleration is speed dependent. KDE's setting
  `~/.config/kcminputrc` `[Libinput] PointerAccelerationProfile` (2 = adaptive, 0 = none) decides
  how wrong the shared pointer's speed is; with the acceleration off the conversion is exact.
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
