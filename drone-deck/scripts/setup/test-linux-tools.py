import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('linux_tools', Path(__file__).with_name('linux-tools.py'))
tools = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tools)

class DiscoveryTests(unittest.TestCase):
    def test_preflight_rejects_unowned_targets_without_running_commands(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory, patch.object(tools, 'ROOT', Path(directory)), patch.object(tools, 'require_noble'), patch.object(tools, 'run') as command:
            (Path(directory) / 'mavsdk').mkdir()
            with self.assertRaisesRegex(RuntimeError, 'unowned'):
                tools.preflight('mavsdk')
            command.assert_not_called()

    def test_managed_target_ownership_is_explicit(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory, patch.object(tools, 'ROOT', Path(directory)):
            path = tools.owned_target('mavsdk')
            self.assertTrue((Path(directory) / '.owned-mavsdk').is_file())
            path.mkdir()
            self.assertEqual(tools.owned_target('mavsdk'), path)

    def test_missing_tools_are_not_marked_compatible(self):
        with patch.object(tools, 'ros_env', return_value=None):
            result = tools.probe('ros2')
        self.assertFalse(result['found'])
        self.assertFalse(result['compatible'])

    def test_custom_paths_are_passed_as_literal_arguments(self):
        with tempfile.TemporaryDirectory(prefix="quotes' spaces ",dir=Path(__file__).parent) as directory:
            path = Path(directory) / 'bin/python'
            path.parent.mkdir()
            path.touch()
            with patch.object(tools, 'output', return_value='4.1.0') as query:
                result = tools.probe('mavsdk', directory)
            self.assertEqual(query.call_args.args[0][0], str(path))
            self.assertEqual(result['executable'], str(path))
            self.assertTrue(result['compatible'])

    def test_postinstall_verification_targets_the_managed_location(self):
        with patch.object(tools, 'probe', return_value={'found': True}) as probe:
            tools.verify('px4')
        self.assertEqual(probe.call_args.args[1], str(tools.ROOT / 'PX4-Autopilot'))

if __name__ == '__main__':
    unittest.main()


class CloneTests(unittest.TestCase):
    def test_a_half_clone_is_removed_and_a_good_checkout_is_kept(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
            half = Path(directory) / 'half'
            (half / '.git').mkdir(parents=True)  # no commit: a clone that was cut off
            good = Path(directory) / 'good'
            (good / '.git').mkdir(parents=True)
            with patch.object(tools, 'output', side_effect=lambda a: 'abc123' if a[2].endswith('good') else '') as query, \
                 patch.object(tools, 'run') as command:
                tools.clone(['url'], good)
                command.assert_not_called()
                self.assertTrue(good.exists())
                tools.clone(['url'], half)
                self.assertFalse((half / '.git').exists(), 'the half clone is cleared before cloning again')
                self.assertEqual(command.call_args.args[0][:5], tools.GIT[:5] + [])

    def test_a_failed_clone_leaves_nothing_behind_and_gives_up_on_dead_connections(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
            path = Path(directory) / 'checkout'

            def fail(args, **kwargs):
                path.mkdir()
                raise tools.subprocess.CalledProcessError(128, args)
            with patch.object(tools, 'run', side_effect=fail) as command:
                with self.assertRaises(tools.subprocess.CalledProcessError):
                    tools.clone(['url'], path)
            self.assertFalse(path.exists())
            self.assertIn('http.lowSpeedTime=120', command.call_args.args[0])
