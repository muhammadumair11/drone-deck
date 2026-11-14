"""Read-only PX4 initialization check. Never arms, changes parameters, or flies.

Find the px4 process started for a simulator (`PX4_SIMULATOR` in its environment) and ask its
own `px4-listener` for one `vehicle_status` message. Readiness kinds the app requests:
`px4-status:<simulator>` (for example `px4-status:sihsim`), `px4-status` (any px4), and `sih`,
the earlier name of `px4-status:sihsim`.
"""
import os
from pathlib import Path
import re
import subprocess
import time

TIMEOUT = 55
SIMULATOR = re.compile(r'^[a-z0-9_]{1,32}$')


def parse(kind):
    """The simulator a readiness kind watches (None for any px4); ValueError when unknown."""
    if kind == 'sih':
        return 'sihsim'
    if kind == 'px4-status':
        return None
    if isinstance(kind, str) and kind.startswith('px4-status:'):
        simulator = kind[len('px4-status:'):]
        if SIMULATOR.fullmatch(simulator):
            return simulator
    raise ValueError('Unknown readiness check.')


def vehicle_status(simulator='sihsim'):
    """`vehicle_status` text from a running px4 for `simulator`, or None while there is none yet."""
    marker = f'PX4_SIMULATOR={simulator}'.encode() if simulator else None
    pids = subprocess.run(['pgrep', '-x', 'px4'], capture_output=True, text=True, timeout=3).stdout.split()
    for pid in pids:
        environment = Path(f'/proc/{pid}/environ').read_bytes().split(b'\0')
        if marker is not None and marker not in environment:
            continue
        listener = Path(os.readlink(f'/proc/{pid}/exe')).with_name('px4-listener')
        state = subprocess.run([str(listener), 'vehicle_status', '-n', '1'],
                               capture_output=True, text=True, timeout=3)
        if state.returncode == 0 and 'pre_flight_checks_pass:' in state.stdout:
            return state.stdout
    return None


def monitor(alive, report, check=None, timeout=TIMEOUT, sleep=time.sleep, clock=time.monotonic,
            simulator='sihsim'):
    """Reports `Ready` once PX4 publishes vehicle status, or `Failed` after `timeout` seconds.

    `alive()` says whether the owned process still runs; `report(state, detail)` emits.
    """
    check = check or (lambda: vehicle_status(simulator))
    name = 'SIH' if simulator == 'sihsim' else 'PX4'
    deadline = clock() + timeout
    while alive() and clock() < deadline:
        try:
            status = check()
        except (OSError, subprocess.SubprocessError):
            status = None
        if status is not None:
            passed = 'pre_flight_checks_pass: True' in status
            report('Ready', f'{name} vehicle status received. Preflight checks: '
                   + ('passed' if passed else 'not ready; inspect QGroundControl'))
            return True
        sleep(.5)
    if alive():
        report('Failed', f'{name} did not publish vehicle status within {timeout:g} seconds.')
    return False
