"""pipe.* methods: frame extraction from MCAP recordings and ONNX inference on an isolated replay.

- `pipe.list` lists finalized recordings (with their topics and MCAP files) and the runs made from
  them: `<recording>/extract/<name>/manifest.json` and `<recording>/inference/<run>/run.json`.
- `pipe.extract{recording, topics, start?, end?, rate?, format: png|npz, name?}` writes
  `<recording>/extract/<name>/` with `index.csv` (timestamp_ns, topic, file) and `manifest.json`
  (source hashes, topics, counts, sha256 per file, tool versions). It runs this file as a worker
  process (`pipe.py extract`) with the ROS environment when one is installed, so either reader is
  importable: the `mcap` + `mcap-ros2-support` packages first, else ROS 2 `rosbag2_py`.
- `pipe.infer{recording, model, project?, topic, domain, rate?}` starts three owned processes: a
  rosbag2 recorder for `/drone_deck/detections`, the inference node (`infer_node.py`, or the
  project's own `infer_node.py`) and `ros2 bag play` of the image topic on an isolated,
  localhost-only domain. When the replay ends (or the request is cancelled) all three stop together
  and `run.json` records model hash, recording, topics, timings and exit states.
- `pipe.export{path, dest}` copies one recording file to a Windows folder (Foxglove fallback when a
  `\\\\wsl.localhost` path cannot be opened).
- `pipe.status` → `{active: [folders]}`: the extract and inference folders this agent is still
  writing (a cancelled run stays active until its processes have stopped), plus one `PREPARING`
  entry for each extract or infer request that has not created its folder yet.

Both long requests report `progress` events and honour cancel.
"""
from collections import deque
import contextlib
import csv
from datetime import datetime, timezone
import hashlib
import inspect
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import struct
import subprocess
import sys
import threading
import time
import uuid
import zlib

HERE = Path(__file__).resolve().parent
RECORDINGS = '~/.local/share/drone-deck/recordings'
PROJECTS = '~/.local/share/drone-deck/projects'
ONNX_PYTHON = '~/.local/share/drone-deck/tools/onnx/bin/python'
ROS_SETUP = '/opt/ros/jazzy/setup.bash'
VERSION = 1
SESSION = re.compile(r'^[A-Za-z0-9_-]{1,110}$')
NAME = re.compile(r'^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$')
TOPIC = re.compile(r'^/[A-Za-z_][A-Za-z0-9_]*(/[A-Za-z_][A-Za-z0-9_]*)*$')
DETECTIONS = '/drone_deck/detections'
IMAGE = 'sensor_msgs/msg/Image'
COMPRESSED = 'sensor_msgs/msg/CompressedImage'
KEYS = {'record': 'pipe-detections', 'node': 'pipe-infer', 'replay': 'pipe-replay'}
PREPARING = 'a request that has not created its folder yet'
FORMATS = ('png', 'npz')
# encoding → (channels, bits, swap red and blue)
ENCODINGS = {'rgb8': (3, 8, False), 'bgr8': (3, 8, True), 'rgba8': (4, 8, False),
             'bgra8': (4, 8, True), 'mono8': (1, 8, False), '8UC1': (1, 8, False),
             'mono16': (1, 16, False), '16UC1': (1, 16, False)}
MISSING_READER = ("Reading MCAP needs the Python packages 'mcap' and 'mcap-ros2-support' "
                  "(python3 -m pip install --user mcap mcap-ros2-support) or ROS 2 rosbag2_py "
                  "(ros-jazzy-rosbag2-py).")
LOCAL_ONLY = {'ROS_LOCALHOST_ONLY': '1', 'ROS_AUTOMATIC_DISCOVERY_RANGE': 'LOCALHOST',
              'RMW_IMPLEMENTATION': 'rmw_fastrtps_cpp'}


def now():
    return datetime.now(timezone.utc).isoformat()


def stamp():
    return datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')


def save_json(path, value):
    temporary = path.with_name(path.name + '.' + uuid.uuid4().hex[:8] + '.tmp')
    temporary.write_text(json.dumps(value, indent=2), encoding='utf-8')
    temporary.replace(path)


def read_json(path, limit=4_000_000):
    try:
        if path.stat().st_size > limit:
            return None
        value = json.loads(path.read_text(encoding='utf-8'))
    except (OSError, ValueError):
        return None
    return value if isinstance(value, dict) else None


def sha256_file(path, cancel=None):
    digest = hashlib.sha256()
    with open(path, 'rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            if cancel is not None and cancel():
                return None
            digest.update(chunk)
    return digest.hexdigest()


# -- images ------------------------------------------------------------------------------------
class Frame:
    """Decoded pixels: rows packed without padding, RGB(A) order, 16-bit samples big-endian (PNG)."""

    def __init__(self, width, height, channels, bits, pixels):
        self.width, self.height, self.channels, self.bits, self.pixels = width, height, channels, bits, pixels


def as_bytes(value):
    if isinstance(value, bytes):
        return value
    if isinstance(value, (bytearray, memoryview)):
        return bytes(value)
    if hasattr(value, 'tobytes'):
        return value.tobytes()
    return bytes(value)


def decode_image(encoding, height, width, step, data, bigendian=False):
    """sensor_msgs/Image fields → Frame. Raises ValueError for unsupported or short images."""
    if encoding not in ENCODINGS:
        raise ValueError(f'Unsupported image encoding {encoding!r}.')
    channels, bits, swap = ENCODINGS[encoding]
    height, width, step = int(height), int(width), int(step)
    if height <= 0 or width <= 0:
        raise ValueError('Image has no pixels.')
    row = width * channels * bits // 8
    data = as_bytes(data)
    if step < row or len(data) < step * (height - 1) + row:
        raise ValueError('Image data is shorter than its height and step.')
    if step == row:
        pixels = bytearray(data[:row * height])
    else:
        pixels = bytearray().join(data[r * step:r * step + row] for r in range(height))
    if swap:
        pixels[0::channels], pixels[2::channels] = pixels[2::channels], pixels[0::channels]
    if bits == 16 and not bigendian:
        pixels[0::2], pixels[1::2] = pixels[1::2], pixels[0::2]
    return Frame(width, height, channels, bits, bytes(pixels))


def png_bytes(frame):
    """A PNG file for the frame (no filtering; zlib level 6)."""
    color = {1: 0, 3: 2, 4: 6}[frame.channels]
    row = frame.width * frame.channels * frame.bits // 8
    raw = b''.join(b'\x00' + frame.pixels[r * row:(r + 1) * row] for r in range(frame.height))

    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body) & 0xffffffff)
    header = struct.pack('>IIBBBBB', frame.width, frame.height, frame.bits, color, 0, 0, 0)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header) + chunk(b'IDAT', zlib.compress(raw, 6)) + chunk(b'IEND', b'')


def frame_array(frame, numpy):
    """Frame → numpy array (H, W) or (H, W, C), uint8 or uint16."""
    dtype = numpy.dtype('>u2') if frame.bits == 16 else numpy.uint8
    array = numpy.frombuffer(frame.pixels, dtype=dtype).reshape(frame.height, frame.width, frame.channels)
    if frame.bits == 16:
        array = array.astype(numpy.uint16)
    return array[:, :, 0] if frame.channels == 1 else array


def compressed_extension(fmt):
    text = str(fmt or '').lower()
    return 'png' if 'png' in text else 'jpg'


def plain(value, depth=0):
    """A ROS message (mcap_ros2 or rclpy) as JSON-ready data."""
    if depth > 32:
        return None
    if value is None or isinstance(value, (bool, int, str)):
        return value
    if isinstance(value, float):
        return value if value == value and abs(value) != float('inf') else str(value)
    if isinstance(value, (bytes, bytearray, memoryview)):
        return list(bytes(value))
    if hasattr(value, 'tolist'):
        return value.tolist()
    if isinstance(value, (list, tuple)) or type(value).__name__ == 'array':
        return [plain(item, depth + 1) for item in value]
    if hasattr(value, 'get_fields_and_field_types'):
        return {name: plain(getattr(value, name), depth + 1) for name in value.get_fields_and_field_types()}
    slots = []
    for kind in type(value).__mro__:
        slots.extend(getattr(kind, '__slots__', ()) or ())
    if slots:
        return {name.lstrip('_'): plain(getattr(value, name), depth + 1)
                for name in slots if not name.startswith('__') and hasattr(value, name)}
    if hasattr(value, '__dict__'):
        return {k: plain(v, depth + 1) for k, v in vars(value).items() if not k.startswith('_')}
    return str(value)


# -- reading MCAP -----------------------------------------------------------------------------
def module_version(name):
    try:
        from importlib.metadata import version
        return version(name)
    except Exception:  # noqa: BLE001 - optional detail in the manifest
        return None


def mcap_messages(files, topics, start, end):
    """(reader name, versions, start_ns, iterator of (topic, type, log_time_ns, message))."""
    from mcap.reader import make_reader
    from mcap_ros2.decoder import DecoderFactory
    base = None
    for path in files:
        with open(path, 'rb') as stream:
            summary = make_reader(stream).get_summary()
            if summary is not None and summary.statistics is not None and summary.statistics.message_count:
                first = summary.statistics.message_start_time
                base = first if base is None else min(base, first)
    base = base or 0
    first = base + int(start * 1e9) if start is not None else None
    last = base + int(end * 1e9) if end is not None else None

    def iterate():
        for path in files:
            with open(path, 'rb') as stream:
                reader = make_reader(stream, decoder_factories=[DecoderFactory()])
                if 'ending_at' in inspect.signature(reader.iter_decoded_messages).parameters:
                    found = reader.iter_decoded_messages(topics=topics, starting_at=first, ending_at=last)
                else:  # mcap before 1.2
                    found = reader.iter_decoded_messages(
                        topics=topics, start_time=first, end_time=None if last is None else last + 1)
                for schema, channel, message, decoded in found:
                    yield channel.topic, schema.name if schema else '', message.log_time, decoded
    versions = {'reader': 'mcap', 'mcap': module_version('mcap'),
                'mcap_ros2_support': module_version('mcap-ros2-support')}
    return versions, base, iterate()


def rosbag2_messages(bag, topics, start, end):
    import rosbag2_py
    from rclpy.serialization import deserialize_message
    from rosidl_runtime_py.utilities import get_message
    reader = rosbag2_py.SequentialReader()
    reader.open(rosbag2_py.StorageOptions(uri=str(bag), storage_id='mcap'),
                rosbag2_py.ConverterOptions('cdr', 'cdr'))
    types = {t.name: t.type for t in reader.get_all_topics_and_types()}
    reader.set_filter(rosbag2_py.StorageFilter(topics=list(topics)))
    base = reader.get_metadata().starting_time.nanoseconds
    first = base + int(start * 1e9) if start is not None else None
    last = base + int(end * 1e9) if end is not None else None
    if first is not None:
        reader.seek(first)

    def iterate():
        classes = {}
        while reader.has_next():
            topic, data, when = reader.read_next()
            if last is not None and when > last:
                break
            kind = types.get(topic, '')
            if kind not in classes:
                classes[kind] = get_message(kind)
            yield topic, kind, when, deserialize_message(data, classes[kind])
    return {'reader': 'rosbag2_py', 'ros': os.environ.get('ROS_DISTRO')}, base, iterate()


def open_messages(recording, topics, start, end):
    files = sorted((recording / 'bag').glob('*.mcap'))
    if not files:
        raise ValueError('This recording has no MCAP files.')
    try:
        return files, mcap_messages(files, topics, start, end)
    except ImportError as mcap_error:
        try:
            return files, rosbag2_messages(recording / 'bag', topics, start, end)
        except ImportError:
            raise RuntimeError(f'{MISSING_READER} ({mcap_error})') from None


# -- extraction -------------------------------------------------------------------------------
def slug(topic):
    return re.sub(r'[^A-Za-z0-9]+', '_', topic.strip('/')).strip('_') or 'root'


class Extractor:
    """Writes kept messages and `index.csv`; `finish` writes `manifest.json`."""

    def __init__(self, folder, fmt, rate):
        self.folder = folder
        self.format = fmt
        self.interval = int(1e9 / rate) if rate else 0
        self.last = {}
        self.topics = {}
        self.files = {}
        self.names = set()
        self.numpy = None
        self.skipped = {}
        self.index_stream = open(folder / 'index.csv', 'w', newline='', encoding='utf-8')
        self.index = csv.writer(self.index_stream)
        self.index.writerow(['timestamp_ns', 'topic', 'file'])
        self.rows = {}

    def _numpy(self):
        if self.numpy is None:
            try:
                import numpy
            except ImportError:
                raise RuntimeError("NPZ output needs numpy (python3 -m pip install --user numpy).") from None
            self.numpy = numpy
        return self.numpy

    def _name(self, topic, when, extension):
        base = f'{slug(topic)}/{when}'
        name, n = f'{base}.{extension}', 1
        while name in self.names:
            name, n = f'{base}_{n}.{extension}', n + 1
        self.names.add(name)
        return name

    def _write(self, name, data):
        path = self.folder / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        self.files[name] = {'file': name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}

    def _npz(self, arrays):
        import io
        numpy = self._numpy()
        buffer = io.BytesIO()
        numpy.savez_compressed(buffer, **arrays)
        return buffer.getvalue()

    def _skip(self, topic, reason):
        self.skipped.setdefault(topic, {'count': 0, 'reason': reason})['count'] += 1

    def add(self, topic, kind, when, message):
        """Writes one message unless the rate limit drops it. Returns whether it was kept."""
        if self.interval and topic in self.last and when - self.last[topic] < self.interval:
            return False
        self.last[topic] = when
        info = self.topics.setdefault(topic, {'type': kind, 'count': 0})
        if kind == IMAGE:
            try:
                frame = decode_image(message.encoding, message.height, message.width, message.step,
                                     message.data, bool(message.is_bigendian))
            except ValueError as error:
                self._skip(topic, str(error))
                return False
            if self.format == 'png':
                name, data = self._name(topic, when, 'png'), png_bytes(frame)
            else:
                name = self._name(topic, when, 'npz')
                data = self._npz({'image': frame_array(frame, self._numpy())})
        elif kind == COMPRESSED:
            raw = as_bytes(message.data)
            if self.format == 'png':
                name, data = self._name(topic, when, compressed_extension(message.format)), raw
            else:
                name = self._name(topic, when, 'npz')
                numpy = self._numpy()
                data = self._npz({'data': numpy.frombuffer(raw, numpy.uint8),
                                  'format': numpy.array(str(message.format))})
        else:
            file = f'{slug(topic)}.jsonl'
            line = self.rows.get(file, 0)
            with open(self.folder / file, 'a', encoding='utf-8') as stream:
                stream.write(json.dumps({'timestamp_ns': when, 'message': plain(message)}) + '\n')
            self.rows[file] = line + 1
            self.names.add(file)
            name = f'{file}#{line}'
        if kind in (IMAGE, COMPRESSED):
            self._write(name, data)
        self.index.writerow([when, topic, name])
        info['count'] += 1
        return True

    def finish(self):
        self.index_stream.close()
        for file in self.rows:
            path = self.folder / file
            self.files[file] = {'file': file, 'bytes': path.stat().st_size, 'sha256': sha256_file(path)}
        index = self.folder / 'index.csv'
        self.files['index.csv'] = {'file': 'index.csv', 'bytes': index.stat().st_size, 'sha256': sha256_file(index)}
        return sorted(self.files.values(), key=lambda f: f['file'])


def extract(request, progress, cancel):
    """The worker: reads the recording and writes the extract folder. Returns the manifest."""
    recording = Path(request['recording_path'])
    folder = Path(request['folder'])
    topics = request['topics']
    start, end, rate, fmt = request.get('start'), request.get('end'), request.get('rate'), request['format']
    files, (versions, base, messages) = open_messages(recording, topics, start, end)
    folder.mkdir(parents=True)
    manifest = {'schema': 1, 'kind': 'extract', 'name': folder.name, 'recording': recording.name,
                'status': 'Running', 'created': now(),
                'params': {'topics': topics, 'start_s': start, 'end_s': end, 'rate_hz': rate, 'format': fmt}}
    save_json(folder / 'manifest.json', manifest)
    sources = []
    for path in files:
        progress(f'Hashing source {path.name}')
        digest = sha256_file(path, cancel)
        sources.append({'file': str(path), 'bytes': path.stat().st_size, 'sha256': digest})
    extractor = Extractor(folder, fmt, rate)
    status, seen, last_report, failure = 'Completed', 0, time.monotonic(), None
    try:
        for topic, kind, when, message in messages:
            if cancel():
                status = 'Cancelled'
                break
            seen += 1
            extractor.add(topic, kind, when, message)
            if time.monotonic() - last_report > 0.5:
                last_report = time.monotonic()
                kept = sum(t['count'] for t in extractor.topics.values())
                progress(f'Extracted {kept} of {seen} messages read')
        if cancel():
            status = 'Cancelled'
    except Exception as error:  # noqa: BLE001 - recorded in the manifest, then re-raised
        status, failure = 'Failed', error
    written = extractor.finish()
    tools = {'python': platform.python_version(), 'pipe': VERSION, **versions}
    if extractor.numpy is not None:
        tools['numpy'] = extractor.numpy.__version__
    for topic in topics:
        extractor.topics.setdefault(topic, {'type': None, 'count': 0})
    manifest.update(status=status, finished=now(), start_ns=base,
                    source={'files': sources}, topics=extractor.topics, skipped=extractor.skipped,
                    count=sum(t['count'] for t in extractor.topics.values()), messages_read=seen,
                    files=written, tools=tools, path=str(folder))
    if failure is not None:
        manifest['error'] = str(failure) or type(failure).__name__
    save_json(folder / 'manifest.json', manifest)
    if failure is not None:
        raise failure
    return manifest


def worker():
    """`pipe.py extract`: one JSON request line on stdin; `cancel` (or EOF) on stdin cancels.
    Writes `{"progress"}` lines and one `{"result"}` or `{"error"}` line on stdout."""
    lock = threading.Lock()
    for stream in (sys.stdin, sys.stdout):
        stream.reconfigure(encoding='utf-8')

    def send(value):
        with lock:
            sys.stdout.write(json.dumps(value, default=str) + '\n')
            sys.stdout.flush()
    request = json.loads(sys.stdin.readline())
    cancelled = threading.Event()

    def watch():
        for line in sys.stdin:
            if line.strip() == 'cancel':
                break
        cancelled.set()
    threading.Thread(target=watch, daemon=True).start()
    try:
        send({'result': extract(request, lambda m: send({'progress': m}), cancelled.is_set)})
    except Exception as error:  # noqa: BLE001 - reported to the agent
        send({'error': str(error) or type(error).__name__})


# -- requests (agent side) --------------------------------------------------------------------
def recording_path(root, value):
    if not isinstance(value, str) or not SESSION.fullmatch(value):
        raise ValueError('Select a recording.')
    path = root / value
    if path.is_symlink() or path.resolve().parent != root.resolve():
        raise ValueError('The recording must stay inside the recording library.')
    if not path.is_dir():
        raise ValueError(f'Recording {value} does not exist.')
    session = read_json(path / 'session.json') or {}
    if str(session.get('status', '')).lower() != 'finalized':
        raise ValueError('Stop and finalize this recording first.')
    return path, session


def seconds(value, label):
    if value is None or value == '':
        return None
    number = float(value)
    if not number >= 0 or number == float('inf'):
        raise ValueError(f'{label} must be zero or more seconds.')
    return number


def extract_request(params, root):
    """Validates `pipe.extract` parameters into the worker request."""
    recording, _ = recording_path(root, params.get('recording'))
    topics = params.get('topics')
    if not isinstance(topics, list) or not 1 <= len(topics) <= 64 \
            or any(not isinstance(t, str) or not TOPIC.fullmatch(t) for t in topics):
        raise ValueError('Choose 1–64 absolute topic names, for example /camera/image_raw.')
    fmt = params.get('format', 'png')
    if fmt not in FORMATS:
        raise ValueError('Format must be png or npz.')
    start, end = seconds(params.get('start'), 'Start'), seconds(params.get('end'), 'End')
    if start is not None and end is not None and end <= start:
        raise ValueError('End must be after start.')
    rate = params.get('rate')
    if rate in (None, '', 0):
        rate = None
    else:
        rate = float(rate)
        if not 0 < rate <= 1000:
            raise ValueError('Rate must be between 0 and 1000 frames per second.')
    name = params.get('name') or stamp()
    if not isinstance(name, str) or not NAME.fullmatch(name):
        raise ValueError('Name: use 1–64 letters, numbers, hyphens or underscores.')
    folder = recording / 'extract' / name
    if folder.exists():
        raise ValueError(f'An extract named {name} already exists for this recording.')
    return {'recording_path': str(recording), 'folder': str(folder), 'topics': list(dict.fromkeys(topics)),
            'start': start, 'end': end, 'rate': rate, 'format': fmt}


def ros_environment(setup=ROS_SETUP):
    """The ROS environment when ROS is installed, else this process's environment."""
    env = dict(os.environ)
    if not Path(setup).is_file():
        return env
    try:
        result = subprocess.run(['bash', '-c', 'source "$1" && env -0', 'drone-deck', setup],
                                capture_output=True, timeout=20, check=True)
    except (OSError, subprocess.SubprocessError):
        return env
    for item in result.stdout.split(b'\0'):
        if b'=' in item:
            key, value = item.split(b'=', 1)
            env[key.decode(errors='replace')] = value.decode(errors='replace')
    return env


def ros_command(args, setup=ROS_SETUP):
    """`bash -lc` text that sources ROS, drops discovery overrides and execs `args`."""
    unset = 'unset ROS_DISCOVERY_SERVER ROS_STATIC_PEERS FASTDDS_DEFAULT_PROFILES_FILE FASTRTPS_DEFAULT_PROFILES_FILE'
    return f'source {shlex.quote(setup)} && {unset} && exec {shlex.join(args)}'


def bag_topics(recording, metadata):
    """{topic: type} from rosbag2 metadata."""
    rows = metadata.get('topics_with_message_count', []) if isinstance(metadata, dict) else []
    found = {}
    for row in rows:
        try:
            found[row['topic_metadata']['name']] = row['topic_metadata']['type']
        except (KeyError, TypeError):
            continue
    return found


def infer_plan(params, root, metadata, projects, template=HERE / 'infer_node.py', python=None):
    """Validates `pipe.infer` parameters and builds the three commands (no process starts).
    `metadata(recording_path)` returns the recording's rosbag2 metadata."""
    recording, session = recording_path(root, params.get('recording'))
    topic = params.get('topic')
    if not isinstance(topic, str) or not TOPIC.fullmatch(topic):
        raise ValueError('Choose the image topic to replay, for example /camera/image_raw.')
    types = bag_topics(recording, metadata(recording))
    if topic not in types:
        raise ValueError(f'{topic} is not in this recording.')
    if types[topic] not in (IMAGE, COMPRESSED):
        raise ValueError(f'{topic} is {types[topic]}; inference needs sensor_msgs Image or CompressedImage.')
    model = Path(os.path.expanduser(str(params.get('model') or '')))
    if not str(params.get('model') or '').strip() or model.suffix.lower() != '.onnx' or not model.is_file():
        raise ValueError('Choose an existing .onnx model file inside the Linux environment.')
    try:
        domain = int(params.get('domain', 78))
    except (TypeError, ValueError):
        raise ValueError('Replay domain must be a number.') from None
    if not 0 <= domain <= 232:
        raise ValueError('Replay domain must be between 0 and 232.')
    if domain == domain_of(session):
        raise ValueError('Use a replay domain different from the domain the recording was captured on.')
    rate = float(params.get('rate') or 1.0)
    if not 0.1 <= rate <= 4:
        raise ValueError('Replay speed must be between 0.1x and 4x.')
    project = params.get('project') or None
    node = Path(template)
    if project is not None:
        if not isinstance(project, str) or not NAME.fullmatch(project):
            raise ValueError('Project: use the project name.')
        own = projects / project / 'infer_node.py'
        if own.is_file():
            node = own
    run = f'{stamp()}_{uuid.uuid4().hex[:8]}'
    folder = recording / 'inference' / run
    python = python or (os.path.expanduser(ONNX_PYTHON) if Path(os.path.expanduser(ONNX_PYTHON)).is_file()
                        else 'python3')
    commands = {
        'record': ['ros2', 'bag', 'record', '-s', 'mcap', '-o', str(folder / 'bag'),
                   '--disable-keyboard-controls', '--topics', DETECTIONS],
        'node': [python, '-u', str(node), '--model', str(model), '--topic', topic, '--type', types[topic],
                 '--output', DETECTIONS],
        'replay': ['ros2', 'bag', 'play', str(recording / 'bag'), '--rate', str(rate), '--clock',
                   '--disable-keyboard-controls', '--topics', topic],
    }
    env = dict(LOCAL_ONLY, ROS_DOMAIN_ID=str(domain), DRONE_DECK_AGENT=str(HERE))
    return {'run': run, 'folder': folder, 'recording': recording, 'model': model, 'node': node,
            'topic': topic, 'type': types[topic], 'domain': domain, 'rate': rate, 'project': project,
            'commands': commands, 'env': env}


def bag_metadata_counts(metadata):
    """{topic: message count} from rosbag2 metadata."""
    rows = metadata.get('topics_with_message_count', []) if isinstance(metadata, dict) else []
    counts = {}
    for row in rows:
        try:
            counts[row['topic_metadata']['name']] = int(row.get('message_count', 0))
        except (KeyError, TypeError, ValueError):
            continue
    return counts


def text(value):
    """A manifest string field, or None when it is missing or not a string."""
    return value if isinstance(value, str) else None


def count(value):
    """A manifest count, or None when it is missing or not a whole number of zero or more."""
    return value if isinstance(value, int) and not isinstance(value, bool) and value >= 0 else None


def domain_of(session):
    try:
        return int(session.get('domain', 0) or 0)
    except (TypeError, ValueError):
        return 0


def mcap_paths(folder):
    bag = folder / 'bag'
    try:
        return [str(p) for p in sorted(bag.glob('*.mcap')) if not p.is_symlink()][:32]
    except OSError:
        return []


class Pipelines:
    """The agent's pipe.* methods. `processes` is the agent's `procs.Processes`."""

    SETTLE_S = 3.0
    DRAIN_S = 2.0
    # A cancelled extraction worker checks between messages and hash chunks; one stuck inside a
    # reader call (a damaged bag, a slow 9P read) is killed after this long.
    CANCEL_GRACE_S = 10.0

    def __init__(self, out, processes, root=RECORDINGS, projects=PROJECTS, metadata=None,
                 python=(sys.executable,), environment=ros_environment, mounts='/mnt/'):
        self.out = out
        self.procs = processes
        self.root = Path(os.path.expanduser(root))
        self.projects = Path(os.path.expanduser(projects))
        self._metadata = metadata
        self.python = list(python)
        self.environment = environment
        self.mounts = mounts
        self.lock = threading.Lock()
        # Extract and inference folders being written now; `list` reports other Running ones
        # as Interrupted.
        self.active = set()
        # extract and infer requests from their start until they return: a cancel that arrives
        # before the request has created its folder still has something to wait for.
        self.requests = {}
        self._recordings = None

    def metadata(self, recording):
        if self._metadata is not None:
            return self._metadata(recording)
        if self._recordings is None:
            import services
            self._recordings = services.load('drone_deck_recordings_pipe', 'recordings.py')
        try:
            return self._recordings.bag_metadata(recording)
        except ImportError:
            return {}

    def call(self, name, params, cancel, progress):
        table = {'list': lambda: self.list(),
                 'extract': lambda: self.extract(params, cancel, progress),
                 'infer': lambda: self.infer(params, cancel, progress),
                 'export': lambda: self.export(params),
                 'status': self.status}
        if name not in table:
            raise ValueError(f'Unknown pipeline method pipe.{name}.')
        if name not in ('extract', 'infer'):
            return table[name]()
        token = object()
        self.requests[token] = name
        try:
            return table[name]()
        finally:
            self.requests.pop(token, None)

    def status(self):
        """Folders being written, then one entry per request that has not created its folder."""
        active = sorted(str(p) for p in set(self.active))
        preparing = max(0, len(self.requests) - len(active))
        return {'active': active + [PREPARING] * preparing}

    # -- list -------------------------------------------------------------------------------
    def list(self):
        recordings, runs = [], []
        if not self.root.is_dir():
            return {'recordings': recordings, 'runs': runs, 'root': str(self.root)}
        for path in sorted(self.root.iterdir(), key=lambda p: p.name, reverse=True)[:300]:
            if not path.is_dir() or path.is_symlink() or not SESSION.fullmatch(path.name):
                continue
            session = read_json(path / 'session.json')
            if session is None or str(session.get('status', '')).lower() != 'finalized':
                continue
            types = bag_topics(path, self.metadata(path))
            recordings.append({'id': path.name, 'label': str(session.get('label') or path.name),
                               'path': str(path), 'domain': domain_of(session),
                               'topics': [{'name': k, 'type': v} for k, v in sorted(types.items())],
                               'mcap': mcap_paths(path)})
            runs.extend(self._runs(path))
            if len(recordings) >= 100:
                break
        runs.sort(key=lambda r: str(r.get('created') or ''), reverse=True)
        return {'recordings': recordings, 'runs': runs[:200], 'root': str(self.root)}

    def _runs(self, recording):
        rows = []
        for kind, folder, manifest in (('extract', 'extract', 'manifest.json'),
                                       ('inference', 'inference', 'run.json')):
            base = recording / folder
            if not base.is_dir():
                continue
            for path in sorted(base.iterdir())[-50:]:
                value = read_json(path / manifest) if path.is_dir() else None
                if value is None:
                    continue
                status = text(value.get('status')) or 'Unknown'
                if status == 'Running' and path not in self.active:
                    status = 'Interrupted'
                row = {'kind': kind, 'recording': recording.name, 'name': path.name, 'path': str(path),
                       'status': status, 'created': text(value.get('created')),
                       'finished': text(value.get('finished'))}
                if kind == 'extract':
                    params = value.get('params') if isinstance(value.get('params'), dict) else {}
                    topics = value.get('topics') if isinstance(value.get('topics'), dict) else {}
                    row.update(count=count(value.get('count')) or 0, format=text(params.get('format')),
                               topics=sorted(str(k) for k in topics))
                else:
                    row.update(model=text(value.get('model')), topic=text(value.get('topic')),
                               detections=count(value.get('detections')), mcap=mcap_paths(path))
                rows.append(row)
        return rows

    # -- extract ----------------------------------------------------------------------------
    def extract(self, params, cancel, progress):
        request = extract_request(params or {}, self.root)
        folder = Path(request['folder'])
        self.active.add(folder)
        try:
            return self._extract(request, cancel, progress)
        finally:
            self.active.discard(folder)

    def _extract(self, request, cancel, progress):
        progress(f'Extracting {len(request["topics"])} topic(s) from {Path(request["recording_path"]).name}')
        child = subprocess.Popen([*self.python, '-u', str(HERE / 'pipe.py'), 'extract'],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 text=True, encoding='utf-8', env=self.environment())
        child.stdin.write(json.dumps(request) + '\n')
        child.stdin.flush()
        errors = deque(maxlen=20)
        threading.Thread(target=lambda: errors.extend(line[-500:] for line in child.stderr),
                         daemon=True).start()
        lines = []
        reader = threading.Thread(target=lambda: lines.extend(self._worker_lines(child, progress)), daemon=True)
        reader.start()
        deadline = None  # set when 'cancel' was sent
        killed = False
        while reader.is_alive():
            reader.join(0.2)
            if cancel.is_set() and deadline is None:
                deadline = time.monotonic() + self.CANCEL_GRACE_S
                try:
                    child.stdin.write('cancel\n')
                    child.stdin.flush()
                except OSError:
                    pass
            elif deadline is not None and not killed and time.monotonic() > deadline:
                # The reader then sees EOF and the loop ends.
                killed = True
                child.kill()
        try:
            child.stdin.close()
        except OSError:
            pass
        try:
            child.wait(timeout=30)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
        for stream in (child.stdout, child.stderr):
            stream.close()
        for value in lines:
            if 'result' in value:
                return value['result']
            if 'error' in value:
                raise RuntimeError(value['error'])
        if deadline is not None:
            self._mark_cancelled(Path(request['folder']) / 'manifest.json')
            raise RuntimeError(f'Cancelled. The extraction worker did not stop within '
                               f'{self.CANCEL_GRACE_S:g} s and was ended; partial output is kept.')
        raise RuntimeError('The extraction worker ended without a result. ' + ''.join(list(errors)[-5:]).strip())

    @staticmethod
    def _mark_cancelled(path):
        """A worker ended after a cancel can leave its manifest Running: record it as Cancelled."""
        manifest = read_json(path)
        if manifest is not None and manifest.get('status') == 'Running':
            manifest.update(status='Cancelled', finished=now(),
                            error='The worker did not stop after the cancel and was ended.')
            save_json(path, manifest)

    @staticmethod
    def _worker_lines(child, progress):
        for line in child.stdout:
            try:
                value = json.loads(line)
            except ValueError:
                continue
            if 'progress' in value:
                progress(str(value['progress']))
            else:
                yield value

    # -- infer ------------------------------------------------------------------------------
    def infer(self, params, cancel, progress):
        plan = infer_plan(params or {}, self.root, self.metadata, self.projects)
        if not self.lock.acquire(blocking=False):
            raise RuntimeError('An inference run is already active. Cancel it first.')
        try:
            return self._infer(plan, cancel, progress)
        finally:
            self.active.discard(plan['folder'])
            self.lock.release()

    def _start(self, part, plan):
        # A process that exits within milliseconds is finished, and dropped from `entries`, by
        # its waiter thread, which needs the table's (reentrant) lock to do so. Holding it
        # until the entry is in hand keeps the entry, and with it the real exit code.
        with getattr(self.procs, 'lock', None) or contextlib.nullcontext():
            self.procs.run(KEYS[part], ros_command(plan['commands'][part]), plan['env'])
            return getattr(self.procs, 'entries', {}).get(KEYS[part])

    @staticmethod
    def _exit_code(entry):
        child = getattr(entry, 'child', None)
        return None if child is None else child.returncode

    @staticmethod
    def _running(entry):
        return entry is not None and not entry.finished.is_set()

    def _infer(self, plan, cancel, progress):
        folder = plan['folder']
        folder.mkdir(parents=True)
        self.active.add(folder)
        progress('Hashing the model')
        run = {'schema': 1, 'kind': 'inference', 'run': plan['run'], 'recording': plan['recording'].name,
               'status': 'Running', 'created': now(), 'model': str(plan['model']),
               'model_sha256': sha256_file(plan['model']), 'node': str(plan['node']),
               'node_sha256': sha256_file(plan['node']), 'project': plan['project'],
               'topic': plan['topic'], 'topic_type': plan['type'], 'output_topic': DETECTIONS,
               'topics': [plan['topic'], DETECTIONS], 'domain': plan['domain'], 'rate': plan['rate'],
               'commands': {k: shlex.join(v) for k, v in plan['commands'].items()},
               'timings': {}, 'exits': {}}
        save_json(folder / 'run.json', run)
        timings, entries, failure = run['timings'], {}, None
        started = time.monotonic()

        def mark(name):
            timings[name] = round(time.monotonic() - started, 3)
        try:
            for part, message in (('record', 'Starting the detections recorder'),
                                  ('node', 'Starting the inference node')):
                if cancel.is_set():
                    break
                progress(message)
                entries[part] = self._start(part, plan)
                mark(f'{part}_started_s')
            deadline = time.monotonic() + self.SETTLE_S
            while time.monotonic() < deadline and not cancel.is_set():
                if not all(self._running(entries.get(p)) for p in ('record', 'node')):
                    failure = 'The inference node or recorder exited before the replay started. See its output.'
                    break
                time.sleep(0.1)
            if failure is None and not cancel.is_set() \
                    and not all(self._running(entries.get(p)) for p in ('record', 'node')):
                failure = 'The inference node or recorder exited before the replay started. See its output.'
            if failure is None and not cancel.is_set():
                progress(f'Replaying {plan["topic"]} on local-only domain {plan["domain"]}')
                entries['replay'] = self._start('replay', plan)
                mark('replay_started_s')
                while self._running(entries['replay']) and not cancel.is_set():
                    if not self._running(entries['node']):
                        failure = 'The inference node exited during the replay. See its output.'
                        break
                    time.sleep(0.2)
                mark('replay_finished_s')
                if failure is None and not cancel.is_set():
                    progress('Replay finished; collecting the last detections')
                    cancel.wait(self.DRAIN_S)
        except Exception as error:  # noqa: BLE001 - recorded in run.json, then re-raised
            failure = str(error) or type(error).__name__
        finally:
            progress('Stopping the run processes')
            errors = []

            def stop(part, grace):
                try:
                    self.procs.stop(KEYS[part], grace)
                except Exception as error:  # noqa: BLE001 - keep stopping the others
                    errors.append(f'Stopping {part}: {error}')
                entry = entries[part]
                if entry is not None:
                    entry.finished.wait(5)
                run['exits'][part] = self._exit_code(entry)
            parts = [(p, g) for p, g in (('replay', 5), ('node', 5), ('record', 20)) if p in entries]
            if cancel.is_set():
                # Cancel (or agent shutdown, which waits only briefly) signals all three at once,
                # so none is left running when the agent exits before the recorder finalizes.
                stoppers = [threading.Thread(target=stop, args=part, daemon=True) for part in parts]
                for thread in stoppers:
                    thread.start()
                for thread in stoppers:
                    thread.join()
            else:
                # In order: the recorder keeps the detections published while the others stop.
                for part in parts:
                    stop(*part)
            failure = failure or (errors[0] if errors else None)
            mark('finished_s')
            run['status'] = 'Failed' if failure else 'Cancelled' if cancel.is_set() else 'Completed'
            if failure:
                run['error'] = failure
            run['finished'] = now()
            run['outputs'] = mcap_paths(folder)
            counts = bag_metadata_counts(self.metadata(folder))
            run['detections'] = counts.get(DETECTIONS)
            save_json(folder / 'run.json', run)
        if failure:
            raise RuntimeError(f'{failure} Run record: {folder / "run.json"}')
        return run

    # -- export -----------------------------------------------------------------------------
    def export(self, params):
        source = Path(str((params or {}).get('path') or ''))
        dest = Path(str((params or {}).get('dest') or ''))
        try:
            inside = source.resolve().is_relative_to(self.root.resolve())
            # `..` or a symlink must not lead a "Windows folder" back into Linux.
            dest = dest.resolve()
        except (OSError, ValueError, RuntimeError):
            inside = False
        if not inside or not source.is_file() or source.suffix.lower() != '.mcap':
            raise ValueError('Choose an MCAP file inside the recording library.')
        if not dest.as_posix().startswith(self.mounts.rstrip('/') + '/') or not dest.is_dir():
            raise ValueError('The export folder must be an existing Windows folder (/mnt/<drive>/…).')
        target = dest / f'{source.parent.parent.name}-{source.name}'
        shutil.copyfile(source, target)
        return {'path': str(target), 'bytes': target.stat().st_size}


if __name__ == '__main__' and sys.argv[1:] == ['extract']:
    worker()
