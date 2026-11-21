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

**The address depends on the corporate VPN (2026-11-21).** The handheld has an
office-LAN address (`192.168.22.55`) and a corporate one (`10.204.96.191`); the
latter is only reachable while the workstation holds an aTrust session
(`/Applications/SSLVPN.app`, the gateway is `vpn.xyyl.com:65201`). When that
session expired (`passportTokenInvalid` in
`~/Library/Application Support/aTrust/logs/aTrustAgent_plugins_aTrustCore_h_e.log`),
the workstation lost its `10.204.96.27` address, the route to `10.204.96.0/24`
went with it, and the handheld became unreachable even though the office LAN
(`192.168.0.0/16`, the workstation sits at `192.168.83.180`) was still up. A
full sweep of that LAN finds no SSH host at all, so the handheld is not on it.
Reaching the device therefore means re-authenticating the VPN, not waiting for
the device.

**The workstation's own network decides whether the device is reachable**
(2026-11-21, afternoon). The handheld was healthy: after the 12:33 reboot it
answered SSH and the deployment checks passed. It became unreachable when the
workstation moved to another site (`192.168.187.110`): from there the whole
`192.168.22.0/24` subnet does not answer ICMP either (a ping sweep finds
nothing at all), and the route to `10.204.96.0/24` was gone with the expired
aTrust session. So an unreachable handheld means "check the workstation's
addresses and routes first", not "the device is off".

**The peer machine can tell whether the device is alive.** `oink-agent call
device_cards` (on macOS: `/Applications/Oink.app/Contents/MacOS/oink-agent`) lists every
paired device with `connection.kind` and `lastSeenMs`. The handheld shows up under the
name of its address (`192.168.22.55`, os `linux`); a device that is off shows `offline`
and the epoch of its last sighting. On 2026-11-21 the linux peer had not been seen since
2026-11-19 17:45 while two Windows peers had been seen that same night: the device
itself was off or off-network, not merely unreachable over SSH.

**Why:** the device looks like "the build is slow" and then like "the machine is broken",
which sends the debugging in the wrong direction; the build script has to be written for a
host that disappears and comes back with a different address.

**How to apply:** build on the device only with few jobs and inside a retry loop, keep the
source and the target directory on `/var/home`, check the sysroot's `lib` directory instead
of the component list, and expect the address to change (look the host up by name or by the
MAC in the ARP table). See also [[linux-test-suite]], [[linux-uinput-ioctl-numbers]] and
[[linux-uaccess-rule-order]].
