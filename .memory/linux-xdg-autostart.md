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

**Why:** the login item and the "the keyboard does not work" symptoms look unrelated, but both
come from code paths that were not wired for Linux, and a republished package that does not
install leaves the tester on the old build.

**How to apply:** when touching `crates/oink-autostart/**` verify with
`OINK_LINUX_ARCH=arm64 tools/linux/check.sh {check,clippy,test} -p oink-autostart`; when
republishing see also [[linux-release-platform-set]] and [[linux-rpm-packaging]].
