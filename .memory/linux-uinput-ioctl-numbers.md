---
name: linux-uinput-ioctl-numbers
description: The uinput ioctl numbers, direction bits and struct layouts the Oink Wayland injector needs, and how they were verified on the device
type: project
---

Facts about `/dev/uinput` in `crates/oink-input/src/linux/uinput.rs` (2026-11-18):

- **The ioctl numbers are positional, not the obvious ones.** From `/usr/include/linux/uinput.h`:
  `UI_SET_EVBIT` = nr 100, `UI_SET_KEYBIT` = 101, `UI_SET_RELBIT` = 102, `UI_SET_ABSBIT` = 103,
  `UI_SET_PROPBIT` = **110**, `UI_DEV_SETUP` = **3** (92 bytes), `UI_ABS_SETUP` = **4** (28 bytes),
  `UI_DEV_CREATE` = **1** (`_IO`, no size). The number is not derivable from the order in the header,
  and the kernel answers every wrong number with `EINVAL` — a wrong `UI_DEV_SETUP` number looks like
  "the ioctl is unsupported" rather than "the device failed to register".
- **The `UI_SET_*BIT` ioctls take the value, not a pointer.** The header says
  `_IOW(UINPUT_IOCTL_BASE, 100, int)`, but the kernel implements them as
  `uinput_set_bit(arg, evbit, EV_MAX)` (`drivers/input/misc/uinput.c`, the same in 4.19, 5.10,
  5.15 and 6.1): the *raw ioctl argument* is the code itself. A pointer is a huge number, so it
  exceeds `EV_MAX` and the kernel answers `EINVAL`. Measured on the device: `ioctl(fd, UI_SET_EVBIT, 2)`
  works, `ioctl(fd, UI_SET_EVBIT, &2)` fails with `EINVAL`.
  `UI_DEV_SETUP` and `UI_ABS_SETUP` are the opposite: they take a pointer to the structure.
  `EVIOCGRAB` (the input core) also takes a pointer to an `int`.
  The bits must be set **before** `UI_DEV_CREATE`: afterwards the kernel answers `EINVAL`.
- **The direction bits matter.** `_IOW` is direction **1**, `_IO` is direction **0**; `_IOWR` is 3.
  The evdev module uses direction 2 for its `_IOR` calls (`EVIOCGBIT`) — that is correct, because
  `_IOR` is 2, but the two modules must not share one constant.
- **`struct uinput_abs_setup` is 28 bytes**, not 24: `__u16 code`, two bytes of padding, then a
  whole `struct input_absinfo` (`value`, `minimum`, `maximum`, `fuzz`, `flat`, `resolution`).
  The header comments the `filler` field out, but `input_absinfo` needs 4-byte alignment, so the
  padding is there anyway.
- **The device is only registered after `UI_DEV_CREATE`.** `UI_DEV_SETUP` alone stores the
  description; writing events before the create call fails with `EINVAL` (the write is treated as
  an event write, and there is no device yet). The old "write a `uinput_user_dev` blob" path is gone
  from modern kernels.
- **How to verify without the Rust build:** a Python probe on the device reproduces the whole
  sequence (`fcntl.ioctl` with the same `_IOC` encoding, the bit ioctls with plain integers). Run it
  as the session user; `/dev/uinput` is writable through logind's `uaccess` ACL. The registered
  device then shows up under `/sys/class/input/input*/name` — the event node number is *not* the
  input number, so look the node up through the `device` symlink.

**Why:** every one of these mistakes returns the same `EINVAL`, so the injector failed to start on
the device with "uinput ioctl 失败：Invalid argument" and no hint which call was wrong. The
published v0.3.6 Linux package is built this way, so the agent exits with code 1 before it writes
any log file and Oink does not start on a Wayland device at all.
**How to apply:** keep `the_uinput_ioctl_numbers_match_the_kernel_header`,
`the_absolute_axis_setup_has_the_kernel_layout` and `the_injector_registers_a_virtual_device` in
`uinput.rs`; the last one creates a real virtual device when `/dev/uinput` is writable and would
catch the argument convention. Verify on the device with
`OINK_LINUX_ARCH=arm64 tools/linux/check.sh {test,clippy} -p oink-input` plus a device run.
See also [[linux-wayland-input]].
