"""Drone Deck's owned rosbag2/MCAP worker. JSON-lines over stdin/stdout; no shell input."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import queue
import re
import shutil
import signal
import subprocess
import sys
import threading
import time
import uuid

SETUP = '/opt/ros/jazzy/setup.bash'
TOPIC = re.compile(r'^/[A-Za-z_][A-Za-z0-9_]*(/[A-Za-z_][A-Za-z0-9_]*)*$')
SESSION = re.compile(r'^[A-Za-z0-9_-]{1,110}$')
OUTPUT_LOCK = threading.Lock()

def emit(value):
    with OUTPUT_LOCK:
        print(json.dumps(value), flush=True)

def domain(value):
    number = int(value)
    if not 0 <= number <= 232:
        raise ValueError('ROS domain must be between 0 and 232.')
    return number

def topics(value):
    if not isinstance(value, list) or not 1 <= len(value) <= 128:
        raise ValueError('Select between 1 and 128 explicit ROS topics.')
    if any(not isinstance(t, str) or not TOPIC.fullmatch(t) for t in value):
        raise ValueError('Use absolute ROS topic names, for example /camera/image_raw.')
    return list(dict.fromkeys(value))

def load_environment():
    if not Path(SETUP).is_file():
        raise RuntimeError('ROS 2 Jazzy is missing. Install ros-jazzy-ros-base and ros-jazzy-rosbag2-storage-mcap in this WSL distribution.')
    result = subprocess.run(['bash', '-c', 'source "$1" && env -0', 'drone-deck', SETUP],
                            capture_output=True, timeout=15, check=True)
    env = dict(os.environ)
    for item in result.stdout.split(b'\0'):
        if b'=' in item:
            key, value = item.split(b'=', 1)
            env[key.decode()] = value.decode()
    env['PYTHONUNBUFFERED'] = '1'
    return env

def bag_metadata(path):
    file = path / 'bag' / 'metadata.yaml'
    if not file.is_file() or file.stat().st_size > 8_000_000:
        return {}
    import yaml
    try:
        document = yaml.safe_load(file.read_text())
    except (yaml.YAMLError, OSError):
        return {}
    if not isinstance(document, dict):
        return {}
    metadata = document.get('rosbag2_bagfile_information', {})
    return metadata if isinstance(metadata, dict) else {}

def atomic_json(path, value):
    temporary = path.with_suffix('.json.tmp')
    temporary.write_text(json.dumps(value, indent=2))
    temporary.replace(path)

class Server:
    def __init__(self, root):
        self.root = Path(root).expanduser().resolve()
        self.env = None
        self.ready = False
        self.jobs = {}

    def command(self, args, domain_id=0, local=False):
        if self.env is None:
            self.env = load_environment()
        env = dict(self.env)
        env['ROS_DOMAIN_ID'] = str(domain(domain_id))
        if local:
            env['RMW_IMPLEMENTATION'] = 'rmw_fastrtps_cpp'
            env['ROS_LOCALHOST_ONLY'] = '1'
            env['ROS_AUTOMATIC_DISCOVERY_RANGE'] = 'LOCALHOST'
            for key in ('ROS_DISCOVERY_SERVER', 'ROS_STATIC_PEERS',
                        'FASTDDS_DEFAULT_PROFILES_FILE', 'FASTRTPS_DEFAULT_PROFILES_FILE'):
                env.pop(key, None)
        return [shutil.which('ros2', path=env.get('PATH')) or 'ros2', *args], env

    def run(self, args, domain_id=0):
        command, env = self.command(args, domain_id)
        process = subprocess.run(command, env=env, capture_output=True, text=True, timeout=20)
        if process.returncode:
            raise RuntimeError((process.stderr or process.stdout)[-2500:])
        return process.stdout

    def check(self):
        self.env = load_environment()
        check = subprocess.run(['python3', '-c',
            'import json,rosbag2_py,yaml; print(json.dumps(sorted(rosbag2_py.get_registered_writers())))'],
            env=self.env, capture_output=True, text=True, timeout=15)
        if check.returncode or 'mcap' not in check.stdout:
            raise RuntimeError('The rosbag2 Python bindings or MCAP storage plugin are missing: ' + check.stderr[-1500:])
        self.run(['bag', 'record', '--help'])
        self.ready = True
        return {'ready': True, 'root': str(self.root), 'ros': 'Jazzy', 'storage': 'MCAP'}

    def session_path(self, name):
        if not isinstance(name, str) or not SESSION.fullmatch(name):
            raise ValueError('Invalid recording session identity.')
        path = self.root / name
        if path.is_symlink() or path.resolve().parent != self.root:
            raise ValueError('Session must stay inside the recording library.')
        return path

    def sessions(self):
        rows = []
        if not self.root.exists():
            return rows
        entries = sorted(self.root.iterdir(), key=lambda p: p.name, reverse=True)
        for path in entries[:500]:
            if not path.is_dir() or path.is_symlink() or not SESSION.fullmatch(path.name):
                continue
            try:
                manifest = path / 'session.json'
                if not manifest.is_file() or manifest.stat().st_size > 1_000_000:
                    continue
                row = json.loads(manifest.read_text())
                row.setdefault('label', path.name)
                row.setdefault('status', 'Unknown; inspect files')
                metadata = bag_metadata(path)
                row.update(id=path.name, path=str(path),
                    messages=metadata.get('message_count', 0),
                    duration=metadata.get('duration', {}).get('nanoseconds', 0) / 1e9,
                    bytes=sum(p.stat().st_size for p in (path / 'bag').glob('*.mcap') if not p.is_symlink()),
                    topics=[{'name': t['topic_metadata']['name'], 'type': t['topic_metadata']['type'],
                             'count': t.get('message_count', 0)}
                            for t in metadata.get('topics_with_message_count', [])])
                active = self.jobs.get('record')
                if row.get('status') == 'recording' and not (active and active['path'] == path):
                    row['status'] = 'Interrupted; inspect files'
                rows.append(row)
                if len(rows) == 100:
                    break
            except (OSError, ValueError, KeyError, TypeError) as error:
                emit({'kind': 'log', 'message': f'Skipped unreadable session {path.name}: {error}'})
        return rows

    def start_job(self, kind, args, domain_id, path=None, local=False):
        if self.jobs:
            raise RuntimeError('Stop the active recording or replay before starting another.')
        command, env = self.command(args, domain_id, local)
        child = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                 text=True, start_new_session=True)
        self.jobs[kind] = {'child': child, 'path': path}
        def output():
            for line in child.stdout:
                emit({'kind': 'log', 'message': line.strip()[:2500]})
        reader = threading.Thread(target=output, daemon=True)
        self.jobs[kind]['reader'] = reader
        reader.start()
        self.activity(f'{kind.capitalize()} process started; inspect its logs for subscriptions/data.')

    def record(self, request):
        if not self.ready:
            raise RuntimeError('Check recording dependencies first.')
        if self.jobs:
            raise RuntimeError('A recording or replay is already active.')
        selected = topics(request.get('topics'))
        label = request.get('name', 'flight')
        if not isinstance(label, str) or not re.fullmatch(r'[A-Za-z0-9_-]{1,48}', label):
            raise ValueError('Session name: use 1–48 letters, numbers, hyphens or underscores.')
        domain_id = domain(request.get('domain', 0))
        self.root.mkdir(parents=True, exist_ok=True)
        if shutil.disk_usage(self.root).free < 256 * 1024 * 1024:
            raise RuntimeError('Less than 256 MiB free in the recording filesystem.')
        stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
        path = self.session_path(f'{stamp}_{label}_{uuid.uuid4().hex[:8]}')
        path.mkdir()
        atomic_json(path / 'session.json', {
            'label': label, 'created': stamp, 'status': 'recording',
            'domain': domain_id, 'requested_topics': selected,
            'clock': 'ROS /clock' if request.get('sim_time') else 'system time',
            'ros': 'jazzy', 'storage': 'mcap',
            'source': str(request.get('source', 'Unspecified'))[:200],
            'wsl_distro': os.environ.get('WSL_DISTRO_NAME', ''),
        })
        args = ['bag', 'record', '-s', 'mcap', '-o', str(path / 'bag'),
                '--disable-keyboard-controls', '--max-cache-size', str(32 * 1024 ** 2),
                '--max-bag-size', str(1024 ** 3), '--topics', *selected]
        if request.get('sim_time'):
            args.append('--use-sim-time')
        try:
            self.start_job('record', args, domain_id, path)
        except Exception:
            self.finalize(path, -1)
            raise
        return {'id': path.name}

    def replay(self, request):
        if not self.ready:
            raise RuntimeError('Check recording dependencies first.')
        path = self.session_path(request.get('session'))
        metadata = bag_metadata(path)
        if metadata.get('storage_identifier') != 'mcap' or not metadata.get('message_count', 0):
            raise ValueError('Select a finalized, non-empty MCAP recording.')
        capture_domain = domain(request.get('capture_domain', 0))
        replay_domain = domain(request.get('domain', 77))
        original = json.loads((path / 'session.json').read_text())
        if replay_domain in (capture_domain, domain(original.get('domain', 0))):
            raise ValueError('Replay domain must differ from current and original capture domains.')
        rate = float(request.get('rate', 1))
        if not 0.1 <= rate <= 4 or not rate == rate:
            raise ValueError('Replay speed must be between 0.1x and 4x.')
        # Only topic playback; never replay service calls. Omit known control-input topics.
        selected = [t['topic_metadata']['name'] for t in metadata.get('topics_with_message_count', [])
                    if not t['topic_metadata']['name'].startswith('/fmu/in/')
                    and not any(p in t['topic_metadata']['name'].lower()
                                for p in ('vehicle_command', 'trajectory_setpoint', 'offboard_control_mode', 'cmd_vel'))]
        topics(selected)
        args = ['bag', 'play', str(path / 'bag'), '--rate', str(rate), '--clock',
                '--disable-keyboard-controls', '--topics', *selected]
        self.start_job('replay', args, replay_domain, path, local=True)
        return {'domain': replay_domain, 'topics': selected}

    def activity(self, message):
        emit({'kind': 'activity', 'recording': 'record' in self.jobs,
              'playing': 'replay' in self.jobs, 'message': message})

    def finalize(self, path, returncode):
        manifest = path / 'session.json'
        value = json.loads(manifest.read_text())
        metadata = bag_metadata(path)
        complete = (path / 'bag' / 'metadata.yaml').is_file() and metadata.get('storage_identifier') == 'mcap'
        value['status'] = 'Finalized' if complete and returncode in (0, -signal.SIGINT) else 'Incomplete; inspect logs'
        value['exit_code'] = returncode
        value['finished'] = datetime.now(timezone.utc).isoformat()
        atomic_json(manifest, value)

    def finish(self, kind, job, code):
        if kind == 'record':
            self.finalize(job['path'], code)
        job['reader'].join(timeout=2)
        if not job['reader'].is_alive():
            job['child'].stdout.close()
        self.activity(f'{kind.capitalize()} exited ({code}). Recording metadata checked.')
        emit({'kind': 'result', 'action': 'list', 'ok': True, 'data': self.sessions()})

    def tick(self):
        for kind, job in list(self.jobs.items()):
            code = job['child'].poll()
            if code is not None:
                del self.jobs[kind]
                self.finish(kind, job, code)

    def stop(self):
        for kind, job in list(self.jobs.items()):
            child = job['child']
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGINT)
                try:
                    child.wait(timeout=25)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGTERM)
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(child.pid, signal.SIGKILL)
                        child.wait(timeout=5)
            del self.jobs[kind]
            self.finish(kind, job, child.returncode)
        return {}

    def dispatch(self, request):
        action = request.get('action')
        if action == 'check':
            return self.check()
        if action == 'list':
            return self.sessions()
        if action == 'topics':
            text = self.run(['topic', 'list', '-t', '--no-daemon', '--spin-time', '2'], request.get('domain', 0))
            return [{'name': line.split(' [', 1)[0], 'type': line.split(' [', 1)[1].rstrip(']')}
                    for line in text.splitlines() if ' [' in line and TOPIC.fullmatch(line.split(' [', 1)[0])]
        if action == 'record':
            return self.record(request)
        if action == 'replay':
            return self.replay(request)
        if action == 'stop':
            return self.stop()
        raise ValueError('Unknown recording action.')

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', default='~/.local/share/drone-deck/recordings')
    args = parser.parse_args()
    server = Server(args.root)
    requests = queue.Queue(maxsize=100)
    def receive():
        try:
            for line in sys.stdin:
                requests.put(line)
        finally:
            requests.put(None)
    threading.Thread(target=receive, daemon=True).start()
    try:
        while True:
            try:
                line = requests.get(timeout=0.25)
            except queue.Empty:
                server.tick()
                continue
            if line is None:
                break
            action = 'unknown'
            try:
                request = json.loads(line)
                action = request.get('action')
                if action == 'shutdown':
                    break
                data = server.dispatch(request)
                emit({'kind': 'result', 'action': action, 'ok': True, 'data': data})
            except Exception as error:
                emit({'kind': 'result', 'action': action, 'ok': False, 'error': str(error)[:3000]})
            server.tick()
    finally:
        server.stop()

if __name__ == '__main__':
    main()
