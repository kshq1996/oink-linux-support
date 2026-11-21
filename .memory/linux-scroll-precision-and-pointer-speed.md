---
name: linux-scroll-precision-and-pointer-speed
description: Why the shared scroll was too fast on Linux and how the pointer speed differs between a handheld screen and a desktop screen
type: project
---

Facts found while fixing the input feel on the Bazzite handheld (2026-11-19):

- **Both Linux injection paths rounded every fractional scroll up to a whole notch.** The X11
  path (`keymap::push_notches`) and the uinput path (`notch_steps`) did
  `value.abs().round().max(1.0)`, so a touchpad scroll of 0.1 notch became a full notch — the
  scroll ran several times faster than on the sending device. Windows injects the fraction
  (`MoveToWithDelta` carries the fractional wheel delta), so the Linux peer was the odd one out.
- **The fix keeps the fraction, and the unit is a whole notch.** The uinput path emits
  `REL_WHEEL` in **notches** (one unit = one notch); the X11 path has whole-notch buttons only,
  so it accumulates the fraction and emits the whole notches. The remainder must not be shared
  between axes, and a direction change must not cancel the fraction. The fraction is carried
  with error diffusion (`keymap::take_notches`: round, keep the rest), not truncation: a
  truncation makes the first small scroll do nothing and then over-scroll.
- **`REL_WHEEL` on a uinput device is a legacy wheel: one unit is one notch.** High-resolution
  wheels (1/120 notch) exist only on real devices: the **HID driver** emits a second event
  `REL_WHEEL_HI_RES` (0x0b) when the wheel axis carries the resolution 120, and the kernel's
  `uinput` driver has no way to declare that resolution (`UI_DEV_SETUP` covers absolute axes
  only), so a uinput device never gets it. libinput therefore reads a uinput wheel as notches.
  Sending 1/120 units was a 120× amplification: one flick scrolled the page to the end
  (fixed 2026-11-21 in `uinput.rs` `wheel_notches`).
- **The capture side counts notches too**: the legacy `REL_WHEEL` value of a real mouse is one
  unit per notch (the kernel sends the fraction separately in `REL_WHEEL_HI_RES`), so the
  capture must not divide it by 120 (that made the Linux source's scroll 120× too slow).
- **The pointer's speed is decided by the screen's physical size, not by its resolution.**
  The layout places every screen with its physical size, and the pointer covers
  `plane_gain / physical_width` of the screen per hand-plane unit. The handheld's screen
  (1920×1080, 376 mm wide) therefore moves the pointer 2.31× faster than the Windows 27"
  (3840×2160, 869 mm wide) with the default gains.
- **The knob for the incoming pointer is the per-screen remote pointer speed**
  (`ScreenPreference::remote_gain`, UI: "Remote pointer speed per screen"). It is
  target-only (`Topology::motion_gain` applies it only when the source is another device),
  so the handheld's own mouse and trackpad keep their speed. The handheld needs
  `376/869 = 0.4328`.
- **The device-level "pointer gain when controlled" (`device_gain`) is not the same knob.** It
  multiplies the per-screen gain, which is part of the plane gain on both sides: it slows the
  incoming pointer but speeds up the outgoing one by the same factor.
- **The speed mode (Physical / ScreenProportional) does not equalize the feel.** Its factor is
  the screen's diagonal over 686 mm, so the handheld gets 0.6465 and stays 1.49× faster than
  the 27" screen; the aspect ratios differ, so the diagonal is only an approximation.
- **The CLI can set the remote pointer speed** on a peer:
  `oink-agent call set_screen_remote_gain '{"screen":{"device":"<device-key>","display":"<display-key>"},"gain":0.4328}'`.
