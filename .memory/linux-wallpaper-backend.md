---
name: linux-wallpaper-backend
description: How the oink-display Linux backend finds the desktop wallpaper (Plasma appletsrc, GNOME gsettings) and renders per-screen thumbnails
type: project
---

Facts learned while adding the Linux wallpaper backend to `crates/oink-display` (2026-11-18):

- The wallpaper the arrangement canvas draws comes from `DisplayBackend::wallpapers()`. macOS and
  Windows implemented it; the Linux backend returned `None`, so the Linux device's screens were a flat
  grey canvas — locally and on the peers' machines. The fix belongs in the display backend, not in
  the frontend: the agent already publishes whatever the backend returns.
- Linux has no system-wide wallpaper API, so the backend reads what the desktop environment writes:
  - **KDE Plasma**: `$XDG_CONFIG_HOME/plasma-org.kde.plasma.desktop-appletsrc`, group
    `[Containments][N][Wallpaper][org.kde.image][General]`, key `Image=…`. The plugin id is
    `org.kde.image` in Plasma 5 and 6 (`org.kde.plasma.image` is accepted as well). The slideshow
    plugin (`org.kde.slideshow`) stores the picture it used last, not the current wallpaper.
    **The value's format differs between the Plasma generations**: Plasma 5 writes a `file://` URI,
    Plasma 6 writes a plain absolute path (`Image=/usr/share/wallpapers/convergence.jxl`). A reader
    that only accepts the URI form finds nothing on a stock Plasma 6 desktop, so both forms are
    accepted (`image_path`).
  - **GNOME**: `gsettings get org.gnome.desktop.background picture-uri`, then `picture-uri-dark`.
    `gsettings` is not guaranteed to be installed (Fedora `glib2`, Debian `libglib2.0-bin`), so the
    read degrades to "no wallpaper" instead of failing.
- One picture covers every screen on both desktops, so the image is decoded once and each display gets
  its own centred crop (`resize_to_fill`) in its own aspect ratio. Plasma 6 can set a picture per
  screen; the port does not distinguish them.
- Decoding and encoding use the `image` crate (`png` + `jpeg`, no default features) as a Linux-only
  dependency of `oink-display`. The crate was already in the graph through `oink-agent` and the Tauri
  app, so `Cargo.lock` only gained the dependency edge and the licence inventory did not change.
- **Fedora and Bazzite ship their default wallpaper as JPEG XL** (`/usr/share/wallpapers/convergence.jxl`),
  and the `image` crate cannot decode that: the canvas stayed grey on a stock Plasma desktop until
  the port added `jxl-oxide` (pure Rust, Apache-2.0, Linux-only dependency of `oink-display`). Its
  `register_image_decoding_hook()` teaches the `image` crate the format *and* its file header, so
  the wallpaper reader needs no format knowledge of its own; `image` stays at 0.25.9 with the same
  features because jxl-oxide depends on it with `default-features = false`.
- `image::Limits` is `#[non_exhaustive]`: build it from `Limits::default()` and assign the fields.
- There is no "wallpaper changed" notification, so the change is detected by polling the mtime of the
  desktop's own configuration file (the Plasma appletsrc, the dconf database for GNOME) every two
  seconds.
- The wallpaper is published as a **JPEG data URL** (`data:image/jpeg;base64,…`), not raw base64.
  A check that looks for a leading `/9j` fails on the wire format.

**Why:** a grey canvas hides which machine a screen belongs to, which is what the user reported.
**How to apply:** keep the desktop-specific parsing in `crates/oink-display/src/linux/wallpaper.rs`
and verify with `OINK_LINUX_ARCH=arm64 tools/linux/check.sh {test,clippy} -p oink-display`.
See also [[linux-x11-display-backend]].
