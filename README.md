<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/assets/banner-dark.png">
    <img alt="Oink — Oink Is Not only a KVM. Your computers, working as one." src=".github/assets/banner-light.png" width="860">
  </picture>
</p>

# Oink — Oink Is Not only a KVM

**English** | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | [한국어](README.ko.md)

Oink, short for "Oink Is Not only a KVM", is modern software that builds a peer-to-peer sharing network across multiple Windows, Mac and Linux computers. Use the keyboard, mouse and trackpad of any device to control any nearby device, with the same native feel everywhere; play the audio of several devices through the speakers or headphones connected to one of them; share the clipboard and drag and drop between devices; discover devices on the local network automatically; all traffic is encrypted by default.

## Install

Get installers and matching source from [GitHub Releases](https://github.com/kshq1996/oink-linux-support/releases).

Supported operating systems and versions:

- **Mac:** Apple silicon (arm64), macOS 14.2+; Intel (x64), macOS 14.2+ (untested)
- **Windows:** x64, Windows 10 version 1809 (build 17763) or later, or Windows 11. Windows 10 versions before 22H2 are untested. Windows 10 LTSC (such as LTSC 2019 / LTSC 2021) does not include the WebView2 Runtime; install the Microsoft Edge WebView2 Runtime (Evergreen) first: https://developer.microsoft.com/microsoft-edge/webview2/
- **Linux:** x64 and arm64, in an X11 or Wayland desktop session. The packages are built on Ubuntu 24.04 and need glibc 2.39 or newer. Install the `.deb` package on Debian 13 or newer, Ubuntu 24.04 or newer and Linux Mint 22 or newer, or the `.rpm` package on Fedora 40 or newer and openSUSE Tumbleweed.

If SmartScreen says "Windows protected your PC", use **More info → Run anyway** only if offered. UAC may show an unknown publisher.

### Linux

The Linux packages install three programs: the tray application `oink`, the command line tool `oink-agent` and the background process `oink-backend`. The package declares its runtime dependencies (WebKitGTK 4.1, GTK 3, libayatana-appindicator3) and the version of the C library the binaries need, so the distribution package manager installs whatever is missing and refuses a system whose C library is too old.

The package is unsigned. `dnf install` and `apt install` take an unsigned local package as it is, but zypper asks what to do with one and stops at the default answer, so install it with:

```
sudo zypper --no-gpg-checks install ./Oink-<version>-linux-<arch>.rpm
```

The Linux backends use X11 for the clipboard (the X11 selection protocol) and the display layout (XRandR). Input capture and injection depend on the session: an X11 session uses XRecord and XTest, a Wayland session reads the kernel's input devices (`/dev/input`) for the capture and writes a virtual pointer device (`/dev/uinput`) for the injection, because XWayland cannot move the compositor's cursor. The capture reads the mouse's raw device counts, so the shared pointer's speed follows the mouse's DPI and the compositor's pointer acceleration; switching the pointer acceleration off in the desktop settings makes the speed exact. The cursor trail overlay that macOS and Windows draw is not available on Linux.

System audio is not available on Linux yet: playing another device's audio on this device and sharing the microphone need a native audio backend, which Linux does not have yet. Everything else — keyboard, mouse, clipboard, display layout and device discovery — works. The trackpad is captured in an X11 session; a Wayland session does not read the touchpad's gestures yet.

On a handheld, a gamepad's stick reaches the desktop as a mouse: the kernel opens a node for the controller that reports relative motion, and udev tags it as a mouse. The stick then drags the injected pointer around, and the cursor looks stuck. Oink leaves the gamepad's nodes out of the capture, takes them away from the desktop while the other device holds the pointer, and the packages install a udev rule that keeps the controller's mouse node out of the pointer for good.

The layout view draws the desktop wallpaper as the background of each screen, read from the KDE Plasma or GNOME settings.

On an immutable Fedora system, such as BazziteOS, Universal Blue or a Fedora
Silverblue image, the root filesystem cannot be changed, so `dnf install` of a
local `.rpm` fails. Layer the package instead and reboot into it:

```
sudo rpm-ostree install ./Oink-<version>-linux-<arch>.rpm
```

`rpm-ostree` does not check the signature of a local package, so the
unsigned file installs as it is, and the change takes effect after the
reboot. A package manager that insists on a signature needs
`--nosignature` (`rpm -i --nosignature ...`).

### Sponsor

If this project helps you, consider sponsoring some of my AI subscription costs. Those tokens help make Oink better.

<a href="https://ko-fi.com/hachiwii"><img alt="Ko-fi" src="https://ko-fi.com/img/githubbutton_sm.svg" height="36"></a>

## Contributing

This project does not accept pull requests without context, because there is no one to review the code. Please use [Issues](https://github.com/oinkvm/oink/issues) for all bug reports and feature requests. When you report a problem, please include your device environment, diagnostic data, steps to reproduce and your AI-assisted investigation's conclusions. Using Oink in real environments and sending feedback is the biggest contribution to this project.

## FAQ

**Is Oink based on, or compatible with, Deskflow?**\
No. Oink is a project started completely from scratch. It uses its own peer-to-peer (not client–server) protocol and is not compatible with other software KVMs.

**Do I have to designate one device as the controller?**\
No. Oink uses a peer-to-peer design: the keyboard and mouse connected to any device can control the other devices.

**Can I share trackpad gestures?**\
Yes, but trackpads on Windows have not been tested.

**Does it support clipboard sharing and drag and drop across operating systems?**\
Yes. The clipboard is synced across devices automatically, and you can drag text, images and files directly from one device to another — even straight into apps such as Chrome.

**The pointer moves faster on my small screen than on my large one — how do I make them feel the same?**\
The layout gives every screen its physical size, so the same hand movement covers a larger share of a small screen than of a large one. Set the remote pointer speed of the smaller screen in the layout editor to the ratio of the two screens' widths: a 15-inch handheld screen needs about 0.43× the speed of a 27-inch screen. The remote pointer speed only applies while another device controls that screen, so the mouse and trackpad of the small screen keep their own speed.

**Do I need to buy a license for personal or commercial use?**\
No. Oink is free software released under GPL-3.0-only with additional terms, so you can use it for any personal or commercial purpose without asking for any further license. If you distribute Oink or a modified version of it, you must provide the complete source code under the same license, keep the original author attribution and license notices, and mark your changes; under the additional terms you also may not use the Oink name to promote derived products. Modifying Oink only for your own use, without distributing it, carries none of these obligations.

## License

This project is licensed under [GPL-3.0-only](LICENSE) with [additional terms](LICENSE-ADDITIONAL-TERMS.md).
