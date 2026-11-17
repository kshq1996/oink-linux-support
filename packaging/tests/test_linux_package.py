import gzip
import importlib.util
import io
import json
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'packaging/linux'), str(ROOT / 'build-support')]
DEB_SPEC = importlib.util.spec_from_file_location('linux_deb', ROOT / 'packaging/linux/deb.py')
DEB = importlib.util.module_from_spec(DEB_SPEC)
DEB_SPEC.loader.exec_module(DEB)
RPM_SPEC = importlib.util.spec_from_file_location('linux_rpm', ROOT / 'packaging/linux/rpm.py')
RPM = importlib.util.module_from_spec(RPM_SPEC)
RPM_SPEC.loader.exec_module(RPM)
SPEC = importlib.util.spec_from_file_location('linux_build', ROOT / 'packaging/linux/build.py')
B = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(B)

def ar_members(data):
    """The members of an ar archive: name and payload."""
    assert data.startswith(b'!<arch>\n'), 'not an ar archive'
    members = {}
    offset = 8
    while offset < len(data):
        header = data[offset:offset + 60]
        name = header[0:16].decode().strip()
        size = int(header[48:58].decode().strip())
        members[name] = data[offset + 60:offset + 60 + size]
        offset += 60 + size + (size % 2)
    return members


def tar_entries(data):
    """The member names of a gzipped tar archive."""
    with tarfile.open(fileobj=io.BytesIO(gzip.decompress(data)), mode='r:') as archive:
        return archive.getnames()


def elf_image(identifier=None):
    """A minimal x86_64 ELF image with a build id, as the report manifest needs."""
    identifier = identifier or bytes(range(16))
    names = b'\0.shstrtab\0.note.gnu.build-id\0'
    note = struct.pack('<III', 4, len(identifier), 3) + b'GNU\0' + identifier
    header_size, program_size, section_size = 64, 56, 64
    note_at = header_size + program_size
    names_at = note_at + len(note)
    sections_at = names_at + len(names)
    sections_at += (-sections_at) % 8
    total = sections_at + 3 * section_size
    data = bytearray(total)
    data[0:16] = b'\x7fELF\x02\x01\x01\x00' + bytes(8)
    struct.pack_into('<HHIQQQIHHHHHH', data, 16, 2, 0x3e, 1, 0x1000, header_size,
                     sections_at, 0, header_size, program_size, 1, section_size, 3, 1)
    # The executable segment covers the whole image, so the text range is known.
    struct.pack_into('<IIQQQQQQ', data, header_size, 1, 5, 0, 0x1000, 0x1000, total, total, 0x1000)
    data[note_at:note_at + len(note)] = note
    data[names_at:names_at + len(names)] = names
    for index, (name, kind, offset, size) in enumerate([
            (0, 0, 0, 0), (1, 3, names_at, len(names)), (11, 7, note_at, len(note))]):
        at = sections_at + index * section_size
        struct.pack_into('<IIQQQQIIQQ', data, at, name, kind, 0, 0, offset, size, 0, 0, 1, 0)
    return bytes(data)


class LinuxStaging(unittest.TestCase):
    def stage(self, root):
        """A staged tree with the three binaries the product needs."""
        binaries = root / 'binaries'
        binaries.mkdir()
        for index, role in enumerate(('ui', 'agent', 'backend')):
            path = binaries / B.I['binaries'][role]
            path.write_bytes(elf_image(bytes([index + 1]) * 16))
            path.chmod(0o755)
        record = {'schema': 1, 'product': B.I['name'], 'provenance': 'source-tree',
                  'source_tree_sha256': '0' * 64, 'version': B.I['version'],
                  'source_commit': None, 'build': 1, 'platform': 'linux',
                  'architecture': 'x64',
                  'components': [B.I['binaries'][role] for role in ('ui', 'agent', 'backend')],
                  'signing': 'unsigned'}
        payload = root / 'payload'
        payload.mkdir()
        B.stage(payload, binaries, None, record)
        return payload, record

    def test_tree_carries_the_product_entry_points_and_notices(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload, record = self.stage(root)
            for role in ('ui', 'agent', 'backend'):
                name = B.I['binaries'][role]
                self.assertTrue((B.host_path(payload, B.INSTALL_DIRECTORY) / name).is_file(), name)
            # The desktop entry and the PATH entries reach the same binaries.
            entry = B.host_path(payload, B.APPLICATIONS_DIRECTORY) / 'com.hachiwii.oink.desktop'
            self.assertIn('Exec=oink', entry.read_text())
            for role in ('ui', 'agent'):
                name = B.I['binaries'][role]
                link = B.host_path(payload, B.BIN_DIRECTORY) / name
                self.assertTrue(link.is_symlink(), name)
                self.assertEqual(DEB.link_target(link), f'/{B.INSTALL_DIRECTORY}/{name}')
            self.assertTrue((B.host_path(payload, B.DOC_DIRECTORY) / 'LICENSE').is_file())
            self.assertTrue((B.host_path(payload, B.INSTALL_DIRECTORY) / 'report-manifest.json').is_file())
            self.assertIn('report_manifest', record)

    def test_debian_package_carries_the_control_file_and_the_tree(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload, _ = self.stage(root)
            name = B.build_deb(payload, root, B.I['version'], 'x64')
            members = ar_members(name.read_bytes())
            self.assertEqual(set(members), {'debian-binary', 'control.tar.gz', 'data.tar.gz'})
            self.assertEqual(members['debian-binary'], b'2.0\n')
            control = tar_entries(members['control.tar.gz'])
            self.assertIn('./control', control)
            self.assertIn('./md5sums', control)
            with tarfile.open(fileobj=io.BytesIO(gzip.decompress(members['control.tar.gz'])), mode='r:') as archive:
                text = archive.extractfile('./control').read().decode()
                sums = archive.extractfile('./md5sums').read().decode()
            self.assertIn('Package: oink', text)
            self.assertIn('Architecture: amd64', text)
            self.assertIn(f'Version: {B.I["version"]}', text)
            self.assertIn('Depends: ', text)
            self.assertIn('libwebkit2gtk-4.1-0', text)
            self.assertIn(f'{B.INSTALL_DIRECTORY}/{B.I["binaries"]["ui"]}', sums)
            data = tar_entries(members['data.tar.gz'])
            self.assertIn(f'./{B.INSTALL_DIRECTORY}/{B.I["binaries"]["backend"]}', data)
            self.assertIn(f'./{B.APPLICATIONS_DIRECTORY}/com.hachiwii.oink.desktop', data)

    def test_debian_package_keeps_directories_and_symlinks(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload, _ = self.stage(root)
            name = B.build_deb(payload, root, B.I['version'], 'x64')
            members = ar_members(name.read_bytes())
            with tarfile.open(fileobj=io.BytesIO(gzip.decompress(members['data.tar.gz'])), mode='r:') as archive:
                kinds = {member.name: member for member in archive.getmembers()}
            self.assertTrue(kinds[f'./{B.INSTALL_DIRECTORY}'].isdir())
            self.assertTrue(kinds[f'./{B.INSTALL_DIRECTORY}/{B.I["binaries"]["ui"]}'].isfile())
            link = kinds[f'./{B.BIN_DIRECTORY}/{B.I["binaries"]["ui"]}']
            self.assertTrue(link.issym())
            self.assertEqual(link.linkname, f'/{B.INSTALL_DIRECTORY}/{B.I["binaries"]["ui"]}')

    def test_the_desktop_entry_declares_one_main_category(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload, _ = self.stage(root)
            entry = (B.host_path(payload, B.APPLICATIONS_DIRECTORY) / 'com.hachiwii.oink.desktop').read_text()
            self.assertIn('Exec=oink', entry)
            categories = next(line for line in entry.splitlines() if line.startswith('Categories='))
            main = {'AudioVideo', 'Audio', 'Video', 'Development', 'Education', 'Game', 'Graphics',
                    'Network', 'Office', 'Science', 'Settings', 'System', 'Utility'}
            values = [value for value in categories.split('=', 1)[1].split(';') if value]
            self.assertEqual(len([value for value in values if value in main]), 1)

    def test_rpm_package_is_built_from_the_same_tree(self):
        if shutil.which('rpmbuild') is None:
            self.skipTest('rpmbuild is not installed')
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload, _ = self.stage(root)
            name = B.build_rpm(payload, root, B.I['version'], 'x64')
            self.assertTrue(name.is_file())
            self.assertGreater(name.stat().st_size, 0)

    def test_rpm_dependencies_are_rich_dependencies(self):
        """Every alternative needs its parentheses.

        RPM reads "gtk3 or libgtk-3-0" as three package names, so the
        package would ask for a package called "or".
        """
        for requirement in B.RPM_REQUIRES:
            if ' or ' in requirement:
                self.assertTrue(requirement.startswith('(') and requirement.endswith(')'), requirement)

    def test_rpm_does_not_own_the_shared_directories(self):
        """The package must not claim a directory another package owns.

        The base filesystem package owns /usr and /opt, and the icon theme
        package owns the hicolor icon directories, so a package that owns
        them cannot be installed next to them.
        """
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            payload, _ = self.stage(root)
            owned = [name for name in RPM._files(payload) if name.startswith('%dir ')]
            for name in owned:
                path = name[len('%dir '):]
                self.assertTrue(path.startswith('/opt/oink') or path.startswith('/usr/share/doc/oink'), path)
            # The shared directories are present in the tree but stay unowned.
            for shared in ('/usr', '/usr/bin', '/usr/share', '/usr/share/applications',
                           '/usr/share/icons/hicolor/32x32/apps', '/opt'):
                self.assertNotIn(f'%dir {shared}', owned, shared)


class LinuxVersions(unittest.TestCase):
    def test_debian_version_keeps_the_display_version(self):
        self.assertEqual(B.debian_version('0.3.6'), '0.3.6')
        self.assertEqual(B.debian_version('0.3.6-1'), '0.3.6-1')
        with self.assertRaisesRegex(RuntimeError, 'unsupported package version'):
            B.debian_version('0.3.6-1-2')

    def test_rpm_version_splits_the_release(self):
        self.assertEqual(B.rpm_version('0.3.6'), ('0.3.6', '1'))
        self.assertEqual(B.rpm_version('0.3.6-3'), ('0.3.6', '3'))

    def test_architectures_match_the_package_managers(self):
        self.assertEqual(B.DEBIAN_ARCHITECTURES, {'x64': 'amd64', 'arm64': 'arm64'})
        self.assertEqual(B.RPM_ARCHITECTURES, {'x64': 'x86_64', 'arm64': 'aarch64'})


class LinuxPackaging(unittest.TestCase):
    def test_package_record_lists_the_artifacts(self):
        if shutil.which('rpmbuild') is None:
            self.skipTest('rpmbuild is not installed')
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binaries = root / 'binaries'
            binaries.mkdir()
            for index, role in enumerate(('ui', 'agent', 'backend')):
                path = binaries / B.I['binaries'][role]
                path.write_bytes(elf_image(bytes([index + 1]) * 16))
                path.chmod(0o755)
            output = root / 'output'
            command = [sys.executable, str(ROOT / 'packaging/linux/build.py'),
                       '--binaries', str(binaries), '--output', str(output), '--arch', 'x64',
                       '--build', '1', '--source-tree-sha256', '0' * 64]
            subprocess.run(command, check=True, capture_output=True)
            record = json.loads((output / 'release.json').read_text())
            self.assertEqual(record['platform'], 'linux')
            self.assertEqual(record['architecture'], 'x64')
            names = {artifact['name'] for artifact in record['artifacts']}
            self.assertEqual(len(names), 2)
            self.assertTrue(any(name.endswith('.deb') for name in names))
            self.assertTrue(any(name.endswith('.rpm') for name in names))
            sums = (output / 'SHA256SUMS').read_text()
            for artifact in record['artifacts']:
                self.assertIn(f"{artifact['sha256']}  {artifact['name']}", sums)


if __name__ == '__main__':
    unittest.main()
