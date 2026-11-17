---
name: linux-x11-input-backend
description: Gotchas when building the oink-input Linux/X11 backend (x11rb 0.13, container arch, XTEST/XRecord semantics)
type: project
---

Facts learned while adding the X11 input backend to `crates/oink-input` (2026-11-17):

- The workspace pins `x11rb = "0.13"` (arboard's clipboard backend requires it). In 0.13 there is no
  `x11rb::protocol::uprot`; `ResourceId` is a plain `u32` and the record context type is `u32`.
- `x11rb::connection::Connection` (not `RequestConnection`) carries `setup()`, `generate_id()` and
  `extension_information()` in 0.13 — importing the wrong trait yields "method not found".
  Protocol calls (`query_pointer`, `get_keyboard_mapping`) live in `xproto::ConnectionExt`.
- XTEST absolute motion is expressed in **root-window pixels**, not the 0..=65535 device range
  (measured on a 1280x1024 screen: detail 0 + x=640 lands on pixel 640). Relative motion uses detail 1.
- XRecord needs two connections: a control connection (create/disable the context) and a data
  connection (`record_enable_context` returns an iterator of replies; it ends on category 5).
- On this workstation the `oink-linux-build:0.3.6-amd64` image is actually an **arm64** image, so
  `tools/linux/check.sh` and `tools/linux/frontend.sh` must be run with `OINK_LINUX_ARCH=arm64`.
- `tools/linux/frontend.sh` hangs (no output, no exit) when the OrbStack docker daemon disappears
  mid-run; it took an hour to hit the 3600 s deadline. Pipe it through `tee <log>` and read the
  log instead of `tail`, and re-check `docker info` when a run produces nothing.
- `crates/oink-input/src/lib.rs` gates `mod macos;` with `#[cfg(target_os = "macos")]`; without it the
  crate cannot build on Linux (objc2/libc are macOS-only dependencies).

**Why:** these are the failure modes that cost the most time and are invisible from the code alone.
**How to apply:** reuse these answers when touching `crates/oink-input/src/linux/**` or the container
tooling; verify with `tools/linux/check.sh {check,clippy,test} -p oink-input`.
