"""resim.* without ROS: the flight library, prepare, the run plan and a run against fake
processes, plus the command player's status record."""
import json
from pathlib import Path
import tempfile
import threading
import types
import unittest

import arbiter
import resim

SESSION_ID = '20261114T101500Z-offboard'
RECIPE = {'schema': 1, 'adapter': 'px4-gazebo', 'autopilot': 'px4', 'system': 'Lab',
          'world': {'id': 'gazebo', 'params': {'world': 'default'}},
          'command_topics': ['/fmu/in/trajectory_setpoint', '/fmu/in/vehicle_command'],
          'evidence_topics': ['/fmu/out/vehicle_status_v1', 'not a topic'], 'params': 'params.params'}


class Entry:
    def __init__(self):
        self.finished = threading.Event()
        self.child = types.SimpleNamespace(returncode=None)


class Processes:
    """The arbiter writes a status and exits with `code` after `run_s`."""

    def __init__(self, run_s=0.2, code=0, folder_status=None):
        self.entries, self.started, self.stopped = {}, [], []
        self.run_s, self.code, self.folder_status = run_s, code, folder_status

    def run(self, key, bash, env=None, cwd=None, readiness=None):
        self.started.append((key, bash, env))
        self.entries[key] = Entry()
        if key == resim.KEYS['arbiter']:
            out = Path(bash.split('--out ')[1].split(' ')[0].strip("'"))
            if self.folder_status is not None:
                (out / 'status.json').write_text(json.dumps(self.folder_status))
            if self.run_s is not None:
                threading.Timer(self.run_s, self.exit, (key, self.code)).start()
        return {'key': key}

    def exit(self, key, code):
        entry = self.entries[key]
        if not entry.finished.is_set():
            entry.child.returncode = code
            entry.finished.set()

    def stop(self, key, grace_s=3):
        self.stopped.append(key)
        self.exit(key, -15)


class Fixture:
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.recording = self.root / SESSION_ID
        self.recording.mkdir()
        (self.recording / 'session.json').write_text(json.dumps({'status': 'finalized', 'label': 'offboard'}))
        (self.recording / 'recipe.json').write_text(json.dumps(RECIPE))
        (self.recording / 'params.params').write_text('1\t1\tMPC_XY_VEL_MAX\t8.0\t9\n')
        plain = self.root / '20261113T090000Z-plain'
        plain.mkdir()
        (plain / 'session.json').write_text(json.dumps({'status': 'finalized'}))

    def tearDown(self):
        self.tmp.cleanup()

    def server(self, processes=None):
        return resim.Resim(None, processes or Processes(), root=str(self.root))


class Library(Fixture, unittest.TestCase):
    def test_only_recordings_with_a_recipe_are_listed(self):
        listed = self.server().call('list', {}, None, None)
        self.assertEqual([f['id'] for f in listed['flights']], [SESSION_ID])
        flight = listed['flights'][0]
        self.assertEqual(flight['recipe'], {'origin': 'simulated', 'source': '', 'adapter': 'px4-gazebo',
                                            'autopilot': 'px4', 'world': 'gazebo',
                                            'system': 'Lab', 'commands': 2, 'params': True})
        self.assertEqual(flight['runs'], [])

    def test_prepare_returns_the_recipe_and_kept_parameters(self):
        server = self.server()
        prepared = server.call('prepare', {'recording': SESSION_ID}, None, None)
        self.assertEqual(prepared['recipe']['adapter'], 'px4-gazebo')
        self.assertIn('MPC_XY_VEL_MAX', prepared['params'])
        (self.recording / 'params.params').unlink()
        self.assertIsNone(server.call('prepare', {'recording': SESSION_ID}, None, None)['params'])
        with self.assertRaisesRegex(ValueError, 'recipe'):
            server.call('prepare', {'recording': '20261113T090000Z-plain'}, None, None)
        with self.assertRaises(ValueError):
            server.call('prepare', {'recording': '../etc'}, None, None)
        with self.assertRaisesRegex(ValueError, 'Unknown'):
            server.call('delete', {}, None, None)


class Import(Fixture, unittest.TestCase):
    def export(self):
        folder = self.root.parent / (self.root.name + '-export')
        folder.mkdir()
        recipe = dict(RECIPE, origin='real', adapter='px4-sih', source='ulog',
                      command_topics=['/fmu/in/trajectory_setpoint'],
                      command_types={'/fmu/in/trajectory_setpoint': 'px4_msgs/msg/TrajectorySetpoint'})
        (folder / 'recipe.json').write_text(json.dumps(recipe))
        (folder / 'commands.jsonl').write_text('{}\n')
        (folder / 'evidence.jsonl').write_text('{}\n')
        (folder / 'flight.ulg').write_bytes(b'ULog')
        (folder / 'stray.sh').write_text('rm -rf /')
        self.addCleanup(lambda: [p.unlink() for p in folder.iterdir()] and folder.rmdir())
        return folder

    def test_a_flight_log_export_is_filed_as_a_finalized_flight(self):
        server = self.server()
        result = server.call('import', {'label': 'Field test #3', 'folder': str(self.export())}, None, None)
        path = self.root / result['id']
        self.assertTrue(result['id'].endswith('-field-test-3'))
        self.assertEqual(sorted(p.name for p in path.iterdir()),
                         ['commands.jsonl', 'evidence.jsonl', 'flight.ulg', 'recipe.json', 'session.json'])
        self.assertEqual(json.loads((path / 'session.json').read_text())['status'], 'finalized')
        self.assertEqual((result['recipe']['origin'], result['recipe']['source']), ('real', 'ulog'))
        flight = next(f for f in server.list()['flights'] if f['id'] == result['id'])
        self.assertEqual(flight['label'], 'Field test #3')

    def test_an_mcap_with_commands_is_preferred_over_the_log(self):
        mcap = self.root.parent / (self.root.name + '-drone.mcap')
        mcap.write_bytes(b'mcap')
        self.addCleanup(mcap.unlink)
        topics = lambda path: ['/fmu/in/trajectory_setpoint', '/fmu/out/vehicle_status_v1', '/camera']
        server = self.server()
        both = server.import_flight({'label': 'both', 'folder': str(self.export()), 'mcap': str(mcap)}, topics)
        recipe = json.loads((self.root / both['id'] / 'recipe.json').read_text())
        self.assertEqual((recipe['source'], recipe['evidence_topics']), ('', ['/fmu/out/vehicle_status_v1']))
        self.assertTrue((self.root / both['id'] / 'bag' / mcap.name).is_file())
        alone = server.import_flight({'label': 'drone', 'mcap': str(mcap)}, topics)
        self.assertEqual(alone['recipe']['origin'], 'real')
        with self.assertRaisesRegex(ValueError, r'no PX4 \(/fmu/in/\) or ArduPilot'):
            server.import_flight({'mcap': str(mcap)}, lambda path: ['/camera'])
        with self.assertRaisesRegex(ValueError, 'Choose'):
            server.import_flight({}, topics)

    def test_an_ardupilot_recording_flies_in_ardupilot_sitl(self):
        mcap = self.root.parent / (self.root.name + '-ap.mcap')
        mcap.write_bytes(b'mcap')
        self.addCleanup(mcap.unlink)
        topics = lambda path: ['/ap/cmd_vel', '/ap/status', '/ap/pose/filtered', '/ap/joy', '/camera']
        result = self.server().import_flight({'label': 'ap', 'mcap': str(mcap)}, topics)
        recipe = json.loads((self.root / result['id'] / 'recipe.json').read_text())
        self.assertEqual((recipe['adapter'], recipe['autopilot'], recipe['vehicle']),
                         ('ardupilot-sitl', 'ardupilot', 'copter'))
        self.assertEqual(recipe['command_topics'], ['/ap/cmd_vel', '/ap/joy'])
        self.assertEqual(recipe['evidence_topics'], ['/ap/status', '/ap/pose/filtered'])
        with self.assertRaisesRegex(ValueError, 'different autopilots'):
            self.server().import_flight({'label': 'mixed', 'folder': str(self.export()), 'mcap': str(mcap)}, topics)


class Plan(Fixture, unittest.TestCase):
    def test_the_run_is_isolated_and_records_the_fresh_reports(self):
        plan = resim.run_plan({'recording': SESSION_ID, 'namespace': 'resim', 'domain': 79}, self.root)
        self.assertEqual(plan['env']['ROS_DOMAIN_ID'], '79')
        self.assertEqual(plan['env']['ROS_LOCALHOST_ONLY'], '1')
        self.assertEqual(plan['env']['ROS_AUTOMATIC_DISCOVERY_RANGE'], 'LOCALHOST')
        arbiter_args = plan['commands']['arbiter']
        self.assertEqual(arbiter_args[arbiter_args.index('--namespace') + 1], 'resim')
        self.assertTrue(str(plan['folder']).startswith(str(self.recording / 'resim')))
        self.assertEqual(plan['commands']['record'][-1], '/fmu/out/vehicle_status_v1',
                         'only valid evidence topics are recorded')
        self.assertEqual(resim.variants(['/fmu/out/vehicle_status', '/fmu/out/x_v1', '/det']),
                         ['/fmu/out/vehicle_status', '/fmu/out/vehicle_status_v1', '/fmu/out/vehicle_status_v2',
                          '/fmu/out/vehicle_status_v3', '/fmu/out/x_v1', '/det'])

    def test_the_takeover_settings_reach_the_player(self):
        plan = resim.run_plan({'recording': SESSION_ID, 'tolerance': 10, 'floor': 0.2, 'hold': 0.5,
                               'code': 'avoid-v2'}, self.root)
        args = plan['commands']['arbiter']
        self.assertEqual([args[args.index(k) + 1] for k in ('--tolerance', '--floor', '--hold')], ['10', '0.2', '0.5'])
        off = resim.run_plan({'recording': SESSION_ID}, self.root)['commands']['arbiter']
        self.assertEqual(off[off.index('--tolerance') + 1], 'off')

    def test_the_target_and_conditions_are_recorded(self):
        plan = resim.run_plan({'recording': SESSION_ID, 'target': 'px4-jmavsim', 'conditions': ['wind'],
                               'home': True}, self.root)
        self.assertEqual((plan['target'], plan['conditions'], plan['home']), ('px4-jmavsim', ['wind'], True))
        self.assertEqual(resim.run_plan({'recording': SESSION_ID}, self.root)['target'], 'px4-gazebo',
                         'as recorded')

    def test_bad_parameters_are_refused(self):
        good = {'recording': SESSION_ID}
        for bad in ({'domain': 0}, {'domain': 'x'}, {'namespace': '../x'}, {'skipped_params': 'all'},
                    {'tolerance': 101}, {'tolerance': 'x'}, {'hold': -1}, {'code': '../x'},
                    {'ai_topics': 'all'}, {'ai_topics': ['detections']}, {'target': '../x'},
                    {'conditions': 'wind'}, {'conditions': ['a b']}):
            with self.assertRaises(ValueError, msg=bad):
                resim.run_plan(dict(good, **bad), self.root)
        (self.recording / 'recipe.json').write_text(json.dumps(dict(RECIPE, command_topics=[])))
        with self.assertRaisesRegex(ValueError, 'no ROS 2 commands'):
            resim.run_plan(good, self.root)


class ArduPilotPlan(Fixture, unittest.TestCase):
    def setUp(self):
        super().setUp()
        recipe = dict(RECIPE, adapter='ardupilot-sitl', autopilot='ardupilot', vehicle='copter',
                      world={'id': 'internal', 'params': {}}, command_topics=['/ap/cmd_vel'],
                      evidence_topics=['/ap/status', '/ap/pose/filtered'])
        (self.recording / 'recipe.json').write_text(json.dumps(recipe))

    def test_the_player_joins_the_vehicle_domain_to_the_run(self):
        plan = resim.run_plan({'recording': SESSION_ID, 'domain': 79, 'vehicle_domain': 80}, self.root)
        args = plan['commands']['arbiter']
        self.assertEqual(args[args.index('--autopilot') + 1], 'ardupilot')
        self.assertEqual(args[args.index('--vehicle-domain') + 1], '80')
        self.assertEqual(args[args.index('--wait') + 1], str(resim.AP_WAIT_S), 'SITL settles before it can arm')
        self.assertEqual(plan['env']['ROS_DOMAIN_ID'], '79', 'your code and the recorder stay on the run domain')
        self.assertEqual(plan['commands']['record'][-2:], ['/ap/status', '/ap/pose/filtered'])
        self.assertEqual((plan['autopilot'], plan['vehicle_domain']), ('ardupilot', 80))

    def test_the_vehicle_needs_a_domain_of_its_own(self):
        for bad in ({}, {'vehicle_domain': 79}, {'vehicle_domain': 300}, {'vehicle_domain': 'x'}):
            with self.assertRaisesRegex(ValueError, 'domain of its own', msg=bad):
                resim.run_plan(dict({'recording': SESSION_ID, 'domain': 79}, **bad), self.root)
        (self.recording / 'recipe.json').write_text(json.dumps(RECIPE))
        px4 = resim.run_plan({'recording': SESSION_ID, 'domain': 79, 'vehicle_domain': 80}, self.root)
        self.assertNotIn('--vehicle-domain', px4['commands']['arbiter'], 'PX4 keeps its namespace instead')
        self.assertEqual(px4['commands']['arbiter'][px4['commands']['arbiter'].index('--autopilot') + 1], 'px4')


class Running(Fixture, unittest.TestCase):
    def run_once(self, processes, cancel=None, **params):
        server = self.server(processes)
        messages = []
        request = dict({'recording': SESSION_ID, 'domain': 79,
                        'skipped_params': [{'name': 'CAL_ACC0_XOFF', 'reason': 'hardware'}]}, **params)
        return server, messages, server.call('run', request, cancel or threading.Event(), messages.append)

    def test_a_completed_run_keeps_its_record(self):
        status = {'commands': 40, 'sent': 40, 'anchored': True, 't': 12.5, 'before_anchor': 2}
        processes = Processes(folder_status=status)
        server, messages, run = self.run_once(processes)
        self.assertEqual(run['status'], 'Completed')
        self.assertEqual((run['commands'], run['sent'], run['before_anchor']), (40, 40, 2))
        self.assertEqual([k for k, _, _ in processes.started], [resim.KEYS['record'], resim.KEYS['arbiter']])
        self.assertEqual(processes.stopped, [resim.KEYS['arbiter'], resim.KEYS['record']])
        saved = json.loads((Path(run['folder']) / 'run.json').read_text())
        self.assertEqual(saved['skipped_params'], [{'name': 'CAL_ACC0_XOFF', 'reason': 'hardware'}])
        self.assertEqual((saved['tolerance'], saved['steering']), ('off', 'recording'))
        self.assertIn('40 of 40 commands replayed · 12.5 s', messages)
        self.assertFalse(server.status()['active'])
        flight = server.list()['flights'][0]
        self.assertEqual([(r['status'], r['sent']) for r in flight['runs']], [('Completed', 40)])

    def test_a_takeover_is_recorded(self):
        taken = {'t': 42.3, 'topic': '/fmu/in/trajectory_setpoint', 'reason': 'velocity[0]: recorded 2, yours 2.3.'}
        status = {'commands': 40, 'sent': 30, 'anchored': True, 't': 50.0, 'steering': 'your code',
                  'compared': 30, 'differed': 6, 'takeover': taken}
        server, messages, run = self.run_once(Processes(folder_status=status), tolerance=10, code='avoid-v2')
        self.assertEqual((run['steering'], run['takeover'], run['tolerance'], run['code']),
                         ('your code', taken, 10.0, 'avoid-v2'))
        self.assertIn('Your code steering since 42.3 s · 50.0 s', messages)
        row = server.list()['flights'][0]['runs'][0]
        self.assertEqual(row['code'], 'avoid-v2')
        self.assertTrue(row['takeover'].startswith('42.3 s, /fmu/in/trajectory_setpoint'))

    def test_a_finished_run_is_compared_and_its_report_returned(self):
        calls = []

        def comparer(recording, folder):
            calls.append((recording, folder))
            (folder / 'compare.json').write_text(json.dumps({'schema': 1, 'verdict': 'changed'}))
            return {'verdict': 'changed', 'split_t': 12.5}

        processes = Processes(folder_status={'commands': 4, 'sent': 4, 'anchored': True})
        original = processes.run

        def run(key, bash, env=None, cwd=None, readiness=None):
            if key == resim.KEYS['record']:
                fresh = Path(bash.split("-o ")[1].split(' ')[0].strip("'"))
                fresh.mkdir(parents=True)
                (fresh / 'fresh_0.mcap').write_bytes(b'mcap')
            return original(key, bash, env, cwd, readiness)
        processes.run = run
        server = resim.Resim(None, processes, root=str(self.root), comparer=comparer)
        result = server.call('run', {'recording': SESSION_ID, 'ai_topics': ['/drone_deck/detections']},
                             threading.Event(), lambda m: None)
        self.assertEqual((result['verdict'], result['split_t']), ('changed', 12.5))
        self.assertEqual(len(calls), 1)
        record = next(bash for key, bash, _ in processes.started if key == resim.KEYS['record'])
        self.assertTrue(record.endswith('/fmu/out/vehicle_status_v1 /drone_deck/detections'), record)
        name = Path(result['folder']).name
        self.assertEqual(server.call('report', {'recording': SESSION_ID, 'run': name}, None, None)['verdict'], 'changed')
        row = server.list()['flights'][0]['runs'][0]
        self.assertEqual((row['verdict'], row['split_t']), ('changed', 12.5))

        def broken(recording, folder):
            raise RuntimeError('no reader')
        server.comparer = broken
        again = server.call('compare', {'recording': SESSION_ID, 'run': name}, None, None)
        self.assertEqual(again['compare_error'], 'no reader')
        with self.assertRaisesRegex(ValueError, 'Select a run'):
            server.call('report', {'recording': SESSION_ID, 'run': '../x'}, None, None)

    def test_a_player_that_fails_fails_the_run(self):
        status = {'commands': 3, 'sent': 0, 'anchored': False, 'error': 'No vehicle status from the fresh autopilot'}
        with self.assertRaisesRegex(RuntimeError, 'code 3: No vehicle status'):
            self.run_once(Processes(code=3, folder_status=status))
        run = json.loads(next((self.recording / 'resim').glob('*/run.json')).read_text())
        self.assertEqual(run['status'], 'Failed')

    def test_cancel_stops_both_processes(self):
        processes, cancel = Processes(run_s=None), threading.Event()
        threading.Timer(0.3, cancel.set).start()
        _, _, run = self.run_once(processes, cancel)
        self.assertEqual(run['status'], 'Cancelled')
        self.assertEqual(sorted(processes.stopped), sorted(resim.KEYS.values()))

    def test_one_run_at_a_time(self):
        server = self.server(Processes(run_s=None))
        cancel = threading.Event()
        worker = threading.Thread(target=server.call, args=('run', {'recording': SESSION_ID}, cancel, lambda m: None))
        worker.start()
        try:
            for _ in range(50):
                if server.status()['active']:
                    break
                cancel.wait(0.02)
            with self.assertRaisesRegex(RuntimeError, 'already running'):
                server.call('run', {'recording': SESSION_ID}, threading.Event(), lambda m: None)
            self.assertEqual(server.list()['flights'][0]['runs'][0]['status'], 'Running')
        finally:
            cancel.set()
            worker.join(5)


class PlayerStatus(unittest.TestCase):
    def test_the_status_says_how_far_the_replay_is(self):
        player = arbiter.Player([(0, '/a', {}), (2_000_000, '/b', {})])
        clock = arbiter.FreshClock()
        value = arbiter.status(player, clock, 0.0, 1)
        self.assertEqual((value['anchored'], value['t'], value['sent'], value['commands']), (False, 0.0, 0, 2))
        clock.observe(1_000_000, 0.0)
        player.due(clock, 0.0)
        value = arbiter.status(player, clock, 0.1, 1, 'boom')
        self.assertEqual((value['anchored'], value['t'], value['sent'], value['error']), (True, 0.1, 1, 'boom'))
        self.assertEqual(resim.progress_text(value), '1 of 2 commands replayed · 0.1 s')
        self.assertIn('Waiting', resim.progress_text({}))

    def test_the_status_file_is_replaced_whole(self):
        with tempfile.TemporaryDirectory() as folder:
            arbiter.write_status(Path(folder), {'sent': 3})
            self.assertEqual(json.loads((Path(folder) / 'status.json').read_text()), {'sent': 3})
            self.assertFalse((Path(folder) / 'status.json.tmp').exists())


if __name__ == '__main__':
    unittest.main()
