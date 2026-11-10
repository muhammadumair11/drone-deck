"""pipe.* without ROS: image decoding, PNG writing, extraction from generated MCAP fixtures, the
extraction worker, inference orchestration with fake processes, and the template node's
processing functions. MCAP tests need the `mcap` and `mcap-ros2-support` packages and skip with
that reason when they are missing; NPZ and node tests need numpy."""
import csv
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct
import sys
import tempfile
import threading
import time
import types
import unittest
from unittest.mock import patch
import zlib

import agent
import channel
import pipe

HAS_MCAP = all(importlib.util.find_spec(name) is not None for name in ('mcap', 'mcap_ros2'))
HAS_NUMPY = importlib.util.find_spec('numpy') is not None
NO_MCAP = "needs the 'mcap' and 'mcap-ros2-support' packages (pip install mcap mcap-ros2-support)"
NO_NUMPY = 'needs numpy (pip install numpy)'

TIME = 'MSG: builtin_interfaces/Time\nint32 sec\nuint32 nanosec\n'
HEADER = 'MSG: std_msgs/Header\nbuiltin_interfaces/Time stamp\nstring frame_id\n'
SEP = '=' * 80 + '\n'
IMAGE_DEF = ('std_msgs/Header header\nuint32 height\nuint32 width\nstring encoding\nuint8 is_bigendian\n'
             'uint32 step\nuint8[] data\n' + SEP + HEADER + SEP + TIME)
COMPRESSED_DEF = 'std_msgs/Header header\nstring format\nuint8[] data\n' + SEP + HEADER + SEP + TIME
STRING_DEF = 'string data\n'
SESSION_ID = '20261110T000000Z_flight_12345678'
BASE_NS = 1_700_000_000_000_000_000


def png_pixels(data):
    """(width, height, bits, color, raw rows without filter bytes) of an unfiltered PNG."""
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    offset, chunks = 8, {}
    while offset < len(data):
        length, = struct.unpack('>I', data[offset:offset + 4])
        kind = data[offset + 4:offset + 8]
        body = data[offset + 8:offset + 8 + length]
        crc, = struct.unpack('>I', data[offset + 8 + length:offset + 12 + length])
        assert crc == zlib.crc32(kind + body) & 0xffffffff
        chunks[kind] = chunks.get(kind, b'') + body
        offset += 12 + length
    width, height, bits, color = struct.unpack('>IIBB', chunks[b'IHDR'][:10])
    raw = zlib.decompress(chunks[b'IDAT'])
    row = len(raw) // height
    assert all(raw[r * row] == 0 for r in range(height))
    return width, height, bits, color, b''.join(raw[r * row + 1:(r + 1) * row] for r in range(height))


class Sink:
    def __init__(self):
        self.lines = []

    def write(self, text):
        self.lines.append(text)

    def flush(self):
        pass


class ImageTests(unittest.TestCase):
    def test_rgb_bgr_mono_and_padded_rows_decode_to_packed_rgb(self):
        rgb = pipe.decode_image('rgb8', 1, 2, 6, bytes([1, 2, 3, 4, 5, 6]))
        self.assertEqual((rgb.width, rgb.height, rgb.channels, rgb.bits), (2, 1, 3, 8))
        self.assertEqual(rgb.pixels, bytes([1, 2, 3, 4, 5, 6]))
        bgr = pipe.decode_image('bgr8', 1, 2, 6, bytearray([3, 2, 1, 6, 5, 4]))
        self.assertEqual(bgr.pixels, bytes([1, 2, 3, 4, 5, 6]))
        padded = pipe.decode_image('mono8', 2, 2, 4, bytes([1, 2, 99, 99, 3, 4, 99, 99]))
        self.assertEqual(padded.pixels, bytes([1, 2, 3, 4]))

    def test_sixteen_bit_samples_are_stored_big_endian(self):
        little = pipe.decode_image('16UC1', 1, 2, 4, struct.pack('<HH', 0x0102, 0x0304))
        self.assertEqual(little.pixels, struct.pack('>HH', 0x0102, 0x0304))
        big = pipe.decode_image('mono16', 1, 2, 4, struct.pack('>HH', 0x0102, 0x0304), bigendian=True)
        self.assertEqual(big.pixels, struct.pack('>HH', 0x0102, 0x0304))

    def test_short_or_unknown_images_are_refused(self):
        with self.assertRaisesRegex(ValueError, 'shorter'):
            pipe.decode_image('rgb8', 2, 2, 6, bytes(7))
        with self.assertRaisesRegex(ValueError, 'Unsupported'):
            pipe.decode_image('32FC1', 1, 1, 4, bytes(4))
        with self.assertRaisesRegex(ValueError, 'no pixels'):
            pipe.decode_image('mono8', 0, 1, 1, b'')

    def test_png_round_trips_gray_rgb_and_sixteen_bit(self):
        for encoding, data, color, bits in (('mono8', bytes([0, 255, 7, 8]), 0, 8),
                                            ('rgb8', bytes(range(12)), 2, 8),
                                            ('16UC1', struct.pack('<4H', 1, 2, 3, 65535), 0, 16)):
            channels = 3 if encoding == 'rgb8' else 1
            step = 2 * channels * bits // 8
            frame = pipe.decode_image(encoding, 2, 2, step, data)
            width, height, depth, kind, pixels = png_pixels(pipe.png_bytes(frame))
            self.assertEqual((width, height, depth, kind), (2, 2, bits, color))
            self.assertEqual(pixels, frame.pixels)

    def test_messages_become_plain_json(self):
        class Stamp:
            __slots__ = ('sec', 'nanosec')

            def __init__(self):
                self.sec, self.nanosec = 1, 2

        class Ros:
            def get_fields_and_field_types(self):
                return {'stamp': 'builtin_interfaces/Time', 'values': 'sequence<double>', 'raw': 'uint8[]'}

            stamp = Stamp()
            values = (1.5, float('nan'))
            raw = b'\x01\x02'
        self.assertEqual(pipe.plain(Ros()), {'stamp': {'sec': 1, 'nanosec': 2},
                                             'values': [1.5, 'nan'], 'raw': [1, 2]})
        json.dumps(pipe.plain(Ros()))

    def test_compressed_extension_follows_the_format(self):
        self.assertEqual(pipe.compressed_extension('png'), 'png')
        self.assertEqual(pipe.compressed_extension('rgb8; jpeg compressed bgr8'), 'jpg')


def write_fixture(root, seconds=2.0, hz=10):
    """A finalized recording: rgb8 images at `hz`, one CompressedImage and String rows."""
    from mcap.writer import CompressionType
    from mcap_ros2.writer import Writer
    folder = root / SESSION_ID
    (folder / 'bag').mkdir(parents=True)
    (folder / 'session.json').write_text(json.dumps({'status': 'Finalized', 'label': 'flight', 'domain': 0}))
    with open(folder / 'bag' / 'flight_0.mcap', 'wb') as stream:
        writer = Writer(stream, compression=CompressionType.NONE)
        image = writer.register_msgdef('sensor_msgs/msg/Image', IMAGE_DEF)
        compressed = writer.register_msgdef('sensor_msgs/msg/CompressedImage', COMPRESSED_DEF)
        text = writer.register_msgdef('std_msgs/msg/String', STRING_DEF)
        frames = int(seconds * hz)
        for i in range(frames):
            when = BASE_NS + i * int(1e9 / hz)
            header = {'stamp': {'sec': i, 'nanosec': 0}, 'frame_id': 'camera'}
            writer.write_message('/camera/image_raw', image, {
                'header': header, 'height': 2, 'width': 3, 'encoding': 'bgr8', 'is_bigendian': 0, 'step': 9,
                'data': bytes([i % 256] * 18)}, log_time=when, publish_time=when)
            writer.write_message('/status', text, {'data': f'frame {i}'}, log_time=when, publish_time=when)
        writer.write_message('/camera/compressed', compressed, {
            'header': {'stamp': {'sec': 0, 'nanosec': 0}, 'frame_id': 'camera'}, 'format': 'jpeg',
            'data': b'\xff\xd8fixture'}, log_time=BASE_NS, publish_time=BASE_NS)
        writer.finish()
    return folder


@unittest.skipUnless(HAS_MCAP, NO_MCAP)
class ExtractTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='pipe-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.recording = write_fixture(self.root)
        self.progress = []

    def run_extract(self, cancel=lambda: False, **params):
        request = pipe.extract_request(dict({'recording': SESSION_ID}, **params), self.root)
        return pipe.extract(request, self.progress.append, cancel)

    def index(self, manifest):
        with open(Path(manifest['path']) / 'index.csv', newline='') as stream:
            return list(csv.DictReader(stream))

    def test_png_extract_writes_frames_index_and_hashed_manifest(self):
        manifest = self.run_extract(topics=['/camera/image_raw', '/camera/compressed', '/status'], name='all')
        folder = self.recording / 'extract' / 'all'
        self.assertEqual(manifest['status'], 'Completed')
        self.assertEqual(manifest['topics']['/camera/image_raw'], {'type': pipe.IMAGE, 'count': 20})
        self.assertEqual(manifest['topics']['/camera/compressed']['count'], 1)
        self.assertEqual(manifest['topics']['/status']['count'], 20)
        rows = self.index(manifest)
        self.assertEqual(len(rows), 41)
        self.assertEqual(rows[0].keys(), {'timestamp_ns', 'topic', 'file'})
        first = next(r for r in rows if r['topic'] == '/camera/image_raw')
        self.assertEqual(first['file'], f'camera_image_raw/{BASE_NS}.png')
        width, height, _, color, pixels = png_pixels((folder / first['file']).read_bytes())
        self.assertEqual((width, height, color, pixels), (3, 2, 2, bytes(18)))
        self.assertTrue(any(r['file'].endswith('.jpg') for r in rows))
        status = [r for r in rows if r['topic'] == '/status']
        self.assertEqual(status[3]['file'], 'status.jsonl#3')
        line = (folder / 'status.jsonl').read_text().splitlines()[3]
        self.assertEqual(json.loads(line)['message'], {'data': 'frame 3'})
        for entry in manifest['files']:
            data = (folder / entry['file']).read_bytes()
            self.assertEqual(entry['sha256'], hashlib.sha256(data).hexdigest(), entry['file'])
        source = manifest['source']['files'][0]
        self.assertEqual(source['sha256'], hashlib.sha256(Path(source['file']).read_bytes()).hexdigest())
        self.assertEqual(manifest['tools']['reader'], 'mcap')
        self.assertTrue(manifest['tools']['mcap'])
        self.assertEqual(json.loads((folder / 'manifest.json').read_text())['count'], 41)

    def test_rate_and_window_limit_what_is_kept(self):
        manifest = self.run_extract(topics=['/camera/image_raw'], rate=2, name='slow')
        self.assertEqual(manifest['topics']['/camera/image_raw']['count'], 4)
        manifest = self.run_extract(topics=['/camera/image_raw'], start=0.5, end=1.0, name='window')
        stamps = [int(r['timestamp_ns']) for r in self.index(manifest)]
        self.assertEqual(stamps[0], BASE_NS + 500_000_000)
        self.assertEqual(stamps[-1], BASE_NS + 1_000_000_000)
        self.assertEqual(len(stamps), 6)

    @unittest.skipUnless(HAS_NUMPY, NO_NUMPY)
    def test_npz_extract_stores_arrays(self):
        import numpy
        manifest = self.run_extract(topics=['/camera/image_raw'], format='npz', name='arrays')
        first = self.index(manifest)[1]
        with numpy.load(self.recording / 'extract' / 'arrays' / first['file']) as data:
            self.assertEqual(data['image'].shape, (2, 3, 3))
            self.assertEqual(int(data['image'][0, 0, 0]), 1)
        self.assertIn('numpy', manifest['tools'])

    def test_cancel_keeps_partial_output_and_says_so(self):
        seen = []

        def cancel():
            seen.append(1)
            return len(seen) > 5
        manifest = self.run_extract(cancel=cancel, topics=['/camera/image_raw'], name='partial')
        self.assertEqual(manifest['status'], 'Cancelled')
        self.assertLess(manifest['count'], 20)
        self.assertEqual(len(self.index(manifest)), manifest['count'])

    def test_a_failure_mid_extraction_is_recorded_as_failed(self):
        def broken(*args):
            raise RuntimeError('disk full')
        with patch.object(pipe.Extractor, 'add', broken):
            with self.assertRaisesRegex(RuntimeError, 'disk full'):
                self.run_extract(topics=['/camera/image_raw'], name='broken')
        saved = json.loads((self.recording / 'extract' / 'broken' / 'manifest.json').read_text())
        self.assertEqual((saved['status'], saved['error']), ('Failed', 'disk full'))

    def test_missing_readers_name_the_packages(self):
        blocked = {name: None for name in ('mcap', 'mcap.reader', 'mcap_ros2', 'mcap_ros2.decoder', 'rosbag2_py')}
        with patch.dict(sys.modules, blocked):
            with self.assertRaisesRegex(RuntimeError, 'mcap-ros2-support'):
                self.run_extract(topics=['/camera/image_raw'], name='none')
        self.assertFalse((self.recording / 'extract' / 'none').exists())

    def test_worker_process_reports_progress_and_one_result(self):
        pipes = pipe.Pipelines(None, None, root=self.root, environment=lambda: dict(os.environ))
        progress = []
        result = pipes.call('extract', {'recording': SESSION_ID, 'topics': ['/camera/image_raw'], 'name': 'w'},
                            threading.Event(), progress.append)
        self.assertEqual(result['status'], 'Completed')
        self.assertEqual(result['count'], 20)
        self.assertTrue(progress[0].startswith('Extracting 1 topic'))
        with self.assertRaisesRegex(ValueError, 'already exists'):
            pipes.call('extract', {'recording': SESSION_ID, 'topics': ['/camera/image_raw'], 'name': 'w'},
                       threading.Event(), progress.append)

    def test_list_shows_finalized_recordings_and_their_runs(self):
        self.run_extract(topics=['/camera/image_raw'], name='listed')
        pipes = pipe.Pipelines(None, None, root=self.root, metadata=lambda path: {})
        listing = pipes.list()
        self.assertEqual([r['id'] for r in listing['recordings']], [SESSION_ID])
        self.assertTrue(listing['recordings'][0]['mcap'][0].endswith('flight_0.mcap'))
        self.assertEqual([(r['kind'], r['name'], r['status'], r['count']) for r in listing['runs']],
                         [('extract', 'listed', 'Completed', 20)])


class Fixture:
    """A finalized recording folder (no MCAP content needed) and a model file."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix='pipe-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.recording = self.root / SESSION_ID
        (self.recording / 'bag').mkdir(parents=True)
        (self.recording / 'session.json').write_text(json.dumps({'status': 'Finalized', 'domain': 3}))
        self.model = self.root / 'model.onnx'
        self.model.write_bytes(b'onnx fixture')

    def metadata(self, path):
        return {'topics_with_message_count': [
            {'topic_metadata': {'name': '/camera/image_raw', 'type': pipe.IMAGE}, 'message_count': 20},
            {'topic_metadata': {'name': '/status', 'type': 'std_msgs/msg/String'}, 'message_count': 20}]}


class RequestTests(Fixture, unittest.TestCase):
    def test_extract_parameters_are_validated(self):
        good = {'recording': SESSION_ID, 'topics': ['/camera/image_raw']}
        request = pipe.extract_request(dict(good, name='a', rate='5', start='', end=None), self.root)
        self.assertEqual((request['rate'], request['start'], request['format']), (5.0, None, 'png'))
        for bad in ({'recording': '../etc'}, {'topics': []}, {'topics': ['camera']}, {'format': 'gif'},
                    {'start': 2, 'end': 1}, {'rate': -1}, {'name': '../x'}, {'start': 'nan'}):
            with self.assertRaises(ValueError, msg=bad):
                pipe.extract_request(dict(good, **bad), self.root)
        (self.recording / 'session.json').write_text(json.dumps({'status': 'recording'}))
        with self.assertRaisesRegex(ValueError, 'finalize'):
            pipe.extract_request(good, self.root)

    def plan(self, **params):
        values = dict({'recording': SESSION_ID, 'model': str(self.model), 'topic': '/camera/image_raw',
                       'domain': 78, 'rate': 2}, **params)
        return pipe.infer_plan(values, self.root, self.metadata, self.root / 'projects', python='python3')

    def test_infer_plan_builds_isolated_local_only_commands(self):
        plan = self.plan()
        self.assertEqual(plan['env']['ROS_DOMAIN_ID'], '78')
        self.assertEqual(plan['env']['ROS_LOCALHOST_ONLY'], '1')
        self.assertEqual(plan['commands']['replay'][:3], ['ros2', 'bag', 'play'])
        self.assertEqual(plan['commands']['replay'][-2:], ['--topics', '/camera/image_raw'])
        self.assertIn('2.0', plan['commands']['replay'])
        self.assertEqual(plan['commands']['record'][-2:], ['--topics', pipe.DETECTIONS])
        self.assertTrue(plan['commands']['record'][6].startswith(str(self.recording / 'inference')))
        self.assertEqual(plan['node'], pipe.HERE / 'infer_node.py')
        bash = pipe.ros_command(['ros2', 'bag', 'play', "/a b/it's"])
        self.assertTrue(bash.startswith('source /opt/ros/jazzy/setup.bash && unset ROS_DISCOVERY_SERVER'))
        self.assertIn("exec ros2 bag play '/a b/it'\"'\"'s'", bash)

    def test_infer_plan_refuses_unsafe_or_unusable_inputs(self):
        for bad, message in (({'domain': 3}, 'different'), ({'domain': 300}, '232'),
                             ({'topic': '/status'}, 'Image'), ({'topic': '/nope'}, 'not in'),
                             ({'model': str(self.root / 'missing.onnx')}, '.onnx'), ({'rate': 9}, 'speed'),
                             ({'project': '../x'}, 'Project')):
            with self.assertRaisesRegex(ValueError, message):
                self.plan(**bad)

    def test_a_project_node_replaces_the_template(self):
        own = self.root / 'projects' / 'vision' / 'infer_node.py'
        own.parent.mkdir(parents=True)
        own.write_text('# custom')
        self.assertEqual(self.plan(project='vision')['node'], own)
        self.assertEqual(self.plan(project='other')['node'], pipe.HERE / 'infer_node.py')

    def test_export_copies_only_library_mcap_files_to_a_windows_folder(self):
        source = self.recording / 'bag' / 'flight_0.mcap'
        source.write_bytes(b'mcap')
        dest = self.root / 'mnt' / 'c' / 'exports'
        dest.mkdir(parents=True)
        pipes = pipe.Pipelines(None, None, root=self.root, mounts=(self.root / 'mnt').as_posix() + '/')
        result = pipes.call('export', {'path': str(source), 'dest': str(dest)}, None, None)
        self.assertEqual(Path(result['path']).read_bytes(), b'mcap')
        self.assertEqual(Path(result['path']).name, f'{SESSION_ID}-flight_0.mcap')
        elsewhere = tempfile.TemporaryDirectory(prefix='pipe-outside-')
        self.addCleanup(elsewhere.cleanup)
        outside = Path(elsewhere.name) / 'outside.mcap'
        outside.write_bytes(b'x')
        for params in ({'path': str(outside), 'dest': str(dest)},
                       {'path': str(source), 'dest': str(self.root)},
                       {'path': str(source), 'dest': (self.root / 'mnt' / '..').as_posix()}):
            with self.assertRaises(ValueError):
                pipes.call('export', params, None, None)
        with self.assertRaisesRegex(ValueError, 'Unknown pipeline method'):
            pipes.call('delete', {}, None, None)


class FakeEntry:
    def __init__(self):
        self.finished = threading.Event()
        self.child = types.SimpleNamespace(returncode=None)


class FakeProcesses:
    def __init__(self, replay_s=0.3, node_dies=False):
        self.entries, self.started, self.stopped = {}, [], []
        self.replay_s, self.node_dies = replay_s, node_dies

    def run(self, key, bash, env=None, cwd=None, readiness=None):
        self.started.append((key, bash, env))
        self.entries[key] = FakeEntry()
        if key == pipe.KEYS['replay']:
            threading.Timer(self.replay_s, self.exit, (key, 0)).start()
        if key == pipe.KEYS['node'] and self.node_dies:
            threading.Timer(0.05, self.exit, (key, 1)).start()
        return {'key': key, 'pid': 1, 'since': 0}

    def exit(self, key, code):
        entry = self.entries[key]
        if not entry.finished.is_set():
            entry.child.returncode = code
            entry.finished.set()

    def stop(self, key, grace_s=3):
        self.stopped.append(key)
        self.exit(key, -15)
        return {'key': key, 'stopped': True, 'running': False}


class QuickExitProcesses(FakeProcesses):
    """Like `procs.Processes`: a waiter thread finishes an entry under the table's lock and drops
    it from `entries`. The node exits at once, before `run` has returned."""

    def __init__(self):
        super().__init__()
        self.lock = threading.RLock()

    def run(self, key, bash, env=None, cwd=None, readiness=None):
        reply = super().run(key, bash, env, cwd, readiness)
        if key == pipe.KEYS['node']:
            waiter = threading.Thread(target=self.exit, args=(key, 3), daemon=True)
            waiter.start()
            waiter.join(0.2)  # done here unless the caller holds the lock
        return reply

    def exit(self, key, code):
        with self.lock:
            entry = self.entries.pop(key, None)
            if entry is not None and not entry.finished.is_set():
                entry.child.returncode = code
                entry.finished.set()

    def stop(self, key, grace_s=3):
        self.stopped.append(key)
        self.exit(key, -15)
        return {'key': key, 'stopped': True, 'running': False}


class InferTests(Fixture, unittest.TestCase):
    def pipes(self, processes):
        def metadata(path):
            if 'inference' in str(path):
                return {'topics_with_message_count': [
                    {'topic_metadata': {'name': pipe.DETECTIONS, 'type': 'std_msgs/msg/String'},
                     'message_count': 17}]}
            return self.metadata(path)
        pipes = pipe.Pipelines(None, processes, root=self.root, projects=self.root / 'projects',
                               metadata=metadata)
        pipes.SETTLE_S, pipes.DRAIN_S = 0.1, 0.05
        return pipes

    def params(self):
        return {'recording': SESSION_ID, 'model': str(self.model), 'topic': '/camera/image_raw', 'domain': 78}

    def test_a_run_starts_recorder_node_then_replay_and_stops_all_three(self):
        processes = FakeProcesses()
        progress = []
        run = self.pipes(processes).call('infer', self.params(), threading.Event(), progress.append)
        self.assertEqual([k for k, _, _ in processes.started], ['pipe-detections', 'pipe-infer', 'pipe-replay'])
        self.assertEqual(sorted(processes.stopped), sorted(pipe.KEYS.values()))
        self.assertEqual(run['status'], 'Completed')
        self.assertEqual(run['exits'], {'replay': 0, 'node': -15, 'record': -15})
        self.assertEqual(run['model_sha256'], hashlib.sha256(b'onnx fixture').hexdigest())
        self.assertEqual(run['recording'], SESSION_ID)
        self.assertEqual(run['topics'], ['/camera/image_raw', pipe.DETECTIONS])
        self.assertEqual(run['detections'], 17)
        self.assertGreaterEqual(run['timings']['replay_finished_s'], run['timings']['replay_started_s'])
        for _, bash, env in processes.started:
            self.assertEqual(env['ROS_DOMAIN_ID'], '78')
            self.assertIn('source /opt/ros/jazzy/setup.bash', bash)
        saved = json.loads((self.recording / 'inference' / run['run'] / 'run.json').read_text())
        self.assertEqual(saved['status'], 'Completed')
        self.assertIn('Replay finished; collecting the last detections', progress)

    def test_cancel_stops_everything_and_records_cancelled(self):
        processes = FakeProcesses(replay_s=30)
        cancel = threading.Event()
        threading.Timer(0.4, cancel.set).start()
        started = time.monotonic()
        run = self.pipes(processes).call('infer', self.params(), cancel, lambda m: None)
        self.assertLess(time.monotonic() - started, 10)
        self.assertEqual(run['status'], 'Cancelled')
        self.assertEqual(sorted(processes.stopped), sorted(pipe.KEYS.values()))

    def test_a_run_cancelled_before_it_starts_launches_nothing(self):
        processes = FakeProcesses()
        cancel = threading.Event()
        cancel.set()
        run = self.pipes(processes).call('infer', self.params(), cancel, lambda m: None)
        self.assertEqual(run['status'], 'Cancelled')
        self.assertEqual(processes.started, [])

    def test_a_node_that_dies_fails_the_run_and_stops_the_recorder(self):
        processes = FakeProcesses(node_dies=True)
        pipes = self.pipes(processes)
        with self.assertRaisesRegex(RuntimeError, 'exited before the replay'):
            pipes.call('infer', self.params(), threading.Event(), lambda m: None)
        self.assertNotIn('pipe-replay', [k for k, _, _ in processes.started])
        self.assertIn('pipe-detections', processes.stopped)
        run = next((self.recording / 'inference').iterdir())
        self.assertEqual(json.loads((run / 'run.json').read_text())['status'], 'Failed')
        listed = pipes.list()['runs']
        self.assertEqual([(r['kind'], r['status']) for r in listed], [('inference', 'Failed')])

    def test_a_process_that_exits_at_once_keeps_its_real_exit_code(self):
        processes = QuickExitProcesses()
        with self.assertRaisesRegex(RuntimeError, 'exited before the replay'):
            self.pipes(processes).call('infer', self.params(), threading.Event(), lambda m: None)
        run = next((self.recording / 'inference').iterdir())
        saved = json.loads((run / 'run.json').read_text())
        self.assertEqual(saved['exits'], {'node': 3, 'record': -15})
        self.assertEqual(saved['status'], 'Failed')

    def test_status_reports_a_request_cancelled_before_it_created_its_folder(self):
        planning, release = threading.Event(), threading.Event()

        def metadata(path):
            if 'inference' not in str(path):
                planning.set()
                release.wait(10)
            return self.metadata(path)
        processes = FakeProcesses()
        pipes = pipe.Pipelines(None, processes, root=self.root, projects=self.root / 'projects',
                               metadata=metadata)
        pipes.SETTLE_S, pipes.DRAIN_S = 0.1, 0.05
        cancel, result = threading.Event(), {}
        worker = threading.Thread(target=lambda: result.update(
            run=pipes.call('infer', self.params(), cancel, lambda m: None)))
        worker.start()
        self.assertTrue(planning.wait(10))
        cancel.set()
        self.assertEqual(pipes.call('status', {}, None, None), {'active': [pipe.PREPARING]})
        release.set()
        worker.join(10)
        self.assertEqual(result['run']['status'], 'Cancelled')
        self.assertEqual(processes.started, [])
        self.assertEqual(pipes.call('status', {}, None, None), {'active': []})

    def test_a_run_left_running_by_an_earlier_agent_lists_as_interrupted(self):
        folder = self.recording / 'inference' / 'old'
        folder.mkdir(parents=True)
        (folder / 'run.json').write_text(json.dumps({'status': 'Running', 'created': 'x'}))
        listed = self.pipes(FakeProcesses()).list()['runs']
        self.assertEqual(listed[0]['status'], 'Interrupted')

    def test_a_run_this_agent_is_writing_lists_as_running_and_in_status(self):
        writing = self.recording / 'extract' / 'now'
        writing.mkdir(parents=True)
        (writing / 'manifest.json').write_text(json.dumps({'status': 'Running', 'created': 'b'}))
        stale = self.recording / 'extract' / 'old'
        stale.mkdir(parents=True)
        (stale / 'manifest.json').write_text(json.dumps({'status': 'Running', 'created': 'a'}))
        pipes = self.pipes(FakeProcesses())
        pipes.active.add(writing)
        listed = {r['name']: r['status'] for r in pipes.list()['runs']}
        self.assertEqual(listed, {'now': 'Running', 'old': 'Interrupted'})
        self.assertEqual(pipes.call('status', {}, None, None), {'active': [str(writing)]})

    def test_malformed_manifests_list_without_breaking_the_others(self):
        bad = self.recording / 'extract' / 'bad'
        bad.mkdir(parents=True)
        (bad / 'manifest.json').write_text(json.dumps(
            {'status': 5, 'created': ['x'], 'count': 'many', 'params': 'png', 'topics': [1]}))
        (self.recording / 'session.json').write_text(json.dumps({'status': 'Finalized', 'domain': 'three'}))
        listing = self.pipes(FakeProcesses()).list()
        self.assertEqual(listing['recordings'][0]['domain'], 0)
        row = listing['runs'][0]
        self.assertEqual((row['status'], row['created'], row['count'], row['format'], row['topics']),
                         ('Unknown', None, 0, None, []))


STUCK_WORKER = '''
import json, pathlib, sys, time
request = json.loads(sys.stdin.readline())
folder = pathlib.Path(request['folder'])
folder.mkdir(parents=True)
(folder / 'manifest.json').write_text(json.dumps({'status': 'Running'}))
print(json.dumps({'progress': 'Reading'}), flush=True)
time.sleep(60)  # stuck in a reader call: never reads the cancel line
'''


class CancelTests(unittest.TestCase):
    def test_a_worker_stuck_after_cancel_is_killed_and_marked_cancelled(self):
        temporary = tempfile.TemporaryDirectory(prefix='pipe-')
        self.addCleanup(temporary.cleanup)
        folder = Path(temporary.name) / 'extract' / 'stuck'
        pipes = pipe.Pipelines(None, None, root=temporary.name, python=(sys.executable, '-c', STUCK_WORKER),
                               environment=lambda: dict(os.environ))
        pipes.CANCEL_GRACE_S = 0.5
        cancel = threading.Event()
        progress = []

        def on_progress(message):
            progress.append(message)
            if message == 'Reading':
                cancel.set()
        request = {'folder': str(folder), 'topics': ['/a'], 'recording_path': temporary.name}
        started = time.monotonic()
        with self.assertRaisesRegex(RuntimeError, 'did not stop within 0.5 s'):
            pipes._extract(request, cancel, on_progress)
        self.assertLess(time.monotonic() - started, 20)
        manifest = json.loads((folder / 'manifest.json').read_text())
        self.assertEqual(manifest['status'], 'Cancelled')
        self.assertIn('finished', manifest)


class DispatchTests(unittest.TestCase):
    def test_pipe_methods_reach_the_pipelines_with_their_progress(self):
        class FakePipes:
            def call(self, name, params, cancel, progress):
                progress('Extracted 3 of 4 messages read')
                return {'name': name, 'params': params}
        sink = Sink()
        service = agent.Agent(channel.Out(sink), processes=types.SimpleNamespace(detach=lambda: None),
                              mavlink=types.SimpleNamespace(stop=lambda: None),
                              recorder=types.SimpleNamespace(close=lambda: None), data=object(),
                              pipes=FakePipes())
        service.serve(io.StringIO(json.dumps({'id': 7, 'method': 'pipe.extract', 'params': {'a': 1}}) + '\n'))
        lines = [json.loads(line) for line in sink.lines]
        self.assertIn({'event': 'progress', 'id': 7, 'message': 'Extracted 3 of 4 messages read'}, lines)
        self.assertIn({'id': 7, 'result': {'name': 'extract', 'params': {'a': 1}}}, lines)


@unittest.skipUnless(HAS_NUMPY, NO_NUMPY)
class NodeTests(unittest.TestCase):
    def setUp(self):
        import numpy
        self.numpy = numpy
        spec = importlib.util.spec_from_file_location('infer_node', pipe.HERE / 'infer_node.py')
        self.node = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.node)

    def test_preprocess_matches_nchw_nhwc_and_dynamic_inputs(self):
        np = self.numpy
        image = np.arange(4 * 6 * 3, dtype=np.uint8).reshape(4, 6, 3)
        nchw = self.node.preprocess(image, [1, 3, 2, 3], np)
        self.assertEqual((nchw.shape, nchw.dtype), ((1, 3, 2, 3), np.float32))
        self.assertAlmostEqual(float(nchw[0, 0, 0, 0]), 0.0)
        self.assertAlmostEqual(float(nchw[0, 1, 0, 0]), 1 / 255, places=6)
        nhwc = self.node.preprocess(image, ['batch', 'h', 'w', 3], np, float_input=False)
        self.assertEqual((nhwc.shape, nhwc.dtype), ((1, 4, 6, 3), np.uint8))
        gray = self.node.preprocess(image[:, :, 0], [1, 3, 4, 6], np)
        self.assertEqual(gray.shape, (1, 3, 4, 6))
        mono = self.node.preprocess(image, [1, 1, 4, 6], np)
        self.assertEqual(mono.shape, (1, 1, 4, 6))
        deep = self.node.preprocess(np.full((2, 2), 65535, np.uint16), [1, 1, 2, 2], np)
        self.assertAlmostEqual(float(deep.max()), 1.0)
        with self.assertRaises(ValueError):
            self.node.preprocess(image, [1, 3, 4], np)

    def test_outputs_are_summarized_without_inventing_meaning(self):
        np = self.numpy
        scores = np.zeros((1, 1000), np.float32)
        scores[0, [7, 42, 3]] = [0.5, 0.9, 0.1]
        rows = self.node.summarize(['small', 'scores'], [np.array([[1.0, 2.0]]), scores], np)
        self.assertEqual(rows[0], {'name': 'small', 'shape': [1, 2], 'dtype': 'float64', 'values': [1.0, 2.0]})
        self.assertEqual(rows[1]['top'][:3], [[42, 0.9], [7, 0.5], [3, 0.1]])
        text = self.node.detection(5, 'camera', '/camera/image_raw', (1, 3, 2, 2), 1.23456, rows)
        value = json.loads(text)
        self.assertEqual((value['stamp_ns'], value['latency_ms'], value['input']), (5, 1.235, [1, 3, 2, 2]))

    def test_non_finite_outputs_stay_strict_json(self):
        np = self.numpy
        scores = np.zeros(100, np.float32)
        scores[[1, 2]] = [np.nan, 0.75]
        rows = self.node.summarize(['small', 'scores'], [np.array([np.inf, 1.0]), scores], np)
        self.assertEqual(rows[0]['values'], [None, 1.0])
        self.assertEqual(rows[1]['top'][0], [2, 0.75])
        text = self.node.detection(1, '', '/c', (1,), 0.0, rows)
        self.assertNotIn('NaN', text)
        json.loads(text, parse_constant=lambda c: self.fail(f'non-standard JSON constant {c}'))

    def test_raw_images_decode_through_the_shared_decoder(self):
        message = types.SimpleNamespace(encoding='bgr8', height=1, width=1, step=3, data=bytes([3, 2, 1]),
                                        is_bigendian=0)
        array = self.node.image_array(message, pipe.IMAGE, self.numpy)
        self.assertEqual(array.tolist(), [[[1, 2, 3]]])


if __name__ == '__main__':
    unittest.main()
