"""Read-only SIH initialization check. Never arms, changes parameters, or flies.

Find the px4 process started for SIH and ask its own `px4-listener` for one `vehicle_status`
message.
"""
import os
from pathlib import Path
import subprocess
import time

TIMEOUT = 55


def vehicle_status():
    """`vehicle_status` text from a running SIH px4, or None while there is none yet."""
    pids = subprocess.run(['pgrep', '-x', 'px4'], capture_output=True, text=True, timeout=3).stdout.split()
    for pid in pids:
        environment = Path(f'/proc/{pid}/environ').read_bytes().split(b'\0')
        if b'PX4_SIMULATOR=sihsim' not in environment:
            continue
        listener = Path(os.readlink(f'/proc/{pid}/exe')).with_name('px4-listener')
        state = subprocess.run([str(listener), 'vehicle_status', '-n', '1'],
                               capture_output=True, text=True, timeout=3)
        if state.returncode == 0 and 'pre_flight_checks_pass:' in state.stdout:
            return state.stdout
    return None


def monitor(alive, report, check=vehicle_status, timeout=TIMEOUT, sleep=time.sleep, clock=time.monotonic):
    """Reports `Ready` once SIH publishes vehicle status, or `Failed` after `timeout` seconds.

    `alive()` says whether the owned process still runs; `report(state, detail)` emits.
    """
    deadline = clock() + timeout
    while alive() and clock() < deadline:
        try:
            status = check()
        except (OSError, subprocess.SubprocessError):
            status = None
        if status is not None:
            passed = 'pre_flight_checks_pass: True' in status
            report('Ready', 'SIH vehicle status received. Preflight checks: '
                   + ('passed' if passed else 'not ready; inspect QGroundControl'))
            return True
        sleep(.5)
    if alive():
        report('Failed', f'SIH did not publish vehicle status within {timeout:g} seconds.')
    return False
