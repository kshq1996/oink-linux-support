---
name: linux-package-verification
description: How to verify a published Linux package in a container (install, run under Xvfb, check the wallpaper and the login item)
type: project
---

The recipe for checking a published Oink Linux package (2026-11-18, used for the
v0.3.6 republish):

- Install the package in a real distribution image and let the package manager
  resolve the dependencies: `docker run --rm --platform linux/arm64 -v <dir>:/work:ro
  fedora:41 bash -lc 'dnf install -y /work/Oink-<version>-linux-arm64.rpm xorg-x11-server-Xvfb'`.
  The install is the slow part (about four minutes), the checks below take seconds.
- The background process needs a display and a desktop configuration, and neither has to be
  a real desktop: `Xvfb :99 -screen 0 1280x1024x24`, `XDG_CURRENT_DESKTOP=KDE`,
  `XDG_CONFIG_HOME=/tmp/xdg`, and a hand-written
  `/tmp/xdg/plasma-org.kde.plasma.desktop-appletsrc` with
  `[Containments][1][Wallpaper][org.kde.image][General]` and `Image=file:///tmp/wallpaper.png`.
  The agent then reads that picture and publishes it.
- The checks (all through `oink-agent call <method> <json>`):
  - `displays` → the display list (the background process is up).
  - `wallpapers '{"refresh":true}'` → one entry per screen; `image` is a **data URL**
    (`data:image/jpeg;base64,…`), not raw base64, so a check for a leading `/9j` fails.
  - `set_launch_at_login '{"enabled":true}'` → `"configured": true`, and the entry lands in
    `$XDG_CONFIG_HOME/autostart/oink.desktop` with `Exec="/opt/oink/oink" --autostart`
    (the interface program next to the backend, not the backend itself).
- The login item is written to `$XDG_CONFIG_HOME`, so a check that looks in
  `~/.config/autostart` finds nothing when the run set `XDG_CONFIG_HOME` elsewhere.

**Why:** a package that installs but cannot start, or starts without the feature the release
was about, is the same failure in a quieter form; the container recipe proves both without a
desktop machine.
**How to apply:** run it against the artifacts the release actually published (the checksums
in `SHA256SUMS` first), not against a local build. See also [[linux-release-platform-set]],
[[linux-wallpaper-backend]] and [[linux-xdg-autostart]].
