"""Own a Linux process group; stop only groups created by this application."""
import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import threading

def main():
    action, name = sys.argv[1:3]
    if not name.replace('-', '').replace('_', '').isalnum():
        raise ValueError('Invalid profile id')
    directory = Path('/tmp') / f'drone-deck-{os.getuid()}'
    directory.mkdir(mode=0o700, exist_ok=True)
    pidfile = directory / (name + '.json')
    if action == 'stop':
        if not pidfile.exists():
            return
        entry = json.loads(pidfile.read_text())
        proc = Path(f'/proc/{entry["pid"]}/stat')
        if proc.exists() and proc.read_text().split()[21] == entry['birth']:
            os.killpg(entry['pid'], signal.SIGTERM)
            for _ in range(30):
                if not proc.exists():
                    break
                time.sleep(0.1)
            if proc.exists():
                os.killpg(entry['pid'], signal.SIGKILL)
        return
    with (directory / (name + '.lock')).open('w') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise SystemExit('This profile is already running in Drone Deck.')
        env = os.environ.copy()
        env['DRONE_DECK_SCRIPTS'] = sys.argv[3]
        # Keep a writable stdin open: PX4's interactive shell spins on immediate EOF.
        # No input is forwarded; stopping still targets this owned process group.
        child = subprocess.Popen(['bash', '-lc', sys.argv[4]], env=env,
                                 stdin=subprocess.PIPE, start_new_session=True)
        birth = Path(f'/proc/{child.pid}/stat').read_text().split()[21]
        pidfile.write_text(json.dumps({'pid': child.pid, 'birth': birth}))
        if name == 'sih':
            from readiness import monitor
            threading.Thread(target=monitor, args=(child,), daemon=True).start()
        def terminate(*_):
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
        signal.signal(signal.SIGTERM, terminate)
        signal.signal(signal.SIGINT, terminate)
        try:
            raise SystemExit(child.wait())
        finally:
            child.stdin.close()
            pidfile.unlink(missing_ok=True)

if __name__ == '__main__':
    main()
