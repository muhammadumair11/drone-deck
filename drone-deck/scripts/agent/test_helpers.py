"""Probe, file listing, output tailing, SIH readiness and the rec/data adapters, without ROS or PX4."""
import json
import os
from pathlib import Path
import sys
import tempfile
import threading
import time
import types
import unittest

import probe
import procs
import services
import sih

PYTHON = (sys.executable, '-c')


class Events:
    def __init__(self):
        self.items = []

    def event(self, name, **fields):
        self.items.append(dict(fields, event=name))

    def log(self, level, message):
        self.event('log', level=level, message=message)


class ProbeTests(unittest.TestCase):
    def test_each_check_has_its_own_result_and_timeout(self):
        started = time.monotonic()
        rows = probe.probe([{'id': 'ok', 'bash': 'print("found")'},
                            {'id': 'fail', 'bash': 'raise SystemExit(2)'},
                            {'id': 'slow', 'bash': 'import time; time.sleep(30)'}], 1, shell=PYTHON)
        self.assertLess(time.monotonic() - started, 10)
        by_id = {row['id']: row for row in rows}
        self.assertEqual((by_id['ok']['ok'], by_id['ok']['out']), (True, 'found'))
        self.assertFalse(by_id['fail']['ok'])
        self.assertFalse(by_id['slow']['ok'])
        self.assertIn('timed out', by_id['slow']['out'])

    def test_cancel_stops_a_check(self):
        cancel = threading.Event()
        threading.Timer(.3, cancel.set).start()
        ok, out = probe.run_check('import time; time.sleep(30)', 20, cancel, PYTHON)
        self.assertFalse(ok)
        self.assertIn('cancelled', out)

    def test_output_is_bounded_and_a_lingering_grandchild_does_not_hang_the_check(self):
        ok, out = probe.run_check('import sys; sys.stdout.write("x" * 5_000_000 + "tail")', 20,
                                  shell=PYTHON)
        self.assertTrue(ok)
        self.assertEqual(len(out), probe.OUTPUT_LIMIT)
        self.assertTrue(out.endswith('tail'))
        started = time.monotonic()
        # The grandchild inherits the output pipe and outlives the check by 30 s.
        ok, out = probe.run_check(
            'import subprocess, sys; subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"],'
            ' start_new_session=True) if sys.platform != "win32" else subprocess.Popen('
            '[sys.executable, "-c", "import time; time.sleep(30)"]); print("parent done")', 20, shell=PYTHON)
        self.assertLess(time.monotonic() - started, 10)
        self.assertTrue(ok)
        self.assertIn('parent done', out)

    def test_invalid_checks_are_rejected(self):
        for checks in (None, [{'id': 1, 'bash': 'x'}], [{'id': 'a'}], [{}] * 65):
            with self.assertRaises(ValueError):
                probe.probe(checks, 1, shell=PYTHON)
        with self.assertRaises(ValueError):
            probe.timeout_seconds('soon')

    def test_files_list_newest_first_with_sizes(self):
        with tempfile.TemporaryDirectory() as folder:
            older, newer = Path(folder, 'a.ulg'), Path(folder, 'sub', 'b.ulg')
            newer.parent.mkdir()
            older.write_bytes(b'12')
            newer.write_bytes(b'1234')
            os.utime(older, (1000, 1000))
            rows = probe.files_list(folder, '**/*.ulg')
            self.assertEqual([(Path(r['path']).name, r['size']) for r in rows], [('b.ulg', 4), ('a.ulg', 2)])
            self.assertEqual(probe.files_list(folder, '*.ulg', limit=1)[0]['size'], 2)
            self.assertEqual(probe.files_list(os.path.join(folder, 'missing'), '*'), [])
            with self.assertRaises(ValueError):
                probe.files_list(folder, '/etc/*')


class TailTests(unittest.TestCase):
    def setUp(self):
        self.events = Events()
        self.processes = procs.Processes(self.events, directory=tempfile.gettempdir())
        self.state = {'path': None, 'offset': 0, 'partial': b'', 'skip': False}

    def lines(self):
        return [e['line'] for e in self.events.items if e['event'] == 'output']

    def test_lines_are_split_across_reads_and_bounded(self):
        feed = lambda data, final=False: self.processes._lines('k', 'stdout', self.state, data, final)
        feed(b'one\ntw')
        feed(b'o\r\n' + b'x' * 5000)
        feed(b'yyy\nthree')
        self.assertEqual(self.lines()[:2], ['one', 'two'])
        self.assertEqual(len(self.lines()[2].encode()), procs.LINE_LIMIT)
        self.assertEqual(len(self.lines()), 3)
        feed(b'', final=True)
        self.assertEqual(self.lines()[-1], 'three')
        feed(b'caf\xc3\xa9 \xff\n')
        self.assertEqual(self.lines()[-1], 'café �')

    def test_adopted_output_starts_at_a_line_boundary(self):
        with tempfile.TemporaryDirectory() as folder:
            out, err = Path(folder, 'k.out'), Path(folder, 'k.err')
            out.write_bytes(b'z' * (procs.ADOPT_CONTEXT + 10) + b'\nnext\n')
            err.write_bytes(b'')
            entry = procs.Entry('k', 1, '1', None)
            entry.attach({'out': out, 'err': err}, from_start=False)
            self.processes._drain(entry)
            self.assertEqual(self.lines(), ['next'])

    def test_birth_parsing_survives_odd_command_names(self):
        if not sys.platform.startswith('linux'):
            self.skipTest('/proc is Linux only')
        self.assertIsNotNone(procs.birth(os.getpid()))
        self.assertIsNone(procs.birth(999999999))
        self.assertIsNone(procs.birth('1'))


class ReadinessTests(unittest.TestCase):
    def test_ready_once_vehicle_status_arrives(self):
        reports, answers = [], iter([None, None, 'pre_flight_checks_pass: True'])
        self.assertTrue(sih.monitor(lambda: True, lambda *r: reports.append(r), lambda: next(answers),
                                    sleep=lambda _: None))
        self.assertEqual(reports, [('Ready', 'SIH vehicle status received. Preflight checks: passed')])

    def test_failed_after_timeout_and_silent_when_process_exits(self):
        now = [0.0]

        def sleep(seconds):
            now[0] += seconds

        reports = []
        self.assertFalse(sih.monitor(lambda: True, lambda *r: reports.append(r), lambda: None, timeout=2,
                                     sleep=sleep, clock=lambda: now[0]))
        self.assertEqual(reports[0][0], 'Failed')
        reports.clear()
        sih.monitor(lambda: False, lambda *r: reports.append(r), lambda: None, sleep=sleep, clock=lambda: now[0])
        self.assertEqual(reports, [])


class AdapterTests(unittest.TestCase):
    def test_recorder_routes_worker_output_to_rec_events_and_finalizes_on_close(self):
        module = types.SimpleNamespace(emit=None)

        class Server:
            def __init__(self, root):
                self.root, self.stopped, self.ticks = root, False, 0

            def dispatch(self, request):
                module.emit({'kind': 'activity', 'message': request['action']})
                return {'root': self.root, 'request': request}

            def tick(self):
                self.ticks += 1

            def stop(self):
                self.stopped = True

        module.Server = Server
        events = Events()
        recorder = services.Recorder(events, root='/r', module=module, tick=0.01)
        result = recorder.call('list', {'domain': 3})
        self.assertEqual(result, {'root': '/r', 'request': {'domain': 3, 'action': 'list'}})
        self.assertEqual(events.items[0], {'event': 'rec', 'data': {'kind': 'activity', 'message': 'list'}})
        with self.assertRaises(ValueError):
            recorder.call('shutdown', {})
        deadline = time.monotonic() + 5
        while recorder.server.ticks == 0 and time.monotonic() < deadline:
            time.sleep(.01)
        recorder.close()
        self.assertTrue(recorder.server.stopped)
        self.assertGreater(recorder.server.ticks, 0)

    def test_data_progress_goes_to_the_request(self):
        module = types.SimpleNamespace()

        def dispatch(request):
            module.emit({'progress': 'Hashing a.mcap'})
            module.emit({'unrelated': True})
            return {'action': request['action']}

        module.dispatch = dispatch
        seen = []
        data = services.Data(module)
        self.assertEqual(data.call('library', {}, seen.append), {'action': 'data_library'})
        self.assertEqual(seen, ['Hashing a.mcap'])
        with self.assertRaises(ValueError):
            data.call('delete_everything', {}, seen.append)

    def test_data_calls_run_concurrently_and_keep_their_own_progress(self):
        module = types.SimpleNamespace()
        release = threading.Event()

        def dispatch(request):
            if request['action'] == 'project_create':
                module.emit({'progress': 'Copying template'})
                release.wait(5)
                module.emit({'progress': 'Copied template'})
            else:
                module.emit({'progress': 'Listing'})
            return request['action']

        module.dispatch = dispatch
        data = services.Data(module)
        slow_seen, fast_seen, results = [], [], []
        worker = threading.Thread(target=lambda: results.append(
            data.call('project_create', {}, slow_seen.append)))
        worker.start()
        deadline = time.monotonic() + 5
        while not slow_seen and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertEqual(data.call('library', {}, fast_seen.append), 'data_library')
        release.set()
        worker.join(5)
        self.assertEqual(results, ['project_create'])
        self.assertEqual(fast_seen, ['Listing'])
        self.assertEqual(slow_seen, ['Copying template', 'Copied template'])

    def test_the_real_helpers_load_without_ros(self):
        studio = services.load('studio_data_for_test', 'studio/data.py')
        self.assertTrue(callable(studio.dispatch))
        recordings = services.load('recordings_for_test', 'recordings.py')
        self.assertTrue(hasattr(recordings, 'Server'))
        with tempfile.TemporaryDirectory() as folder:
            events = Events()
            recorder = services.Recorder(events, root=folder, module=recordings)
            self.assertEqual(recorder.call('list', {}), [])
            recorder.close()
        self.assertEqual(json.loads(json.dumps(events.items)), events.items)


if __name__ == '__main__':
    unittest.main()
