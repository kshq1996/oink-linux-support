---
name: linux-x11-display-backend
description: XRandR facts behind the oink-display Linux backend (rotation encoding, scanout size, primary output, x11rb 0.13 quirks)
type: project
---

Facts learned while adding the X11/XRandR display backend to `crates/oink-display` (2026-11-17):

- **RandR rotation is a bitmask, not 0..3.** `randrproto.h` defines `RR_Rotate_0 = 1<<0`,
  `RR_Rotate_90 = 1<<1`, `RR_Rotate_180 = 1<<2`, `RR_Rotate_270 = 1<<3`, and the X server sends
  `crtc->rotation` in that form (`xserver/randr/rrcrtc.c`, `rep.rotation = crtc->rotation`).
  x11rb 0.13 mirrors it: `randr::Rotation::ROTATE90 == 1<<1`. Decode with `u16::from(rotation)`.
- **`RRGetCrtcInfo` reports the scanout size, not the mode size.** The server computes it with
  `RRCrtcGetScanoutSize()` (mode box transformed), so a 90°-rotated 1920x1080 CRTC already reports a
  1080x1920 footprint. `bounds` can therefore come straight from the CRTC, and the *mode* size from
  `RRGetScreenResourcesCurrent` has to be transposed for 90/270 to match the footprint.
- **There is no primary field in `RRGetScreenResourcesCurrent`.** The primary output is
  `RRGetOutputPrimary` (RandR 1.3); older servers reject it, and RandR 1.5's `RRGetMonitors` is not
  available everywhere (XWayland), so the backend sticks to the RandR 1.2 flow and falls back to
  "the CRTC that covers the root origin".
- **x11rb 0.13 details:** `randr::Output`/`Crtc`/`Mode` are plain `u32` aliases (no newtype);
  `Rotation`/`Connection`/`NotifyMask` are newtypes whose `PartialEq`/`BitOr` only exist with the
  `extra-traits` feature (the workspace does not enable it) → compare with `u8::from`/`u16::from` and
  build a mask with `NotifyMask::from(a | b)`. `cookie.check()` returns `Result<(), ReplyError>`, not
  `ConnectionError`.
- **The X server fabricates the panel size at 96 dpi when the output has no EDID.** The shared
  `crates/oink-display/src/extent.rs` only rejects the macOS 72-dpi fallback, so the Linux side
  rejects the 96-dpi one itself before calling `extent::reference_extent`.
- RandR output names (`RRGetOutputInfo.name`) are the connector names (`DP-1`, `HDMI-1`) and survive
  re-plugging the same port, while the output XID changes — the connector name is the stable key.
- **XWayland reports a scaled desktop in logical pixels while X11 clients live in the root
  window's pixel space.** On a 1920x1200 panel at 150%, `xdpyinfo` reports a 1920x1200 root but
  RandR reports the CRTC as 1280x800 (the logical size). The pointer, its capture and every
  injection happen in the root window space, so the display geometry has to be converted: the
  backend computes `RootScale::between(root_size, crtc_extent(outputs))` and scales the bounds
  and the mode size by it. Two axes that disagree mean "the output does not fill the root", not
  a scaled desktop, and the scale is then 1.

**Why:** the rotation encoding and the scanout-size semantics are easy to get wrong and silently
produce swapped or unrotated geometry; they are not visible from the x11rb docs alone. Without
the root-space conversion the injected pointer is confined to the logical size, which looks like
"the mouse only moves in a small area".
**How to apply:** reuse these answers when touching `crates/oink-display/src/linux/**`; verify with
`OINK_LINUX_ARCH=arm64 tools/linux/check.sh {check,clippy,test} -p oink-display` (container details
in [[linux-x11-input-backend]]).
