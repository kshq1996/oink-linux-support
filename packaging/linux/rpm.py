#!/usr/bin/env python3
"""Write a binary RPM from a staged file tree.

rpmbuild owns the RPM container format; this module owns the package's
contents. The spec is generated from the staged tree so the file list and the
payload can never disagree.
"""
import os
from pathlib import Path
import subprocess
import tempfile


# The base filesystem package owns the directories every distribution shares,
# and the icon theme package owns the hicolor icon directories, so a package
# that claims them cannot be installed next to them. Only the directories the
# product owns itself are packaged as directories; every other directory is
# created by the distribution.
OWNED_ROOTS = ('/opt/oink', '/usr/share/doc/oink')


def _owned_directory(name):
    """Whether the package owns this directory of the staged tree."""
    return any(name == root or name.startswith(root + '/') for root in OWNED_ROOTS)


def _files(root):
    """The %files list: every packaged path, owned directories marked as such."""
    lines = []
    for path in sorted(root.rglob('*')):
        name = '/' + path.relative_to(root).as_posix()
        if path.is_dir():
            if _owned_directory(name):
                lines.append(f'%dir {name}')
        else:
            lines.append(name)
    return lines


def spec(root, name, version, release, arch, summary, description, requires, license, url):
    """The RPM spec for a staged tree.

    The architecture is the build target rpmbuild was given, so the spec
    states it as documentation and never constrains the build host.
    """
    requires = '\n'.join(f'Requires: {item}' for item in requires)
    return f"""# Package architecture: {arch}
Name: {name}
Version: {version}
Release: {release}
Summary: {summary}
License: {license}
URL: {url}
{requires}
%description
{description}
%prep

%install
mkdir -p %{{buildroot}}
cp -a %{{oink_staging}}/. %{{buildroot}}/

%files
%defattr(-,root,root,-)
{chr(10).join(_files(root))}

%changelog
"""


def build(root, output, **fields):
    """Build the RPM for a staged tree and return its path."""
    root = Path(root)
    with tempfile.TemporaryDirectory(prefix='oink-rpm-') as temporary:
        topdir = Path(temporary) / 'rpm'
        (topdir / 'SPECS').mkdir(parents=True)
        (topdir / 'BUILD').mkdir()
        (topdir / 'RPMS').mkdir()
        (topdir / 'SOURCES').mkdir()
        (topdir / 'SRPMS').mkdir()
        path = topdir / 'SPECS' / f"{fields['name']}.spec"
        path.write_text(spec(root, **fields))
        environment = dict(os.environ, SOURCE_DATE_EPOCH='0')
        subprocess.run([
            'rpmbuild', '-bb', '--target', fields['arch'],
            '--define', f'_topdir {topdir}',
            '--define', f'oink_staging {root}', '--define', f'_build_id_links none',
            str(path),
        ], check=True, env=environment, capture_output=True)
        built = list((topdir / 'RPMS').rglob('*.rpm'))
        if len(built) != 1:
            raise RuntimeError('rpmbuild did not produce exactly one package')
        Path(output).write_bytes(built[0].read_bytes())
    return Path(output)
