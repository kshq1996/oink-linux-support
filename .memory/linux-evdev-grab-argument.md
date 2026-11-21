---
name: linux-evdev-grab-argument
description: EVIOCGRAB takes the raw argument value, not a pointer, and the grab state has to be tracked by the caller
type: project
---

Facts about `EVIOCGRAB` in `crates/oink-input/src/linux/evdev.rs` (2026-11-22):

- **The ioctl's third argument is the decision, not a pointer to it.** The kernel
  (`drivers/input/evdev/evdev.c`, same in v2.6.32 and v6.12) does
  `case EVIOCGRAB: if (p) return evdev_grab(...); else return evdev_ungrab(...);`
  where `p` is the raw argument the caller passed (`evdev_ioctl_handler(file, cmd,
  (void __user *)arg, 0)`). So **non-zero = grab, zero = release**. The request number is
  `_IOW('E', 0x90, int)` = `0x4004_4590`; the size field is ignored by the kernel.
- **Passing a pointer to an `int` breaks the release.** The address is never NULL, so the
  "release" call is a *second grab*: the kernel answers `EBUSY` (the device is already
  grabbed), the caller sees a failure, and the node stays grabbed by Oink for the rest of
  the session — the desktop never sees the gamepad's mouse node again.
- **The caller has to remember the grab state.** A device that is not grabbed answers
  `EINVAL` to a release, and a second grab answers `EBUSY`; the device's own state field
  (`Device::grabbed`) is what makes "already in the target state" a no-op instead of a
  failed ioctl. A failed grab must not be remembered as held, otherwise the later
  release is skipped and the device is stranded.
- **Consequence for the pointer conflict:** with the pointer bug the published v0.3.6
  build could not hand the gamepad's pointer nodes back to the desktop, so the right
  stick kept fighting the injected pointer (the cursor was dragged back). The fix is
  `ioctl(fd, EVIOCGRAB, 1)` to grab and `ioctl(fd, EVIOCGRAB, 0)` to release.

**Why:** every mistake returns the same `EINVAL`/`EBUSY`, so the symptom was "the stick
still drags the cursor" with nothing in the log that named the cause.
**How to apply:** keep `the_grab_argument_is_the_value_and_not_a_pointer`,
`a_device_is_not_grabbed_twice` and `a_failed_grab_is_not_remembered_as_held` in
`evdev.rs`; verify with `OINK_LINUX_ARCH=arm64 tools/linux/check.sh test -p oink-input`.
See also [[linux-gamepad-pointer-conflict]] and [[linux-uinput-ioctl-numbers]].
