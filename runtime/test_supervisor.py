"""Exercise real group ownership without starting a flight simulator."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import unittest

class SupervisorTest(unittest.TestCase):
    def test_child_input_does_not_immediately_reach_eof(self):
        script = str(Path(__file__).with_name('supervisor.py'))
        name = 'test-input-' + str(os.getpid())
        command = [sys.executable, script, 'run', name, '/tmp',
                   "python3 -c 'import sys; sys.stdin.read(1); print(\"UNEXPECTED_EOF\")'"]
        child = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            time.sleep(.5)
            self.assertIsNone(child.poll(), 'child saw EOF instead of waiting for input')
            subprocess.run([sys.executable, script, 'stop', name], check=True, timeout=8)
            out, _ = child.communicate(timeout=8)
            self.assertNotIn(b'UNEXPECTED_EOF', out)
        finally:
            if child.poll() is None:
                child.terminate(); child.communicate(timeout=8)

    def test_owned_group_stops_and_lock_rejects_duplicate(self):
        script = str(Path(__file__).with_name('supervisor.py'))
        name = 'test-' + str(os.getpid())
        pidfile = Path('/tmp') / f'drone-deck-{os.getuid()}' / (name + '.json')
        command = [sys.executable, script, 'run', name, '/tmp', 'sleep 60']
        first = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            for _ in range(100):
                if pidfile.exists():
                    break
                time.sleep(.05)
            self.assertTrue(pidfile.exists(), 'supervisor did not create an ownership record')
            entry = json.loads(pidfile.read_text())
            second = subprocess.run(command, capture_output=True, timeout=5)
            self.assertNotEqual(second.returncode, 0)
            subprocess.run([sys.executable, script, 'stop', name], check=True, timeout=8)
            first.communicate(timeout=8)
            self.assertFalse(Path(f'/proc/{entry["pid"]}').exists())
            self.assertFalse(pidfile.exists())
        finally:
            if first.poll() is None:
                first.terminate()
                first.communicate(timeout=8)

if __name__ == '__main__':
    unittest.main()
