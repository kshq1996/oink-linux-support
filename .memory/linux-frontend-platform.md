---
name: linux-frontend-platform
description: How the oink web frontend models the Linux platform (platform type, source keys, layout and WebKit painting)
type: project
---

Facts about the Linux port in `apps/oink/src` (2026-11-17):

- The frontend platform type is `'mac' | 'win' | 'linux'` (`apps/oink/src/window/DesktopWindow.tsx`).
  `platformOf()` maps the backend host OS (`macos`/`windows`/`linux`) onto it; every component takes
  `Platform`, never a raw host string.
- Linux is a sidebar window like macOS: the titlebar back button, the request group in the nav stack
  and the taskbar jump list stay Windows-only. The Windows layout puts the pairing requests directly
  in `.nav-stack`; the macOS and Linux layout put them inside `.nav-view`.
- The Linux **source** keys are Caps Lock, Shift, Control, Alt and Super (ids `caps`, `shift`,
  `control`, `super`, `alt`, see `LINUX_KEYS`/`KEY_NAMES.linux` in `settings/prefs.ts`). The target
  device is still a Mac or a PC, so the target names keep the macOS table.
- Linux has no Command and no Option key: every modifier hint shows Control. `settings/Shortcuts.tsx`
  keeps `LABELS`/`SPOKEN`/`ORDER` per platform, with `Meta` spelled "Super" on Linux.
- The arrangement paints with CSS left/top on WebKit hosts (macOS **and** Linux's WebKitGTK) and with
  fractional-DPR `translate()` on Chromium (Windows). The Chromium branch is a Chromium-only
  workaround; applying it to a WebKit host reintroduces the anti-aliasing seams between screens.

**Why:** the frontend used to fold every non-Windows host into `mac`, so a Linux build silently
claimed macOS-only abilities and showed wrong modifier names.
**How to apply:** when touching the frontend for Linux, keep the platform branching explicit, and
verify with `OINK_LINUX_ARCH=arm64 tools/linux/frontend.sh`.
