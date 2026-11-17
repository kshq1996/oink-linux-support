#!/usr/bin/env python3
"""Write a binary Debian package from a staged file tree.

The archive is produced directly, without dpkg-deb: the package holds a fixed,
reviewable layout and the writer must be reproducible, so every member carries
an explicit mode, owner and timestamp instead of whatever the build host used.
"""
import gzip
import io
import tarfile
from pathlib import Path

CONTROL = 'control'
MD5SUMS = 'md5sums'
DATA = 'data.tar.gz'


def _ar_member(name, data):
    """One ar member: a fixed header, the payload, and even-byte padding."""
    header = f'{name:<16}{0:<12}{0:<6}{0:<6}{0o100644:<8}{len(data):<10}`\n'.encode()
    padding = b'\n' if len(data) % 2 else b''
    return header + data + padding


def _tar_bytes(entries):
    """A gzip-compressed tar archive of (name, mode, kind, payload) entries."""
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode='w', format=tarfile.GNU_FORMAT) as archive:
        for name, mode, kind, payload in entries:
            info = tarfile.TarInfo(name)
            info.type = kind
            info.mode = mode
            info.size = len(payload) if kind == tarfile.REGTYPE else 0
            info.mtime = 0
            info.uid = 0
            info.gid = 0
            info.uname = 'root'
            info.gname = 'root'
            info.linkname = payload.decode() if kind == tarfile.SYMTYPE else ''
            archive.addfile(info, io.BytesIO(payload) if kind == tarfile.REGTYPE else None)
    return gzip.compress(raw.getvalue(), compresslevel=9, mtime=0)


def _tree_entries(root):
    """The staged tree as tar members, directories first and paths relative."""
    entries = []
    for path in sorted(root.rglob('*')):
        name = './' + path.relative_to(root).as_posix()
        if path.is_symlink():
            entries.append((name, 0o120777, tarfile.SYMTYPE, str(path.readlink()).encode()))
        elif path.is_dir():
            entries.append((name + '/', 0o755, tarfile.DIRTYPE, b''))
        else:
            entries.append((name, 0o755 if path.stat().st_mode & 0o111 else 0o644, tarfile.REGTYPE, path.read_bytes()))
    return entries




def build(root, output, control, md5sums):
    """Write the .deb for a staged tree at `root` to `output`."""
    data = _tar_bytes(_tree_entries(root))
    control_entries = [('./' + CONTROL, 0o644, tarfile.REGTYPE, control.encode())]
    if md5sums is not None:
        control_entries.append(('./' + MD5SUMS, 0o644, tarfile.REGTYPE, md5sums.encode()))
    archive = b'!<arch>\n'
    archive += _ar_member('debian-binary', b'2.0\n')
    archive += _ar_member('control.tar.gz', _tar_bytes(control_entries))
    archive += _ar_member(DATA, data)
    Path(output).write_bytes(archive)
    return Path(output)
