"""Owned Linux process groups for the agent: run, stop, input, list, adopt.

Each process runs as `bash -lc <command>` in its own session, so it leads a process group that
the agent created and may signal. Ownership survives the agent:

- `<key>.json` (pidfile): pid, /proc birth time, start time. A pid only counts as ours while its
  birth time matches, so a reused pid is never signalled.
- `<key>.lock`: flock taken by the agent and inherited by the child, so a second start of the
  same key is refused for as long as any process of the group holds it.
- `<key>.out` / `<key>.err`: the child's output. The agent tails these files instead of pipes,
  so the process keeps running if the agent exits and a later agent can adopt and follow it.
- `<key>.in`: a FIFO the child opens read-write as stdin. It never reaches EOF (PX4's shell
  spins on EOF) and `input` writes lines to it.

Files live in `/tmp/drone-deck-<uid>/` (mode 0700), shared with the older `supervisor.py`
pidfiles, which `list` also reports.
"""
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import threading
import time

try:
    import fcntl
except ImportError:  # Windows: the module imports for tooling; processes need Linux.
    fcntl = None

import sih

KEY = re.compile(r'^[A-Za-z0-9_-]{1,64}$')
ENV_NAME = re.compile(r'^[A-Za-z_][A-Za-z0-9_]*$')
LINE_LIMIT = 4096
LOG_LIMIT = 8 * 1024 * 1024
READ_LIMIT = 256 * 1024
TAIL_INTERVAL = 0.1
ADOPT_CONTEXT = 16 * 1024
STREAMS = (('stdout', 'out'), ('stderr', 'err'))


def state_dir():
    override = os.environ.get('DRONE_DECK_AGENT_STATE')
    return Path(override) if override else Path('/tmp') / f'drone-deck-{os.getuid()}'


def valid_key(key):
    if not isinstance(key, str) or not KEY.fullmatch(key):
        raise ValueError('Process key: use 1-64 letters, numbers, hyphens or underscores.')
    return key


def birth(pid):
    """Start time of a live `pid` in clock ticks since boot (/proc stat field 22), else None.

    Zombies count as gone: their group has exited and only waits to be reaped.
    """
    if not isinstance(pid, int) or pid <= 0:
        return None
    try:
        text = Path(f'/proc/{pid}/stat').read_text()
    except OSError:
        return None
    # The command name (field 2) may contain spaces and parentheses; fields after it may not.
    fields = text.rsplit(')', 1)[-1].split()
    if len(fields) < 20 or fields[0] in ('Z', 'X'):
        return None
    return fields[19]


def started_at(ticks):
    """Epoch seconds for a /proc start time, from the boot time in /proc/stat."""
    try:
        for line in Path('/proc/stat').read_text().splitlines():
            if line.startswith('btime '):
                return int(line.split()[1]) + int(ticks) / os.sysconf('SC_CLK_TCK')
    except (OSError, ValueError, TypeError):
        pass
    return None


def group_alive(pgid):
    try:
        os.killpg(pgid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


def signal_group(pgid, number):
    try:
        os.killpg(pgid, number)
    except (ProcessLookupError, PermissionError):
        pass


def lock_held(path):
    """Whether some process holds the key's flock: a member of the group it was started with.

    The lock is inherited by every process of the group, so it outlives a leader that exited
    while the agent was away and its children kept running.
    """
    if fcntl is None:
        return False
    try:
        fd = os.open(path, os.O_RDWR | os.O_CLOEXEC)
    except OSError:
        return False
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        return True
    except OSError:
        return False
    else:
        fcntl.flock(fd, fcntl.LOCK_UN)
        return False
    finally:
        os.close(fd)


def record_alive(record, lock_path):
    """A pidfile's process group still runs: its leader with the recorded birth time, or (leader
    gone) members of the group that still hold the key's lock. The group id cannot be reused
    while any member lives, so signalling it reaches only our processes."""
    if record is None:
        return False
    if record['birth'] is not None and birth(record['pid']) == record['birth']:
        return True
    return group_alive(record['pid']) and lock_held(lock_path)


def read_record(path):
    try:
        if path.stat().st_size > 65536:
            return None
        record = json.loads(path.read_text())
    except (OSError, ValueError):
        return None
    if not isinstance(record, dict) or not isinstance(record.get('pid'), int):
        return None
    record['birth'] = None if record.get('birth') is None else str(record['birth'])
    return record


def write_record(path, value):
    temporary = path.with_name(path.name + '.tmp')
    temporary.write_text(json.dumps(value))
    os.chmod(temporary, 0o600)
    temporary.replace(path)


class Entry:
    """A process whose output and exit this agent follows: started here, or adopted."""

    def __init__(self, key, pid, ticks, since, child=None):
        self.key = key
        self.pid = pid
        self.birth = ticks
        self.since = since
        self.child = child
        self.lock_fd = None
        self.done = False
        self.finished = threading.Event()
        self.streams = {}

    def attach(self, paths, from_start):
        for stream, suffix in STREAMS:
            path = paths[suffix]
            offset = 0
            skip = False
            if not from_start:
                try:
                    size = path.stat().st_size
                except OSError:
                    size = 0
                offset = max(0, size - ADOPT_CONTEXT)
                skip = offset > 0
            self.streams[stream] = {'path': path, 'offset': offset, 'partial': b'', 'skip': skip}


class Processes:
    def __init__(self, out, directory=None, shell=('bash', '-lc'), tail_interval=TAIL_INTERVAL,
                 readiness_monitor=None):
        self.out = out
        self.dir = Path(directory) if directory else state_dir()
        self.shell = list(shell)
        self.tail_interval = tail_interval
        self.readiness_monitor = readiness_monitor or sih.monitor
        self.entries = {}
        self.lock = threading.RLock()
        self.stopping = threading.Event()
        self.tailer = None

    # -- files -----------------------------------------------------------------------------
    def prepare(self):
        self.dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        info = os.lstat(self.dir)
        if stat.S_ISLNK(info.st_mode) or info.st_uid != os.getuid():
            raise RuntimeError(f'{self.dir} is not a folder owned by this user; refusing to use it.')
        if stat.S_IMODE(info.st_mode) != 0o700:
            os.chmod(self.dir, 0o700)

    def paths(self, key):
        return {suffix: self.dir / f'{key}.{suffix}' for suffix in ('json', 'lock', 'out', 'err', 'in')}

    def remove_stale(self, key, record):
        paths = self.paths(key)
        current = read_record(paths['json'])
        if current is not None and record is not None and current.get('pid') != record.get('pid'):
            return
        for suffix in ('json', 'in'):
            try:
                paths[suffix].unlink()
            except FileNotFoundError:
                pass

    # -- run -------------------------------------------------------------------------------
    def run(self, key, bash, env=None, cwd=None, readiness=None):
        key = valid_key(key)
        if not isinstance(bash, str) or not bash.strip():
            raise ValueError('The command is empty.')
        if readiness not in (None, '', 'sih'):
            raise ValueError('Unknown readiness check.')
        environment = dict(os.environ)
        for name, value in (env or {}).items():
            if not isinstance(name, str) or not ENV_NAME.fullmatch(name):
                raise ValueError(f'Invalid environment variable name: {name!r}')
            environment[name] = str(value)
        directory = None
        if cwd:
            directory = os.path.expanduser(str(cwd))
            if not os.path.isdir(directory):
                raise ValueError(f'Working folder does not exist: {directory}')
        if fcntl is None:
            raise RuntimeError('Owned processes need Linux.')
        self.prepare()
        paths = self.paths(key)
        with self.lock:
            if key in self.entries:
                raise RuntimeError(f'{key} is already running in Drone Deck.')
            lock_fd = os.open(paths['lock'], os.O_RDWR | os.O_CREAT | os.O_CLOEXEC, 0o600)
            try:
                fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                os.close(lock_fd)
                raise RuntimeError(f'{key} is already running from an earlier Drone Deck session. '
                                   'Adopt or stop it first.')
            try:
                entry = self._spawn(key, bash, environment, directory, lock_fd, paths)
            except Exception:
                os.close(lock_fd)
                raise
            self.entries[key] = entry
        threading.Thread(target=self._wait, args=(entry,), daemon=True).start()
        self._ensure_tailer()
        if readiness == 'sih':
            def report(state, detail):
                self.out.event('readiness', key=key, state=state, detail=detail)
            threading.Thread(target=self.readiness_monitor,
                             args=(lambda: entry.child.poll() is None, report), daemon=True).start()
        return {'key': key, 'pid': entry.pid, 'since': entry.since}

    def _spawn(self, key, bash, environment, directory, lock_fd, paths):
        for suffix in ('out', 'err'):
            os.close(os.open(paths[suffix], os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600))
        try:
            paths['in'].unlink()
        except FileNotFoundError:
            pass
        os.mkfifo(paths['in'], 0o600)
        stdin = os.open(paths['in'], os.O_RDWR | os.O_CLOEXEC)
        stdout = os.open(paths['out'], os.O_WRONLY | os.O_APPEND | os.O_CLOEXEC)
        stderr = os.open(paths['err'], os.O_WRONLY | os.O_APPEND | os.O_CLOEXEC)
        try:
            child = subprocess.Popen([*self.shell, bash], stdin=stdin, stdout=stdout, stderr=stderr,
                                     env=environment, cwd=directory, start_new_session=True,
                                     pass_fds=(lock_fd,))
        finally:
            for fd in (stdin, stdout, stderr):
                os.close(fd)
        since = time.time()
        entry = Entry(key, child.pid, birth(child.pid), since, child)
        entry.lock_fd = lock_fd
        try:
            write_record(paths['json'], {'pid': child.pid, 'birth': entry.birth, 'since': since,
                                         'command': bash[:2000], 'output': True})
        except OSError:
            # Without a pidfile nobody could find or stop it later: do not leave it running.
            signal_group(child.pid, signal.SIGKILL)
            child.wait()
            raise
        entry.attach(paths, from_start=True)
        return entry

    def _wait(self, entry):
        code = entry.child.wait()
        # Stragglers in the owned group would hold ports and the key's lock.
        if group_alive(entry.pid):
            signal_group(entry.pid, signal.SIGTERM)
            deadline = time.monotonic() + 2
            while group_alive(entry.pid) and time.monotonic() < deadline:
                time.sleep(.05)
            signal_group(entry.pid, signal.SIGKILL)
        self._finish(entry, code)

    def _watch(self, entry):
        record = {'pid': entry.pid, 'birth': entry.birth}
        lock_path = self.paths(entry.key)['lock']
        while not self.stopping.is_set() and record_alive(record, lock_path):
            time.sleep(.25)
        if not self.stopping.is_set():
            self._finish(entry, None)

    def _finish(self, entry, code):
        with self.lock:
            if entry.done:
                return
            entry.done = True
            self._drain(entry, final=True)
            if self.entries.get(entry.key) is entry:
                del self.entries[entry.key]
        if entry.lock_fd is not None:
            os.close(entry.lock_fd)
            entry.lock_fd = None
        self.remove_stale(entry.key, {'pid': entry.pid})
        self.out.event('exited', key=entry.key, code=code)
        entry.finished.set()

    # -- output ----------------------------------------------------------------------------
    def _ensure_tailer(self):
        with self.lock:
            if self.tailer is None or not self.tailer.is_alive():
                self.tailer = threading.Thread(target=self._tail, daemon=True)
                self.tailer.start()

    def _tail(self):
        while not self.stopping.wait(self.tail_interval):
            with self.lock:
                for entry in list(self.entries.values()):
                    self._drain(entry)

    def _drain(self, entry, final=False):
        for stream, state in entry.streams.items():
            try:
                with open(state['path'], 'rb') as handle:
                    size = os.fstat(handle.fileno()).st_size
                    if size < state['offset']:
                        state['offset'] = 0
                    handle.seek(state['offset'])
                    data = handle.read(READ_LIMIT)
            except OSError:
                continue
            state['offset'] += len(data)
            self._lines(entry.key, stream, state, data, final)
            # Bound disk use: the child appends (O_APPEND), so after a truncate it writes at 0.
            if state['offset'] > LOG_LIMIT and not state['partial']:
                try:
                    os.truncate(state['path'], 0)
                    state['offset'] = 0
                except OSError:
                    pass

    def _lines(self, key, stream, state, data, final):
        buffer = state['partial'] + data
        *lines, rest = buffer.split(b'\n')
        for line in lines:
            if state['skip']:
                state['skip'] = False
                continue
            self._emit(key, stream, line)
        if len(rest) > LINE_LIMIT:
            if not state['skip']:
                self._emit(key, stream, rest)
            state['skip'] = True
            rest = b''
        if final and rest:
            if not state['skip']:
                self._emit(key, stream, rest)
            rest = b''
        state['partial'] = rest

    def _emit(self, key, stream, raw):
        line = raw[:LINE_LIMIT].decode('utf-8', 'replace').rstrip('\r')
        self.out.event('output', key=key, stream=stream, line=line)

    # -- stop, input, list, adopt ----------------------------------------------------------
    def stop(self, key, grace_s=3):
        key = valid_key(key)
        try:
            grace = min(max(float(grace_s), 0.0), 30.0)
        except (TypeError, ValueError):
            raise ValueError('grace_s must be a number of seconds.')
        paths = self.paths(key)
        record = read_record(paths['json'])
        if not record_alive(record, paths['lock']):
            if record is not None:
                with self.lock:
                    attached = key in self.entries
                if not attached:
                    self.remove_stale(key, record)
            return {'key': key, 'stopped': False, 'running': False}
        pid = record['pid']
        signal_group(pid, signal.SIGTERM)
        deadline = time.monotonic() + grace
        while time.monotonic() < deadline and group_alive(pid):
            time.sleep(.05)
        if group_alive(pid):
            signal_group(pid, signal.SIGKILL)
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline and group_alive(pid):
                time.sleep(.05)
        running = group_alive(pid)
        with self.lock:
            entry = self.entries.get(key)
        if entry is not None and not running:
            # Report `exited` before the reply, so a start of the same key right after this
            # stop is never mistaken for (or refused because of) the process that just ended.
            entry.finished.wait(3)
        return {'key': key, 'stopped': True, 'running': running}

    def input(self, key, line):
        key = valid_key(key)
        if not isinstance(line, str) or '\n' in line or len(line.encode()) >= LINE_LIMIT:
            raise ValueError('Input must be one line of at most 4095 bytes.')
        fifo = self.paths(key)['in']
        try:
            if not stat.S_ISFIFO(os.stat(fifo).st_mode):
                raise FileNotFoundError
            fd = os.open(fifo, os.O_WRONLY | os.O_NONBLOCK)
        except FileNotFoundError:
            raise RuntimeError(f'{key} is not running or does not accept input.')
        except OSError as error:
            raise RuntimeError(f'{key} is not reading input ({error.strerror}).')
        data = (line + '\n').encode()
        try:
            # At most PIPE_BUF (4096) bytes are written atomically; a longer line could tear.
            if os.write(fd, data) != len(data):
                raise RuntimeError(f'{key} took only part of the line.')
        except BlockingIOError:
            raise RuntimeError(f'{key} is not reading its input.')
        finally:
            os.close(fd)
        return {}

    def list(self):
        rows = []
        if not self.dir.is_dir():
            return rows
        for path in sorted(self.dir.glob('*.json')):
            key = path.stem
            if not KEY.fullmatch(key):
                continue
            record = read_record(path)
            if record is None:
                continue
            alive = record_alive(record, self.dir / f'{key}.lock')
            with self.lock:
                attached = key in self.entries
            since = record.get('since')
            if not isinstance(since, (int, float)):
                since = started_at(record['birth']) if alive else None
            age = max(0.0, time.time() - since) if since is not None else None
            rows.append({'key': key, 'pid': record['pid'], 'since': since, 'age_s': age, 'alive': alive,
                         'attached': attached, 'output': bool(record.get('output')),
                         'command': str(record.get('command', ''))[:2000]})
            if not alive and not attached:
                self.remove_stale(key, record)
        return rows

    def adopt(self, key):
        key = valid_key(key)
        paths = self.paths(key)
        record = read_record(paths['json'])
        if not record_alive(record, paths['lock']):
            raise RuntimeError(f'{key} is no longer running.')
        output = bool(record.get('output'))
        with self.lock:
            entry = self.entries.get(key)
            if entry is not None:
                return {'key': key, 'pid': entry.pid, 'output': output}
            since = record.get('since') if isinstance(record.get('since'), (int, float)) else None
            entry = Entry(key, record['pid'], record['birth'], since)
            if output:
                entry.attach(paths, from_start=False)
            self.entries[key] = entry
        threading.Thread(target=self._watch, args=(entry,), daemon=True).start()
        self._ensure_tailer()
        return {'key': key, 'pid': entry.pid, 'output': output}

    def detach(self):
        """Stops following processes and leaves them running for a later agent."""
        self.stopping.set()
        with self.lock:
            for entry in self.entries.values():
                self._drain(entry)
