"""Drone Deck's WSL agent: one long-lived process per distribution.

Started by the app as `wsl.exe -d <distro> --cd <Windows path of scripts/agent> --exec python3 -u
agent.py`: wsl.exe maps that folder through the distro's own drive mount. JSON lines (UTF-8) on
stdin/stdout:

- request  {"id": N, "method": "...", "params": {...}}
- reply    {"id": N, "result": ...} or {"id": N, "error": {"message": "..."}}
- event    {"event": "...", ...} without an id: output, exited, readiness, router, log, rec, progress
- {"method": "cancel", "params": {"id": N}} asks a running request to give up (probe honours it).

Closing stdin ends the agent. Owned processes keep running and the next agent lists them; the
MAVLink router stops and an active recording is finalized.
"""
import io
import json
import os
from pathlib import Path
import platform
import re
import sys
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))

import channel  # noqa: E402
import pipe  # noqa: E402
import probe  # noqa: E402
import procs  # noqa: E402
import router  # noqa: E402
import services  # noqa: E402

VERSION = 1
# Fast methods run in order on the reader thread (input lines must not overtake each other);
# the rest run on their own threads.
INLINE = {'hello', 'run', 'input', 'list', 'adopt', 'router.status'}
MOUNT = re.compile(r'^(/(?:.*/)?)[a-z]$')
# The runtime scripts folder as this distro sees it; owned processes get it as DRONE_DECK_SCRIPTS.
SCRIPTS = str(Path(__file__).resolve().parent.parent)


def automount_root(conf='/etc/wsl.conf', mounts='/proc/mounts'):
    """Where Windows drives are mounted: `[automount] root` in wsl.conf, else the mounts seen."""
    try:
        section = None
        for raw in Path(conf).read_text().splitlines():
            line = raw.split('#', 1)[0].strip()
            if line.startswith('[') and line.endswith(']'):
                section = line[1:-1].strip().lower()
            elif section == 'automount' and '=' in line:
                name, value = (part.strip() for part in line.split('=', 1))
                value = value.strip('"\'')
                if name.lower() == 'root' and value.startswith('/'):
                    return value if value.endswith('/') else value + '/'
    except OSError:
        pass
    try:
        for line in Path(mounts).read_text().splitlines():
            parts = line.split()
            if len(parts) >= 3 and parts[2] in ('9p', 'drvfs'):
                match = MOUNT.match(parts[1])
                if match:
                    return match.group(1)
    except OSError:
        pass
    return '/mnt/'


def user_name():
    try:
        import pwd
        return pwd.getpwuid(os.getuid()).pw_name
    except (ImportError, KeyError, AttributeError):
        return os.environ.get('USER', '')


def hello():
    return {'version': VERSION, 'distro': os.environ.get('WSL_DISTRO_NAME', ''), 'user': user_name(),
            'home': str(Path.home()), 'automount_root': automount_root(),
            'python': platform.python_version(), 'pid': os.getpid()}


def run_env(env):
    """A `run` request's environment, with DRONE_DECK_SCRIPTS unless the request sets it."""
    if env is None:
        env = {}
    if not isinstance(env, dict):
        raise ValueError('env must be an object.')
    return {'DRONE_DECK_SCRIPTS': SCRIPTS, **env}


def params_of(request):
    params = request.get('params')
    if params is None:
        return {}
    if not isinstance(params, dict):
        raise ValueError('params must be an object.')
    return params


class Agent:
    def __init__(self, out, processes=None, mavlink=None, recorder=None, data=None, shell=('bash', '-lc'),
                 pipes=None):
        self.out = out
        self.shell = shell
        self.procs = processes or procs.Processes(out, shell=shell)
        self.router = mavlink or router.Router(out)
        self.recorder = recorder or services.Recorder(out)
        self.data = data or services.Data()
        self.pipes = pipes or pipe.Pipelines(out, self.procs, mounts=automount_root())
        self.cancels = {}
        self.cancels_lock = threading.Lock()
        self.workers = []
        self.table = {
            'hello': lambda p, c, i: hello(),
            'run': lambda p, c, i: self.procs.run(p.get('key'), p.get('bash'), run_env(p.get('env')),
                                                  p.get('cwd'), p.get('readiness')),
            'stop': lambda p, c, i: self.procs.stop(p.get('key'), p.get('grace_s', 3)),
            'input': lambda p, c, i: self.procs.input(p.get('key'), p.get('line')),
            'list': lambda p, c, i: self.procs.list(),
            'adopt': lambda p, c, i: self.procs.adopt(p.get('key')),
            'probe': lambda p, c, i: probe.probe(p.get('checks'), p.get('timeout_s'), c, self.shell),
            'files.list': lambda p, c, i: probe.files_list(p.get('dir'), p.get('pattern', '*'),
                                                            p.get('limit', 500)),
            'router.start': lambda p, c, i: self.router.start(p.get('udp_listen', 14550),
                                                              p.get('tcp_port', 5760),
                                                              p.get('udp_host', '127.0.0.1'),
                                                              p.get('tcp_host', '0.0.0.0')),
            'router.stop': lambda p, c, i: self.router.stop(),
            'router.status': lambda p, c, i: self.router.status(),
        }

    def method(self, name):
        handler = self.table.get(name)
        if handler is not None:
            return handler
        if name.startswith('rec.'):
            return lambda p, c, i: self.recorder.call(name[4:], p)
        if name.startswith('pipe.'):
            return lambda p, c, i: self.pipes.call(
                name[5:], p, c, lambda message: self.out.event('progress', id=i, message=message))
        if name.startswith('data.'):
            return lambda p, c, i: self.data.call(
                name[5:], p, lambda message: self.out.event('progress', id=i, message=message))
        return None

    def handle(self, line):
        """Handles one input line. Returns False when the agent should stop reading."""
        try:
            request = json.loads(line)
        except ValueError:
            self.out.log('warn', 'Ignored a request line that is not JSON.')
            return True
        if not isinstance(request, dict):
            self.out.log('warn', 'Ignored a request that is not an object.')
            return True
        request_id = request.get('id')
        if request_id is not None and (isinstance(request_id, bool)
                                       or not isinstance(request_id, (int, str))):
            # An id the reply could not carry back (or a dict key) is a protocol error.
            self.out.log('warn', 'Ignored a request whose id is not a number or a string.')
            return True
        name = request.get('method')
        if not isinstance(name, str):
            if request_id is not None:
                self.out.error(request_id, 'method must be a string.')
            return True
        try:
            params = params_of(request)
        except ValueError as error:
            if request_id is not None:
                self.out.error(request_id, error)
            return True
        if name == 'cancel':
            target = params.get('id')
            with self.cancels_lock:
                flag = self.cancels.get(target) if isinstance(target, (int, str)) else None
            if flag is not None:
                flag.set()
            if request_id is not None:
                self.out.reply(request_id, {'cancelled': flag is not None})
            return True
        if name == 'shutdown':
            if request_id is not None:
                self.out.reply(request_id, {})
            return False
        handler = self.method(name)
        if handler is None:
            if request_id is not None:
                self.out.error(request_id, f'Unknown method: {name}')
            return True
        cancel = threading.Event()
        if request_id is not None:
            with self.cancels_lock:
                self.cancels[request_id] = cancel
        if name in INLINE:
            self.execute(request_id, handler, params, cancel)
        else:
            worker = threading.Thread(target=self.execute, args=(request_id, handler, params, cancel),
                                      daemon=True)
            self.workers = [w for w in self.workers if w.is_alive()] + [worker]
            worker.start()
        return True

    def execute(self, request_id, handler, params, cancel):
        try:
            result = handler(params, cancel, request_id)
        except Exception as error:  # noqa: BLE001 - every failure becomes the request's error
            if request_id is not None:
                self.out.error(request_id, str(error) or type(error).__name__)
        else:
            if request_id is not None:
                self.out.reply(request_id, result)
        finally:
            if request_id is not None:
                with self.cancels_lock:
                    self.cancels.pop(request_id, None)

    def serve(self, stream):
        try:
            for line in stream:
                if not line.strip():
                    continue
                try:
                    keep_reading = self.handle(line)
                except Exception as error:  # noqa: BLE001 - one bad request never ends the agent
                    self.out.log('error', f'Request failed before dispatch: {error}')
                    keep_reading = True
                if not keep_reading:
                    break
        finally:
            self.close()

    def close(self, wait=2.0):
        # Finalizing a recording matters most and the app waits only a few seconds, so it goes
        # first; requests still in flight get a short, shared grace period afterwards.
        for step in (self.recorder.close, self.router.stop, self.procs.detach):
            try:
                step()
            except Exception as error:  # noqa: BLE001 - shutting down; report and continue
                self.out.log('error', f'Agent shutdown: {error}')
        with self.cancels_lock:
            pending = list(self.cancels.values())
        for cancel in pending:
            cancel.set()
        deadline = time.monotonic() + wait
        for worker in self.workers:
            worker.join(timeout=max(0.0, deadline - time.monotonic()))


def protocol_stream():
    """The protocol's own copy of stdout; fd 1 itself is pointed at stderr.

    Anything a library prints, and any child process that inherits fd 1, then lands on stderr
    (shown as agent log lines) and can never corrupt the JSON-lines stream.
    """
    sys.stdout.flush()
    protocol = os.dup(1)  # not inherited by children (PEP 446)
    os.dup2(2, 1)
    sys.stdout = sys.stderr
    return io.TextIOWrapper(io.FileIO(protocol, 'w'), encoding='utf-8', errors='replace',
                            newline='\n', write_through=True)


def main():
    stdin = io.TextIOWrapper(sys.stdin.buffer, encoding='utf-8', errors='replace')
    Agent(channel.Out(protocol_stream())).serve(stdin)


if __name__ == '__main__':
    main()
