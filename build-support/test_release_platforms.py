import unittest

from release_platforms import DEFAULT, PLATFORMS, matrix, parse


class ReleasePlatformTests(unittest.TestCase):
    def test_the_default_publishes_both_linux_architectures(self):
        self.assertEqual(parse(''), ['linux-x64', 'linux-arm64'])
        self.assertEqual(parse(None), DEFAULT)
        self.assertEqual(parse('  '), DEFAULT)

    def test_a_configured_list_selects_exactly_those_platforms(self):
        self.assertEqual(parse('windows,linux-x64'), ['windows', 'linux-x64'])
        self.assertEqual(parse('macos'), ['macos'])

    def test_unknown_and_duplicate_platforms_are_rejected(self):
        with self.assertRaises(ValueError):
            parse('linux-riscv')
        with self.assertRaises(ValueError):
            parse('linux-x64,linux-x64')

    def test_the_matrix_carries_the_runner_and_target_of_each_platform(self):
        entries = matrix(['linux-x64', 'linux-arm64'])['include']
        self.assertEqual([entry['os'] for entry in entries], ['ubuntu-24.04', 'ubuntu-24.04-arm'])
        self.assertEqual([entry['target'] for entry in entries],
                         ['x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu'])
        self.assertEqual([entry['platform'] for entry in entries], ['linux', 'linux'])

    def test_every_platform_names_a_runner_and_a_target(self):
        for name, entry in PLATFORMS.items():
            self.assertIn('os', entry, name)
            self.assertIn('target', entry, name)


if __name__ == '__main__':
    unittest.main()
