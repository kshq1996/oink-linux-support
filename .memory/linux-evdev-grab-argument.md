---
name: linux-evdev-grab-argument
description: EVIOCGRAB takes the raw argument value, not a pointer, and the grab state has to be tracked by the caller
type: project
---

Facts about `EVIOCGRAB` in `crates/oink-input/src/linux/evdev.rs` (2026-11-22):

- **The ioctl's third argument is the decision, not a pointer to it.** The kernel
  (`drivers/input/evdev/evdev.c`) does
  `case EVIOCGRAB: if (p) return evdev_grab(...); else return evdev_ungrab(...);`
  where `p` is the raw argument the caller passed (`evdev_ioctl_handler(file, cmd,
  (void __user *)arg, 0)`), so **non-zero = grab, zero = release**. The request number is
  `_IOW('E', 0x90, int)` = `0x4004_4590`; the size field is ignored by the kernel.
  (The upstream copy under `/tmp/evdev-6.12.c` shows this. The other local copy,
  `/tmp/evdev-2632.c`, is *not* v2.6.32 — it contains `compat_ptr`, which postdates 5.18 —
  so do not cite it as evidence for an old kernel.)
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
- **Measured on a 7.x kernel** (the OrbStack VM, kernel `7.2.12-orbstack`, the same
  generation as the handheld's `7.3.4`): a virtual REL device created through
  `/dev/uinput`, its event node opened twice. `ioctl(fd, EVIOCGRAB, 1)` returns 0 and the
  **other reader stops receiving events** (so a grab really takes the node away from the
  desktop); `ioctl(fd, EVIOCGRAB, 0)` returns 0 and the other reader receives events again;
  a pointer to an `int` holding 0 answers `EBUSY` (errno 16) — it is a second grab, not a
  release. The probe lives in `/tmp/probe/grab_probe_vm.py` (throwaway).
  The OrbStack VM is the only place here that can do this: the project container's
  `/dev/uinput` is root-owned and `mknod` is refused, and `/tmp` is `nodev` there, so the
  event node has to be created under the VM's root filesystem (`/root/...`), not `/tmp`.
- **The grab is exclusive and device-level** (measured on the same 7.2.12 kernel, two
  readers on one event node): the reader that holds the grab gets the events, the other
  one gets nothing; a second client that tries to grab the same device gets `EBUSY`
  (errno 16). So a *failed* grab means somebody else holds the node (the desktop's
  compositor or SteamOS's InputPlumber), not that the argument was wrong — the
  `手柄指针节点的独占` log line names the nodes that failed for exactly that reason.
  A grab that succeeded once is silent to the desktop until it is released, which is what
  takes the right stick away from the injected pointer.

**Why:** every mistake returns the same `EINVAL`/`EBUSY`, so the symptom was "the stick
still drags the cursor" with nothing in the log that named the cause.
**How to apply:** keep `the_grab_argument_is_the_value_and_not_a_pointer`,
`a_device_is_not_grabbed_twice` and `a_failed_grab_is_not_remembered_as_held` in
`evdev.rs`; verify with `OINK_LINUX_ARCH=arm64 tools/linux/check.sh test -p oink-input`.
See also [[linux-gamepad-pointer-conflict]] and [[linux-uinput-ioctl-numbers]].
