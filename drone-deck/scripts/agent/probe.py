"""Read-only checks and file listings for the app: each check runs under its own timeout."""
import os
from pathlib import Path
import signal
import subprocess
import threading
import time

OUTPUT_LIMIT = 4000
MAX_CHECKS = 64
SCAN_LIMIT = 20000


def timeout_seconds(value, default=10.0):
    try:
        seconds = float(default if value is None else value)
    except (TypeError, ValueError):
        raise ValueError('timeout_s must be a number of seconds.')
    if seconds != seconds:
        raise ValueError('timeout_s must be a number of seconds.')
    return min(max(seconds, 0.5), 300.0)


class Tail:
    """Reads a pipe to its end on a thread, keeping only the last `limit` bytes."""

    def __init__(self, pipe, limit=OUTPUT_LIMIT * 4):
        self.pipe = pipe
        self.limit = limit
        self.data = b''
        self.thread = threading.Thread(target=self._read, daemon=True)
        self.thread.start()

    def _read(self):
        try:
            for chunk in iter(lambda: self.pipe.read1(65536), b''):
                self.data = (self.data + chunk)[-self.limit:]
        except (OSError, ValueError):
            pass

    def text(self, wait):
        # A grandchild that left the process group may hold the pipe open; never wait on it.
        self.thread.join(timeout=wait)
        return self.data.decode('utf-8', 'replace')[-OUTPUT_LIMIT:].strip()


def run_check(bash, timeout, cancel=None, shell=('bash', '-lc')):
    """Runs one check; returns (ok, output). Times out or cancels by killing its process group.

    Output is bounded (a chatty check keeps only its tail) and a check never outlives its
    timeout, even when something it started keeps the output pipe open.
    """
    try:
        child = subprocess.Popen([*shell, bash], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                 stderr=subprocess.STDOUT, start_new_session=True)
    except OSError as error:
        return False, str(error)
    tail = Tail(child.stdout)
    deadline = time.monotonic() + timeout
    reason = None
    while True:
        try:
            child.wait(timeout=0.1)
            break
        except subprocess.TimeoutExpired:
            if cancel is not None and cancel.is_set():
                reason = 'cancelled'
            elif time.monotonic() >= deadline:
                reason = f'timed out after {timeout:g} s'
            if reason:
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except (ProcessLookupError, PermissionError, AttributeError):
                    child.kill()
                child.wait()
                break
    text = tail.text(1.0)
    if not tail.thread.is_alive():  # closing under a blocked reader would wait for it
        child.stdout.close()
    if reason:
        return False, (text + '\n' if text else '') + f'({reason})'
    return child.returncode == 0, text


def probe(checks, timeout_s=None, cancel=None, shell=('bash', '-lc')):
    if not isinstance(checks, list) or len(checks) > MAX_CHECKS:
        raise ValueError(f'checks must be a list of at most {MAX_CHECKS} items.')
    timeout = timeout_seconds(timeout_s)
    for check in checks:
        if not isinstance(check, dict) or not isinstance(check.get('id'), str) \
                or not isinstance(check.get('bash'), str):
            raise ValueError('Each check needs a string id and a bash command.')
    results = [None] * len(checks)

    def one(index, check):
        ok, out = run_check(check['bash'], timeout, cancel, shell)
        results[index] = {'id': check['id'], 'ok': ok, 'out': out}

    threads = [threading.Thread(target=one, args=(i, c), daemon=True) for i, c in enumerate(checks)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    return results


def files_list(directory, pattern='*', limit=500):
    if not isinstance(directory, str) or not directory:
        raise ValueError('dir is required.')
    if not isinstance(pattern, str) or not pattern or pattern.startswith('/'):
        raise ValueError('pattern must be a relative glob such as *.ulg or **/*.ulg.')
    try:
        limit = min(max(int(limit), 1), 5000)
    except (TypeError, ValueError):
        raise ValueError('limit must be a number.')
    base = Path(os.path.expanduser(directory))
    if not base.is_dir():
        return []
    rows = []
    for scanned, path in enumerate(base.glob(pattern)):
        if scanned >= SCAN_LIMIT:
            break
        try:
            if not path.is_file():
                continue
            info = path.stat()
        except OSError:
            continue
        rows.append({'path': str(path), 'size': info.st_size, 'mtime': info.st_mtime})
    rows.sort(key=lambda row: row['mtime'], reverse=True)
    return rows[:limit]
