---
name: linux-rpm-packaging
description: How the Oink Linux RPM and DEB name their dependencies per distribution, and the three traps that make a package install but not run
type: project
---

Facts about the Oink Linux packages (2026-11-17):

- **RPM reads an unparenthesised alternative as separate package names.**
  `Requires: gtk3 or libgtk-3-0` asks for three packages, one of them called
  `or`. Every alternative needs its own parentheses, and the built package has
  to be checked with `rpm -qp --requires` (the packaging test does that).
- **The package names differ per distribution** (verified by installing the
  packages in the real images):
  - Fedora: `gtk3`, `webkit2gtk4.1`, `libayatana-appindicator-gtk3` (the
    un-suffixed `libayatana-appindicator` was the name up to Fedora 40),
    `alsa-lib`.
  - openSUSE: `libgtk-3-0`, `libwebkit2gtk-4_1-0`, `libayatana-appindicator3-1`,
    `libasound2` (openSUSE still *provides* `alsa-lib`, so that name works
    there too).
  - RHEL: `gtk3`, `webkit2gtk4.1`, `libappindicator-gtk3`, `alsa-lib`.
  A requirement that names only one distribution's spelling leaves the others
  without the library: the package installs and the program cannot start.
- **The tray icon library is dlopen'd with a fallback.**
  `libappindicator-sys` loads `libayatana-appindicator3.so.1` and falls back to
  the legacy `libappindicator3.so.1`, so the package may depend on either
  indicator package.
- **A package must not own the shared directories.** The base filesystem
  package owns `/usr`, `/opt` and `/usr/share`, the icon theme owns the hicolor
  directories; a package that lists them as `%dir` cannot be installed next to
  them. The product owns `/opt/oink` and `/usr/share/doc/oink` only.
- **Fedora's `/etc/dnf/dnf.conf` sets `tsflags=nodocs`**, so a dnf install
  leaves the documentation out on purpose. The licence files are in the
  package (`rpm -ql` shows them); a missing `/usr/share/doc/oink` after a dnf
  install is not a packaging defect.
- **The binaries are linked against the C library of the build image**
  (Ubuntu 24.04, glibc 2.39). A system with an older C library installs the
  package and then fails to start the program, so the Debian package states
  `libc6 (>= 2.39)` and the RPM carries the same through its symbol versions.
  The version is read from the binaries with `readelf --version-info`
  (`packaging/linux/build.py` `glibc_minimum`), not remembered.

**Why:** the user reported that the `.rpm` could not be installed at all; the
first fix (parentheses, owned directories) was not enough, because the
dependency names were also wrong for openSUSE and Fedora 41, and a package
that installs but cannot start is the same failure in a quieter form.
**How to apply:** verify a packaging change by installing the built package in
the real distribution images (`docker run --platform linux/arm64 fedora:41`,
`opensuse/tumbleweed`, `ubuntu:24.04`, `debian:13`), not only through the
unit tests; check that the program starts, not only that the package manager
is happy. See also [[linux-test-suite]] and [[linux-release-platform-set]].
