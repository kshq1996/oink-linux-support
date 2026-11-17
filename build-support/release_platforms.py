#!/usr/bin/env python3
"""The release platform set: the matrix a run builds and the records it expects.

One list names the platforms a release publishes. The guard job turns it into
the build matrix, and the draft job requires exactly those platform records,
so a platform that is configured can never be missing from a published
release and a platform that is not configured is never published.
"""
import argparse
import json
import os

# The platforms a release can publish. The macOS entries need the upstream
# signing identity, so a fork publishes the Linux packages until it has one.
PLATFORMS = {
    'macos': {'os': 'macos-15', 'platform': 'macos', 'arch': 'arm64',
              'target': 'aarch64-apple-darwin', 'jobs': '2'},
    'macos-x64': {'os': 'macos-15-intel', 'platform': 'macos', 'arch': 'x64',
                  'target': 'x86_64-apple-darwin', 'jobs': '2'},
    'windows': {'os': 'windows-2022', 'platform': 'windows',
                'target': 'x86_64-pc-windows-msvc', 'jobs': '4'},
    'linux-x64': {'os': 'ubuntu-24.04', 'platform': 'linux', 'release': 'linux-x64',
                  'arch': 'x64', 'target': 'x86_64-unknown-linux-gnu', 'jobs': '4'},
    'linux-arm64': {'os': 'ubuntu-24.04-arm', 'platform': 'linux', 'release': 'linux-arm64',
                    'arch': 'arm64', 'target': 'aarch64-unknown-linux-gnu', 'jobs': '4'},
}

DEFAULT = ['linux-x64', 'linux-arm64']


def parse(value):
    """The platform names a release publishes, from a comma-separated list."""
    names = [name.strip() for name in (value or '').split(',') if name.strip()]
    if not names:
        names = list(DEFAULT)
    unknown = [name for name in names if name not in PLATFORMS]
    if unknown:
        raise ValueError('unknown release platform: ' + ', '.join(unknown))
    if len(set(names)) != len(names):
        raise ValueError('duplicate release platform: ' + ', '.join(names))
    return names


def matrix(names):
    """The build matrix for the platform names."""
    return {'include': [dict(PLATFORMS[name]) for name in names]}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('operation', choices=['list', 'matrix'])
    p.add_argument('--platforms', default=os.environ.get('OINK_RELEASE_PLATFORMS', ''),
                   help='Comma-separated platform names; the default publishes the Linux packages.')
    a = p.parse_args()
    names = parse(a.platforms)
    if a.operation == 'list':
        print(','.join(names))
    else:
        print(json.dumps(matrix(names), separators=(',', ':')))


if __name__ == '__main__':
    main()
