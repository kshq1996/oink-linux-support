---
name: linux-tauri-ui-shape
description: How the Oink Tauri UI runs on Linux (tray, backend launch, attribution root, panel) and why the macOS/Windows gates differ
type: project
---

The Linux Tauri UI (`apps/oink/src-tauri`) runs the same shell as macOS/Windows with three deliberate differences:

- The **tray belongs to the UI process** on Linux (StatusNotifier/libayatana-appindicator), exactly like Windows; macOS keeps the status item in the backend. Tray creation failure on Linux is logged and the app continues with the main window (a desktop without a status area is normal); Windows stays fail-fast.
- The **backend is started by the UI** on Linux through `oink_agent::backend::launch::start_backend_tracked`, because the Linux package ships `oink`, `oink-agent` and `oink-backend` as three binaries in one directory and there is no login item or system service. The UI then waits for the IPC endpoint and follows the subscription like the Windows `start_once` path.
- The **attribution/notice files** are read from `/usr/share/doc/oink` on Linux (`commands::read_attribution`), because `packaging/linux/build.py` installs the notices with the documentation; macOS reads them from the bundle `Resources` and Windows from the resource dir.

Other Linux-specific shapes: the main window keeps native GTK decorations (only macOS uses an overlay title bar and only Windows removes the frame to draw its own caption buttons), the quick panel is positioned/sized with `panel_bounds` and shown like the Windows panel, and `native_menu`/`live_views` are compiled on Linux because the frontend calls `shell_set_language` unconditionally and `show_normal` refreshes the live views.

**Why:** the Linux desktop is a plain window plus a tray icon, so every platform-specific native service (dock, activation policy, local package drop, elevated installer) stays out of the Linux build.

**How to apply:** when adding a UI command on Linux, check that the frontend calls it unconditionally (otherwise register it on Linux too) and that the macOS-only helper it uses has a non-macOS stub, as `native_menu::install` does. See [[linux-clipboard-grant-identity]].
