---
name: linux-handheld-build-fragility
description: The Bazzite handheld goes offline during a Rust build, so the device build needs few jobs and has to survive reboots
type: project
---

Facts about building on the Bazzite handheld (the test device of this fork, 2026-11-18/19):

- **The machine goes offline under a sustained Rust build.** Twice in one session the device
  stopped answering ARP and SSH (`ssh: connect to host 192.168.22.55 port 22: Host is down`)
  while a `cargo build --release` was running: first with 13 jobs, then with 4 jobs. It came
  back after a while with a new DHCP lease (10.204.96.191 → 192.168.22.55) and an uptime of
  six hours, so it reboots rather than only losing the network.
- **A reboot can break the rustup toolchain.** After the first crash the toolchain's
  `lib/rustlib/x86_64-unknown-linux-gnu/lib` was empty, and every build failed with
  `error[E0463]: can't find crate for std` — while `rustup component list` still reported
  `rust-std` as installed. The check that tells the truth is the file list under the sysroot,
  not the component list.
- **The build survives the crashes.** Cargo is incremental and the target directory lives on
  `/var/home` (the persistent partition), so a restarted build keeps what it compiled. The
  build therefore runs with few jobs (`-j 3`), repairs the toolchain when the sysroot is
  incomplete and retries; each attempt makes progress.
- **The session user can write `/dev/uinput`** through logind's `uaccess` ACL
  (`user:kangshaoqi:rw-`), so the device-side test really registers a virtual device.
- **`/usr` is read only** on the immutable image, so a rule file cannot be dropped there
  during a live test: `/etc/udev/rules.d/65-oink-input.rules` is the place for it (and an
  rpm-ostree install puts the packaged rule into the next deployment, which is a different
  thing).

**Why:** the device looks like "the build is slow" and then like "the machine is broken",
which sends the debugging in the wrong direction; the build script has to be written for a
host that disappears and comes back with a different address.

**How to apply:** build on the device only with few jobs and inside a retry loop, keep the
source and the target directory on `/var/home`, check the sysroot's `lib` directory instead
of the component list, and expect the address to change (look the host up by name or by the
MAC in the ARP table). See also [[linux-test-suite]], [[linux-uinput-ioctl-numbers]] and
[[linux-uaccess-rule-order]].
