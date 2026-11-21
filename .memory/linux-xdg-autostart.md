---
name: linux-xdg-autostart
description: How the Oink login item works on Linux (XDG autostart entry) and how the fork reinstalls a republished package
type: project
---

Facts about the Linux login item and the republishing flow (2026-11-17):

- The Linux login item is a **user-level XDG autostart entry**:
  `crates/oink-autostart/src/linux.rs` writes `$XDG_CONFIG_HOME/autostart/oink.desktop`
  (`$XDG_CONFIG_HOME` must be absolute, otherwise `$HOME/.config`) with
  `Exec="<exe>" --autostart`, `Terminal=false` and both `X-GNOME-Autostart-enabled=true` and
  `Hidden=false`. Only the UI executable (`oink`) may register it; the command line tool and the
  backend report `main_app_required`.
- The state is read **from the file, not from the desktop**: the file being present means the entry
  is registered, `Hidden=true` or `X-GNOME-Autostart-enabled=false` means the desktop switched it
  off (`requires_approval`), and a different `Exec` means another installation
  (`different_installation`). There is no cross-desktop settings page, so
  `open_system_settings` reports `unsupported_platform`.
- `crates/oink-autostart/src/lib.rs` has to name the module: `mod linux;` under
  `#[cfg(target_os = "linux")]` plus the `linux` branch in `user_status`, `set_user_enabled` and
  `open_system_settings`. Without those the login item is silently "unsupported platform".
- **Republishing the same version does not upgrade an installed package.** The public package's
  release number is the release sequence, so a rebuilt 0.3.6 with sequence 1 is still `0.3.6-1`:
  `rpm-ostree install` refuses it with "already installed". On an immutable system remove the
  overlay first (`rpm-ostree uninstall oink`) and install the new file afterwards. A local RPM
  file is installed without a signature check, so `--nosignature` is not a valid option.

**The handheld's session is a gamescope one, and it runs no autostart entries**
(2026-11-21). `/etc/sddm.conf.d/zz-holo-autologin.conf` sets
`Session=gamescope-session-ogui-steam.desktop`, so the session runs
`gamescope-session-plus ogui-steam` with `gamescope` as the compositor. There
is no `systemd-xdg-autostart` in that session: the XDG autostart entry the
product writes is **never executed**, so the interface program does not start
by itself there. The deployment therefore installs a user service
(`~/.config/systemd/user/oink-ui.service`) that starts the installed program.

- **The session's environment is what the program needs.** The compositor's
  socket is `/run/user/1000/gamescope-0` (a desktop session names it
  `wayland-0`), and XWayland serves `:0` and `:1` **without** an Xauthority
  cookie. The interface program fails with "Failed to initialize GTK"
  (tao → `gtk::rt::init`) when `WAYLAND_DISPLAY` is missing, and the backend
  fails with "连接 X 服务器失败：$DISPLAY variable not set" when `DISPLAY`
  is missing. The `systemd --user` environment of a gamescope session carries
  `XDG_CURRENT_DESKTOP=gamescope` but **no** `WAYLAND_DISPLAY` and no
  `DISPLAY`, so the program has to be started with them.
- The service looks the socket up when it starts
  (`ls /run/user/$(id -u)/wayland-* /run/user/$(id -u)/gamescope-[0-9]`), and
  `Restart=on-failure` covers the ordering: the service may start before the
  compositor exists.
**Why:** the login item and the "the keyboard does not work" symptoms look unrelated, but both
come from code paths that were not wired for Linux, and a republished package that does not
install leaves the tester on the old build.

**How to apply:** when touching `crates/oink-autostart/**` verify with
`OINK_LINUX_ARCH=arm64 tools/linux/check.sh {check,clippy,test} -p oink-autostart`; when
republishing see also [[linux-release-platform-set]] and [[linux-rpm-packaging]].
