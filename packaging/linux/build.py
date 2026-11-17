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
import shutil
import sys
from pathlib import Path, PurePosixPath

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from deb import build as build_deb_package
from rpm import build as build_rpm_package

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'build-support'))
from identity import IDENTITY as I
from identity import artifact_stem
from payload import copy_notices
from provenance import digest
from report_manifest import write_manifest

ROOT = Path(__file__).resolve().parents[2]
# The installed tree is a Linux filesystem tree: its paths are POSIX, whatever
# host stages it. The host paths are derived from them explicitly.
INSTALL_DIRECTORY = PurePosixPath('opt/oink')
DOC_DIRECTORY = PurePosixPath('usr/share/doc/oink')
APPLICATIONS_DIRECTORY = PurePosixPath('usr/share/applications')
BIN_DIRECTORY = PurePosixPath('usr/bin')
ICONS_DIRECTORY = PurePosixPath('usr/share/icons/hicolor')


def host_path(root, relative):
    """The host path of a package-relative POSIX path."""
    return root.joinpath(*relative.parts)


# The dependency names differ between the two distribution families, and the
# RPM names differ between Fedora/RHEL and openSUSE as well.
DEBIAN_DEPENDS = ('libc6, libgtk-3-0, libwebkit2gtk-4.1-0, libayatana-appindicator3-1, '
                   'libasound2t64 | libasound2')
# RPM reads "a or b" as three separate package names, so the alternative
# needs its own parentheses to become a rich dependency.
# The tray icon library is loaded at runtime and the legacy indicator is
# accepted as a fallback, so either package satisfies the dependency.
RPM_REQUIRES = ('(gtk3 or libgtk-3-0)',
                '(webkit2gtk4.1 or webkit2gtk-4.1)',
                '(libayatana-appindicator or libayatana-appindicator3 '
                'or libappindicator-gtk3)',
                'alsa-lib')
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


def debian_version(version):
    """A Debian version keeps the display version; the suffix is the revision."""
    if version.count('-') > 1:
        raise RuntimeError(f'unsupported package version: {version}')
    return version


def rpm_version(version):
    """RPM splits the version from the release and rejects a hyphen in the version."""
    base, _, suffix = version.partition('-')
    return base, suffix or '1'


def debian_control(version, arch):
    """The control file of the Debian package."""
    return '\n'.join([
        'Package: oink',
        f'Version: {debian_version(version)}',
        f'Architecture: {DEBIAN_ARCHITECTURES[arch]}',
        f'Maintainer: {I["publisher"]}',
        'Section: utils',
        'Priority: optional',
        'Homepage: https://github.com/oinkvm/oink',
        f'Depends: {DEBIAN_DEPENDS}',
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


def build_deb(root, output, version, arch):
    """Write the Debian package for the staged tree."""
    name = output / f'{artifact_stem(version, "linux", arch)}.deb'
    return build_deb_package(root, name, debian_control(version, arch), debian_md5sums(root))


def build_rpm(root, output, version, arch):
    """Write the RPM package for the staged tree."""
    base, release = rpm_version(version)
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
    rpm = build_rpm(payload, a.output, a.version, a.arch)
    deb = build_deb(payload, a.output, a.version, a.arch)
    record['artifacts'] = [{'name': f.name, 'size': f.stat().st_size, 'sha256': digest(f)} for f in (deb, rpm)]
    (a.output / 'release.json').write_text(json.dumps(record, indent=2) + '\n')
    (a.output / 'SHA256SUMS').write_text(''.join(f"{f['sha256']}  {f['name']}\n" for f in record['artifacts']))
    print(f'Package complete: {a.output}')


if __name__ == '__main__':
    main()
