#!/usr/bin/env python3
"""Build the Linux packages for one architecture.

The Linux package keeps the UI, the command line tool and the backend in one
directory: the UI starts the backend as its sibling, so the three binaries
must ship together. The desktop entry and the PATH entries point at that
directory, and the package manager owns the update path.

Two formats come out of the same staging tree: a Debian package for the
Debian family and an RPM package for the Red Hat family. They differ in
their dependency names, not in their contents.
"""
import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path, PurePosixPath

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from deb import build as build_deb_package
from rpm import build as build_rpm_package

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'build-support'))
from identity import IDENTITY as I
from identity import artifact_stem
from identity import decode_build
from payload import copy_notices
from provenance import digest
from report_manifest import write_manifest
from versioning import release_version


def package_release(version, build):
    """The version and the release slot the package managers compare.

    A rebuilt package has to sort above the package it replaces, so the
    release slot carries the sequence of the build. The build states the
    version it packages, so the label and the build cannot disagree.
    """
    base = version.split('-')[0]
    packaged, sequence = decode_build(build)
    if packaged != base:
        raise RuntimeError(f'build {build} packages {packaged}, not {base}')
    return base, str(sequence)

ROOT = Path(__file__).resolve().parents[2]
# The installed tree is a Linux filesystem tree: its paths are POSIX, whatever
# host stages it. The host paths are derived from them explicitly.
INSTALL_DIRECTORY = PurePosixPath('opt/oink')
DOC_DIRECTORY = PurePosixPath('usr/share/doc/oink')
APPLICATIONS_DIRECTORY = PurePosixPath('usr/share/applications')
BIN_DIRECTORY = PurePosixPath('usr/bin')
ICONS_DIRECTORY = PurePosixPath('usr/share/icons/hicolor')
UDEV_RULES_DIRECTORY = PurePosixPath('usr/lib/udev/rules.d')
# systemd runs the `uaccess` builtin from `73-seat-late.rules`: it grants the
# seat's user access to the devices that carry the `uaccess` tag. The tag has
# to be set before that file runs, so Oink's rules sort before it — a `99-`
# file would be too late and the access right would never be granted.
UDEV_RULES_NAME = '65-oink-input.rules'

def host_path(root, relative):
    """The host path of a package-relative POSIX path."""
    return root.joinpath(*relative.parts)


# The runtime dependencies of the Debian package. The C library carries a
# version: the binaries are linked against the library of the image they were
# built in, so the package states the version it needs. A package that
# installs on an older system and then fails to start is worse than one the
# package manager refuses.
GLIBC_MINIMUM = '2.39'


def debian_depends(glibc):
    """The runtime dependencies of the Debian package."""
    return (f'libc6 (>= {glibc}), libgtk-3-0, libwebkit2gtk-4.1-0, '
            'libayatana-appindicator3-1, libasound2t64 | libasound2')


def glibc_versions(output):
    """The C library versions a binary names, from the readelf output."""
    return set(re.findall(r'GLIBC_(\d+(?:\.\d+)*)', output))


def newest_glibc(versions):
    """The newest of the C library versions."""
    return max(versions, key=lambda value: tuple(int(part) for part in value.split('.')))


def glibc_minimum(binaries):
    """The oldest C library the packaged binaries run on.

    The binaries name the library versions they need, so the requirement
    follows the build instead of a number that has to be remembered.
    """
    versions = set()
    for role in ('ui', 'agent', 'backend'):
        binary = binaries / I['binaries'][role]
        try:
            output = subprocess.run(['readelf', '--version-info', str(binary)],
                                    check=True, capture_output=True, text=True).stdout
        except (OSError, subprocess.CalledProcessError):
            return GLIBC_MINIMUM
        versions.update(glibc_versions(output))
    if not versions:
        return GLIBC_MINIMUM
    return newest_glibc(versions)


# The RPM dependency names differ between the RPM distributions: openSUSE
# calls the WebKitGTK and the indicator library differently, and it names the
# ALSA library libasound2. RPM reads "a or b" as three separate package
# names, so every alternative needs its own parentheses to become a rich
# dependency. The tray icon library is loaded at runtime and the legacy
# indicator is accepted as a fallback, so either indicator package satisfies
# the dependency.

RPM_REQUIRES = ('(gtk3 or libgtk-3-0)',
                '(webkit2gtk4.1 or webkit2gtk-4.1 or libwebkit2gtk-4_1-0)',
                '(libayatana-appindicator-gtk3 or libayatana-appindicator or '
                'libayatana-appindicator3 or libayatana-appindicator3-1 or '
                'libappindicator-gtk3)',
                '(alsa-lib or libasound2)')
DEBIAN_ARCHITECTURES = {'x64': 'amd64', 'arm64': 'arm64'}
RPM_ARCHITECTURES = {'x64': 'x86_64', 'arm64': 'aarch64'}
SUMMARY = 'Share keyboard, mouse and clipboard between your computers'
DESCRIPTION = ('Oink (Oink Is Not only a KVM) shares the keyboard, mouse, trackpad and '
               'clipboard between your computers over a local network. Every device acts '
               'as both a host and a guest, so you can move your cursor and your typing to '
               'any nearby machine.')



def desktop_entry():
    return '\n'.join(['[Desktop Entry]',
                      'Type=Application',
                      'Name=Oink',
                      f'Comment={SUMMARY}',
                      'Exec=oink',
                      'Icon=oink',
                      'Terminal=false',
                      'Categories=Utility;RemoteAccess;',
                      'StartupNotify=true',
                      'StartupWMClass=com.hachiwii.oink',
                      'X-GNOME-UsesNotifications=true',
                      ''])

def udev_rules():
    """The udev rules the product installs.

    A gamepad's mouse node is a kernel artefact: the same controller also
    opens a node that reports `REL_X`/`REL_Y` and `BTN_LEFT`, so udev tags it
    as a mouse and the desktop drives the cursor with the stick. The injected
    pointer then fights the stick, and the cursor looks stuck where the stick
    drags it back. Clearing the mouse tag keeps the stick out of the pointer,
    the way SteamOS does it for its own controller.

    The first rule gives the seat's user access to the input devices. The
    desktop reads them through the file descriptors systemd hands it, so a
    plain process cannot: the nodes belong to the `input` group and the
    desktop's user is usually not in it. systemd's own `uaccess` rules cover
    other device classes, not input devices. Without the access right Oink
    cannot read the keyboard and cannot take the stick's pointer node away
    from the desktop.
    """
    return '\n'.join([
        '# 输入设备：截获要读它们。',
        '#',
        '# 桌面（合成器）是用 systemd 传的文件描述符读输入设备的，普通进程读不到：',
        '# 输入设备的节点归 `input` 组，而桌面的用户通常不在那个组里。',
        '# systemd 的 uaccess 规则覆盖了别的设备类，输入设备不在里面，',
        '# 所以这里给当前座位上的用户访问权。',
        'SUBSYSTEM=="input", KERNEL=="event*", TAG+="uaccess"',
        '',
        '# 手柄的鼠标节点不要当作指针。',
        '#',
        '# 内核把同一个手柄开出三个节点：摇杆（js0）、按键（event*）和一个鼠标节点。',
        '# 鼠标节点带 `REL_X`/`REL_Y` 和 `BTN_LEFT`，udev 把它标成 `ID_INPUT_MOUSE=1`，',
        '# 桌面（libinput）就把它当成鼠标：摇杆一动，光标就跟着动。',
        '# 共享指针注入的时候，两者会互相拉扯，光标像是被摇杆拖回原处。',
        '#',
        '# SteamOS 对自己的手柄也是这么处理的（`60-steam-input.rules` 清掉 `ID_INPUT_MOUSE`）。',
        'SUBSYSTEM=="input", ATTRS{name}=="*Controller*Mouse*", ENV{ID_INPUT_MOUSE}=""',
        'SUBSYSTEM=="input", ATTRS{name}=="*Gamepad*Mouse*", ENV{ID_INPUT_MOUSE}=""',
        'SUBSYSTEM=="input", ATTRS{name}=="*Joystick*Mouse*", ENV{ID_INPUT_MOUSE}=""',
        '',
    ])


def stage(root, binaries, notes_file, record):
    """Lay out the installed tree the two packages share."""
    directory = host_path(root, INSTALL_DIRECTORY)
    directory.mkdir(parents=True)
    for role in ('ui', 'agent', 'backend'):
        name = I['binaries'][role]
        source = binaries / name
        if not source.is_file():
            raise RuntimeError(f'missing packaged binary: {name}')
        target = directory / name
        shutil.copy2(source, target)
        target.chmod(0o755)
    # The desktop entry and a shell both reach the product through the PATH.
    # A symbolic link keeps the installation directory the only copy.
    for role in ('ui', 'agent'):
        name = I['binaries'][role]
        link = host_path(root, BIN_DIRECTORY) / name
        link.parent.mkdir(parents=True, exist_ok=True)
        link.symlink_to(f'/{INSTALL_DIRECTORY.as_posix()}/{name}')
    for size, icon in (('32x32', '32x32.png'), ('128x128', '128x128.png'), ('256x256', '128x128@2x.png')):
        target = host_path(root, ICONS_DIRECTORY) / size / 'apps'
        target.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / 'apps/oink/src-tauri/icons' / icon, target / 'oink.png')
    applications = host_path(root, APPLICATIONS_DIRECTORY)
    applications.mkdir(parents=True, exist_ok=True)
    (applications / 'com.hachiwii.oink.desktop').write_text(desktop_entry(), encoding='utf-8')
    rules = host_path(root, UDEV_RULES_DIRECTORY)
    rules.mkdir(parents=True)
    (rules / UDEV_RULES_NAME).write_text(udev_rules(), encoding='utf-8')
    documentation = host_path(root, DOC_DIRECTORY)
    documentation.mkdir(parents=True, exist_ok=True)
    copy_notices(documentation)
    if notes_file:
        shutil.copy2(notes_file, directory / 'oink-update-notes.json')
    (directory / I['metadata_file']).write_text(json.dumps(record, indent=2) + '\n')
    report = write_manifest(directory / 'report-manifest.json', record['source_tree_sha256'],
                            record['version'], record['build'],
                            [('agent', directory / I['binaries']['backend']),
                             ('agent', directory / I['binaries']['agent']),
                             ('ui_rust', directory / I['binaries']['ui'])])
    record['report_manifest'] = report


def debian_version(version, build):
    """A Debian version: the display version and the release as the revision."""
    base, release = package_release(version, build)
    return f'{base}-{release}'


def rpm_version(version, build):
    """RPM splits the version from the release."""
    base, release = package_release(version, build)
    return base, release


def debian_control(version, build, arch, glibc=GLIBC_MINIMUM):
    """The control file of the Debian package."""
    return '\n'.join([
        'Package: oink',
        f'Version: {debian_version(version, build)}',
        f'Architecture: {DEBIAN_ARCHITECTURES[arch]}',
        f'Maintainer: {I["publisher"]}',
        'Section: utils',
        'Priority: optional',
        'Homepage: https://github.com/oinkvm/oink',
        f'Depends: {debian_depends(glibc)}',
        f'Description: {SUMMARY}',
        ' ' + DESCRIPTION.replace('\n', '\n '),
        ''])


def debian_md5sums(root):
    """The checksum list of the packaged files, as the control archive wants it."""
    lines = []
    for path in sorted(root.rglob('*')):
        if path.is_symlink() or path.is_dir():
            continue
        checksum = hashlib.md5(path.read_bytes()).hexdigest()
        lines.append(f'{checksum}  {path.relative_to(root).as_posix()}')
    return '\n'.join(lines) + '\n'


def build_deb(root, output, version, build, arch, glibc=GLIBC_MINIMUM):
    """Write the Debian package for the staged tree."""
    name = output / f'{artifact_stem(version, "linux", arch)}.deb'
    return build_deb_package(root, name, debian_control(version, build, arch, glibc), debian_md5sums(root))


def build_rpm(root, output, version, build, arch):
    """Write the RPM package for the staged tree."""
    base, release = rpm_version(version, build)
    name = output / f'{artifact_stem(version, "linux", arch)}.rpm'
    return build_rpm_package(root, name, name='oink', version=base, release=release,
                             arch=RPM_ARCHITECTURES[arch], summary=SUMMARY,
                             description=DESCRIPTION, requires=RPM_REQUIRES,
                             license='GPL-3.0-only', url='https://github.com/oinkvm/oink')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binaries', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--arch', choices=['arm64', 'x64'], default='arm64')
    p.add_argument('--version', default=I['version'])
    p.add_argument('--update-notes-file', type=Path, help='Prepared embedded package notes')
    p.add_argument('--source-commit', help='Full source commit for a in-house build-labelled version')
    p.add_argument('--build', type=int, required=True)
    p.add_argument('--source-tree-sha256', required=True)
    p.add_argument('--tool-cache', type=Path, help='Unused: the system dpkg and rpm build the packages')
    a = p.parse_args()
    if a.version != I['version']:
        p.error('version must match packaging/identity.json')
    try:
        a.version = release_version(a.version, a.source_commit, a.build)
    except ValueError as error:
        p.error(str(error))
    a.output.mkdir(parents=True, exist_ok=True)
    record = dict(schema=1, product=I['name'], provenance='source-tree',
                  source_tree_sha256=a.source_tree_sha256, version=a.version,
                  source_commit=a.source_commit, build=a.build, platform='linux',
                  architecture=a.arch,
                  components=[I['binaries'][role] for role in ('ui', 'agent', 'backend')],
                  signing='unsigned')
    # The payload stays in the output directory: the release audit inspects
    # the installed tree that the packages were built from.
    payload = a.output / 'payload'
    if payload.exists():
        shutil.rmtree(payload)
    payload.mkdir(parents=True)
    stage(payload, a.binaries, a.update_notes_file, record)
    # Both packages are written from the same staging tree; neither writer
    # changes the tree, so the order only decides which file is listed first.
    rpm = build_rpm(payload, a.output, a.version, a.build, a.arch)
    deb = build_deb(payload, a.output, a.version, a.build, a.arch, glibc_minimum(a.binaries))
    record['artifacts'] = [{'name': f.name, 'size': f.stat().st_size, 'sha256': digest(f)} for f in (deb, rpm)]
    (a.output / 'release.json').write_text(json.dumps(record, indent=2) + '\n')
    (a.output / 'SHA256SUMS').write_text(''.join(f"{f['sha256']}  {f['name']}\n" for f in record['artifacts']))
    print(f'Package complete: {a.output}')


if __name__ == '__main__':
    main()
