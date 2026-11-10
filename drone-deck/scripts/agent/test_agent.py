"""Agent protocol tests. In-process dispatch runs anywhere; the subprocess tests need Linux
(process groups, FIFOs, /proc) and use bash with plain commands: no ROS, PX4 or WSL."""
import io
import json
import os
from pathlib import Path
import queue
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest

import agent
import channel

LINUX = sys.platform.startswith('linux')
SCRIPT = str(Path(__file__).with_name('agent.py'))


class Sink:
    def __init__(self):
        self.lines = []

    def write(self, text):
        self.lines.extend(json.loads(line) for line in text.splitlines() if line)

    def flush(self):
        pass

    def replies(self):
        return {line['id']: line for line in self.lines if 'id' in line and 'event' not in line}

    def events(self, name):
        return [line for line in self.lines if line.get('event') == name]


class FakeRecorder:
    def __init__(self):
        self.calls = []
        self.closed = False

    def call(self, action, params):
        self.calls.append((action, params))
        if action == 'check':
            return {'ready': True}
        raise ValueError('Select a recording first.')

    def close(self):
        self.closed = True


class FakeData:
    def call(self, name, params, progress):
        progress('Hashing fixture.mcap')
        return {'name': name, 'params': params}


class FakeRouter:
    def __init__(self):
        self.stopped = False

    def start(self, *args):
        return {'state': 'listening', 'args': list(args)}

    def stop(self):
        self.stopped = True
        return {'state': 'stopped'}

    def status(self):
        return {'state': 'stopped'}


class FakeProcesses:
    def __init__(self):
        self.detached = False
        self.runs = []

    def run(self, *args):
        self.runs.append(args)
        return {'key': args[0], 'pid': 1}

    def detach(self):
        self.detached = True


class DispatchTests(unittest.TestCase):
    def setUp(self):
        self.sink = Sink()
        self.recorder = FakeRecorder()
        self.router = FakeRouter()
        self.processes = FakeProcesses()
        self.agent = agent.Agent(channel.Out(self.sink), processes=self.processes, mavlink=self.router,
                                 recorder=self.recorder, data=FakeData())

    def run_lines(self, *requests):
        stream = io.StringIO(''.join(json.dumps(r) + '\n' for r in requests))
        self.agent.serve(stream)
        return self.sink.replies()

    def test_hello_reports_the_environment_the_app_needs(self):
        replies = self.run_lines({'id': 1, 'method': 'hello'})
        result = replies[1]['result']
        for field in ('version', 'distro', 'user', 'home', 'automount_root', 'python'):
            self.assertIn(field, result)
        self.assertEqual(result['version'], agent.VERSION)

    def test_run_gives_processes_the_scripts_folder_this_distro_sees(self):
        replies = self.run_lines({'id': 1, 'method': 'run', 'params': {'key': 'a', 'bash': 'true'}},
                                 {'id': 2, 'method': 'run',
                                  'params': {'key': 'b', 'bash': 'true', 'env': {'X': '1'}}},
                                 {'id': 3, 'method': 'run',
                                  'params': {'key': 'c', 'bash': 'true', 'env': ['X']}})
        scripts = str(Path(agent.__file__).resolve().parent.parent)
        self.assertEqual(self.processes.runs[0][2], {'DRONE_DECK_SCRIPTS': scripts})
        self.assertEqual(self.processes.runs[1][2], {'DRONE_DECK_SCRIPTS': scripts, 'X': '1'})
        self.assertIn('result', replies[2])
        self.assertIn('env', replies[3]['error']['message'])
        self.assertEqual(len(self.processes.runs), 2)

    def test_errors_are_replies_and_bad_lines_do_not_stop_the_agent(self):
        stream = io.StringIO('not json\n[1]\n'
                             + json.dumps({'id': 2, 'method': 'nope'}) + '\n'
                             + json.dumps({'id': 3, 'method': 'hello', 'params': [1]}) + '\n'
                             + json.dumps({'id': 4, 'method': 'hello'}) + '\n')
        self.agent.serve(stream)
        replies = self.sink.replies()
        self.assertIn('Unknown method', replies[2]['error']['message'])
        self.assertIn('params', replies[3]['error']['message'])
        self.assertIn('result', replies[4])
        self.assertEqual(len(self.sink.events('log')), 2)

    def test_malformed_ids_and_methods_never_stop_the_agent(self):
        stream = io.StringIO(''.join(json.dumps(r) + '\n' for r in (
            {'id': [1], 'method': 'hello'},
            {'id': {'a': 1}, 'method': 'cancel'},
            {'id': 2, 'method': ['hello']},
            {'id': 3, 'method': 'cancel', 'params': {'id': [9]}},
            {'id': 4, 'method': 'hello'})))
        self.agent.serve(stream)
        replies = self.sink.replies()
        self.assertIn('method', replies[2]['error']['message'])
        self.assertEqual(replies[3]['result'], {'cancelled': False})
        self.assertIn('result', replies[4])
        self.assertEqual(len(self.sink.events('log')), 2)

    def test_rec_and_data_methods_route_to_the_ported_helpers(self):
        replies = self.run_lines({'id': 5, 'method': 'rec.check'},
                                 {'id': 6, 'method': 'rec.replay', 'params': {'domain': 77}},
                                 {'id': 7, 'method': 'data.library', 'params': {'x': 1}})
        self.assertEqual(replies[5]['result'], {'ready': True})
        self.assertEqual(replies[6]['error']['message'], 'Select a recording first.')
        self.assertEqual(self.recorder.calls[1], ('replay', {'domain': 77}))
        self.assertEqual(replies[7]['result'], {'name': 'library', 'params': {'x': 1}})
        progress = self.sink.events('progress')
        self.assertEqual(progress, [{'event': 'progress', 'id': 7, 'message': 'Hashing fixture.mcap'}])

    def test_end_of_input_stops_the_router_and_finalizes_recordings(self):
        self.run_lines({'id': 1, 'method': 'router.start', 'params': {'udp_listen': 0, 'tcp_port': 0}})
        self.assertTrue(self.router.stopped)
        self.assertTrue(self.recorder.closed)
        self.assertTrue(self.processes.detached)

    def test_shutdown_replies_then_stops_reading(self):
        replies = self.run_lines({'id': 1, 'method': 'shutdown'}, {'id': 2, 'method': 'hello'})
        self.assertIn(1, replies)
        self.assertNotIn(2, replies)

    def test_cancel_reaches_a_running_request(self):
        started = threading.Event()
        seen = []

        def slow(params, cancel, request_id):
            started.set()
            seen.append(cancel.wait(5))
            return {}

        self.agent.table['slow'] = slow
        self.agent.handle(json.dumps({'id': 9, 'method': 'slow'}))
        self.assertTrue(started.wait(5))
        self.agent.handle(json.dumps({'id': 10, 'method': 'cancel', 'params': {'id': 9}}))
        self.agent.close()
        self.assertEqual(seen, [True])
        self.assertEqual(self.sink.replies()[10]['result'], {'cancelled': True})

    def test_automount_root_prefers_wsl_conf_then_mounts(self):
        with tempfile.TemporaryDirectory() as folder:
            conf = Path(folder) / 'wsl.conf'
            mounts = Path(folder) / 'mounts'
            mounts.write_text('none /usr/lib/wsl/drivers 9p ro 0 0\nC:\\134 /win/c 9p rw 0 0\n')
            self.assertEqual(agent.automount_root(str(conf), str(mounts)), '/win/')
            conf.write_text('[boot]\nsystemd=true\n[automount]\nroot = /drives # comment\n')
            self.assertEqual(agent.automount_root(str(conf), str(mounts)), '/drives/')
            self.assertEqual(agent.automount_root(str(conf) + '.missing', str(mounts) + '.missing'), '/mnt/')


class Client:
    """Talks to a real agent subprocess."""

    def __init__(self, state):
        env = dict(os.environ, DRONE_DECK_AGENT_STATE=state)
        self.child = subprocess.Popen([sys.executable, '-u', SCRIPT], stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, text=True, env=env)
        self.lines = queue.Queue()
        self.next_id = 0
        self.backlog = []
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self.child.stdout:
            self.lines.put(json.loads(line))
        self.lines.put(None)

    def call(self, method, timeout=10, **params):
        self.next_id += 1
        request_id = self.next_id
        self.child.stdin.write(json.dumps({'id': request_id, 'method': method, 'params': params}) + '\n')
        self.child.stdin.flush()
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                line = self.lines.get(timeout=0.1)
            except queue.Empty:
                continue
            if line is None:
                raise AssertionError('agent exited')
            if line.get('id') == request_id and 'event' not in line:
                if 'error' in line:
                    raise RuntimeError(line['error']['message'])
                return line['result']
            self.backlog.append(line)
        raise AssertionError(f'no reply to {method}')

    def wait_event(self, predicate, timeout=10):
        for line in self.backlog:
            if predicate(line):
                self.backlog.remove(line)
                return line
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                line = self.lines.get(timeout=0.1)
            except queue.Empty:
                continue
            if line is None:
                break
            if predicate(line):
                return line
            self.backlog.append(line)
        raise AssertionError('event did not arrive')

    def close(self):
        if self.child.poll() is None:
            self.child.stdin.close()
            self.child.wait(timeout=15)
        self.child.stdout.close()


@unittest.skipUnless(LINUX, 'owned process groups need Linux')
class ProcessTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='drone-deck-agent-')
        self.addCleanup(self.temp.cleanup)
        self.state = os.path.join(self.temp.name, 'state')
        self.agent = Client(self.state)
        self.addCleanup(self.agent.close)

    def output(self, key, text):
        return self.agent.wait_event(lambda e: e.get('event') == 'output' and e['key'] == key
                                     and text in e['line'])

    def test_run_streams_output_takes_input_and_reports_exit(self):
        result = self.agent.call('run', key='echo', bash='echo "started $DRONE_DECK_X"; read line; '
                                 'echo "got $line"; echo oops >&2; exit 3', env={'DRONE_DECK_X': 'ok'})
        self.assertGreater(result['pid'], 0)
        self.output('echo', 'started ok')
        self.agent.call('input', key='echo', line='ping')
        self.output('echo', 'got ping')
        error = self.agent.wait_event(lambda e: e.get('event') == 'output' and e['stream'] == 'stderr')
        self.assertEqual(error['line'], 'oops')
        exited = self.agent.wait_event(lambda e: e.get('event') == 'exited' and e['key'] == 'echo')
        self.assertEqual(exited['code'], 3)
        self.assertFalse(Path(self.state, 'echo.json').exists())

    def test_stdin_never_reaches_eof(self):
        self.agent.call('run', key='eof', bash='python3 -c "import sys; sys.stdin.read(1); print(\'EOF\')"')
        time.sleep(.6)
        rows = {row['key']: row for row in self.agent.call('list')}
        self.assertTrue(rows['eof']['alive'])
        self.assertTrue(self.agent.call('stop', key='eof', grace_s=2)['stopped'])
        self.agent.wait_event(lambda e: e.get('event') == 'exited' and e['key'] == 'eof')
        self.assertFalse(any(e.get('line') == 'EOF' for e in self.agent.backlog))

    def test_duplicate_key_is_refused_and_stop_ends_the_whole_group(self):
        self.agent.call('run', key='group', bash='sleep 60 & sleep 60; wait')
        with self.assertRaisesRegex(RuntimeError, 'already running'):
            self.agent.call('run', key='group', bash='sleep 1')
        record = json.loads(Path(self.state, 'group.json').read_text())
        result = self.agent.call('stop', key='group', grace_s=2)
        self.assertEqual(result, {'key': 'group', 'stopped': True, 'running': False})
        # `exited` comes before the stop reply, so a restart of the key is never confused with it.
        self.assertTrue(any(e.get('event') == 'exited' and e['key'] == 'group' for e in self.agent.backlog))
        self.assertEqual(self.agent.call('run', key='group', bash='sleep 60')['key'], 'group')
        self.agent.call('stop', key='group', grace_s=1)
        self.assertFalse(Path(f'/proc/{record["pid"]}').exists())
        self.assertEqual(self.agent.call('stop', key='group')['stopped'], False)

    def test_a_later_agent_lists_adopts_and_stops_a_leftover_process(self):
        self.agent.call('run', key='left', bash='while true; do echo tick; sleep 0.2; done')
        self.output('left', 'tick')
        self.agent.close()
        later = Client(self.state)
        self.addCleanup(later.close)
        rows = {row['key']: row for row in later.call('list')}
        self.assertTrue(rows['left']['alive'])
        self.assertFalse(rows['left']['attached'])
        self.assertIsInstance(rows['left']['since'], float)
        self.assertGreaterEqual(rows['left']['age_s'], 0)
        with self.assertRaisesRegex(RuntimeError, 'earlier Drone Deck session'):
            later.call('run', key='left', bash='true')
        self.assertTrue(later.call('adopt', key='left')['output'])
        later.wait_event(lambda e: e.get('event') == 'output' and e['key'] == 'left')
        self.assertTrue(later.call('stop', key='left')['stopped'])
        exited = later.wait_event(lambda e: e.get('event') == 'exited' and e['key'] == 'left')
        self.assertIsNone(exited['code'])
        self.assertNotIn('left', {row['key'] for row in later.call('list')})

    def test_a_group_whose_leader_died_is_still_listed_and_stopped(self):
        self.agent.call('run', key='orphan', bash='sleep 60 & echo ORPHAN_READY; sleep 60')
        # run replies after spawning bash, before it has necessarily spawned the background child.
        # Wait for that child to exist before killing its leader; otherwise there is no orphan.
        self.output('orphan', 'ORPHAN_READY')
        record = json.loads(Path(self.state, 'orphan.json').read_text())
        self.agent.close()
        os.kill(record['pid'], signal.SIGKILL)
        deadline = time.monotonic() + 5
        while Path(f'/proc/{record["pid"]}/stat').exists() and time.monotonic() < deadline:
            if Path(f'/proc/{record["pid"]}/stat').read_text().rsplit(')', 1)[-1].split()[0] == 'Z':
                break
            time.sleep(.05)
        later = Client(self.state)
        self.addCleanup(later.close)
        rows = {row['key']: row for row in later.call('list')}
        self.assertTrue(rows['orphan']['alive'], 'the background sleep still holds the key')
        self.assertEqual(later.call('stop', key='orphan', grace_s=2),
                         {'key': 'orphan', 'stopped': True, 'running': False})
        # An unattached stopped pidfile is reported dead once while list prunes it.
        stopped_rows = {row['key']: row for row in later.call('list')}
        self.assertFalse(stopped_rows.get('orphan', {}).get('alive', False))
        self.assertNotIn('orphan', {row['key'] for row in later.call('list')})
        self.assertEqual(later.call('run', key='orphan', bash='true')['key'], 'orphan')

    def test_stale_pidfiles_are_reported_dead_and_removed(self):
        os.makedirs(self.state, mode=0o700, exist_ok=True)
        Path(self.state, 'old.json').write_text(json.dumps({'pid': 999999999, 'birth': '1'}))
        rows = self.agent.call('list')
        self.assertEqual([(r['key'], r['alive']) for r in rows], [('old', False)])
        self.assertEqual(self.agent.call('list'), [])

    def test_probe_runs_checks_under_their_own_timeout(self):
        started = time.monotonic()
        rows = self.agent.call('probe', checks=[{'id': 'yes', 'bash': 'echo found'},
                                                {'id': 'no', 'bash': 'exit 1'},
                                                {'id': 'slow', 'bash': 'sleep 30'}], timeout_s=1)
        self.assertLess(time.monotonic() - started, 8)
        by_id = {row['id']: row for row in rows}
        self.assertEqual((by_id['yes']['ok'], by_id['yes']['out']), (True, 'found'))
        self.assertFalse(by_id['no']['ok'])
        self.assertIn('timed out', by_id['slow']['out'])

    def test_invalid_keys_and_commands_are_rejected(self):
        for key in ('../x', '', 'a b', 'x' * 65):
            with self.assertRaises(RuntimeError):
                self.agent.call('run', key=key, bash='true')
        with self.assertRaises(RuntimeError):
            self.agent.call('run', key='empty', bash='  ')
        with self.assertRaises(RuntimeError):
            self.agent.call('run', key='env', bash='true', env={'BAD NAME': '1'})


if __name__ == '__main__':
    unittest.main()
