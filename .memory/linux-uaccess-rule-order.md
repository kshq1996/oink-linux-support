---
name: linux-uaccess-rule-order
description: Why the Oink udev rule file must sort before 73-seat-late.rules, and how the seat user gets access to input devices
type: project
---

Facts about the access right for `/dev/input/event*` on Linux (2026-11-18, verified on the
Bazzite handheld):

- **systemd runs the `uaccess` builtin from `73-seat-late.rules`**:
  `TAG=="uaccess|xaccess-*", ENV{MAJOR}!="", RUN{builtin}+="uaccess"`. The builtin gives the
  seat's active user read/write access to the device node. The `uaccess` **tag** therefore has to
  be set *before* that file runs — a rule file named `99-…` is too late and the access right is
  never granted. Oink's rules are installed as `65-oink-input.rules`
  (`packaging/linux/build.py` `UDEV_RULES_NAME`) so they sort before it.
- **The tag alone is not enough for `ID_INPUT`:** the rule matches `SUBSYSTEM=="input",
  KERNEL=="event*"`; `ID_INPUT` is only set for the devices the `input_id` builtin recognises.
- **The desktop reads the input devices through file descriptors systemd hands it**, so a plain
  process cannot open them: the nodes belong to the `input` group and the desktop's user is
  usually not in it. Without the access right Oink cannot read the keyboard and cannot take the
  stick's pointer node away from the desktop (`EVIOCGRAB`).
- **The rule has to be applied to the devices that already exist.** `udevadm trigger
  --subsystem-match=input --action=change` re-runs the rules and the builtin applies the access
  right (`--action=add` also works but makes libinput see an "add" for a device it already has).
  Devices that appear later get it on their own.
- **A device created through `/dev/uinput` is an ordinary input device**: udev creates
  `/dev/input/eventN` for it (the event number is not the input number), and with the rule in place
  it gets the same access right — the compositor can read the injected device.

**Why:** the rule looked right but did nothing; the failure is silent (no access right, no error),
and only the rule file's name explains it.
**How to apply:** keep the rule file name ahead of `73-seat-late.rules` and keep
`test_the_package_installs_the_input_udev_rules` in `packaging/tests/test_linux_package.py`, which
pins the ordering. See also [[linux-gamepad-pointer-conflict]] and [[linux-wayland-input]].
