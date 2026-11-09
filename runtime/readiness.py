"""Read-only SIH initialization check. Never arms, changes parameters, or flies."""
import json
import os
from pathlib import Path
import subprocess
import time

def report(state, detail):
    print('@readiness ' + json.dumps(dict(state=state, detail=detail)), flush=True)

def monitor(child):
    deadline = time.monotonic() + 55
    while child.poll() is None and time.monotonic() < deadline:
        try:
            pids = subprocess.run(['pgrep', '-x', 'px4'], capture_output=True, text=True, timeout=3).stdout.split()
            for pid in pids:
                environment = Path(f'/proc/{pid}/environ').read_bytes().split(b'\0')
                if b'PX4_SIMULATOR=sihsim' not in environment:
                    continue
                listener = Path(os.readlink(f'/proc/{pid}/exe')).with_name('px4-listener')
                state = subprocess.run([str(listener), 'vehicle_status', '-n', '1'], capture_output=True, text=True, timeout=3)
                if state.returncode == 0 and 'pre_flight_checks_pass:' in state.stdout:
                    passed = 'pre_flight_checks_pass: True' in state.stdout
                    report('Ready', 'SIH vehicle status received. Preflight checks: ' + ('passed' if passed else 'not ready; inspect QGroundControl'))
                    return
        except (OSError, subprocess.SubprocessError):
            pass
        time.sleep(.5)
    if child.poll() is None:
        report('Failed', 'SIH did not publish vehicle status within 55 seconds.')
