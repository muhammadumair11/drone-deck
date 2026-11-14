"""compare.* without ROS: one clock for both flights, the path verdict, events, commands and AI."""
import json
import unittest

import compare

STATUS = '/fmu/out/vehicle_status_v1'
POSITION = '/fmu/out/vehicle_local_position'


def flight(anchor_us, xs, start_log=0, dt=0.1, modes=None):
    """Rows of a flight along x: status at the anchor (and mode changes), positions every `dt`."""
    rows = [(STATUS, start_log, {'timestamp': anchor_us, 'arming_state': 1, 'nav_state': 4, 'failsafe': False})]
    for i, x in enumerate(xs):
        stamp = anchor_us + int(i * dt * 1e6)
        rows.append((POSITION, start_log + int(i * dt * 1e9), {'timestamp': stamp, 'x': x, 'y': 0.0, 'z': -2.0}))
    for t, arming, nav in modes or []:
        rows.append((STATUS, start_log + int(t * 1e9),
                     {'timestamp': anchor_us + int(t * 1e6), 'arming_state': arming, 'nav_state': nav, 'failsafe': False}))
    return rows


class Clock(unittest.TestCase):
    def test_each_flight_is_measured_from_its_own_first_status(self):
        rows = flight(5_000_000, [0.0, 1.0], start_log=7)
        self.assertEqual(compare.anchors(rows), (5_000_000, 7))
        self.assertEqual(compare.positions(rows, 5_000_000), [(0.0, 0.0, 0.0, -2.0), (0.1, 1.0, 0.0, -2.0)])
        self.assertEqual(compare.anchors([(POSITION, 0, {'timestamp': 1})]), (None, None))


class Path(unittest.TestCase):
    def test_the_same_flight_keeps_its_course(self):
        xs = [i * 0.5 for i in range(50)]
        result = compare.path(compare.positions(flight(1_000_000, xs), 1_000_000),
                              compare.positions(flight(9_000_000, [x + 0.3 for x in xs]), 9_000_000), 0.10)
        self.assertEqual(result['verdict'], 'same')
        self.assertIsNone(result['split_t'])
        self.assertAlmostEqual(result['max_m'], 0.3)
        self.assertAlmostEqual(result['mean_m'], 0.3)

    def test_a_path_that_drifts_changes_where_it_leaves_the_tolerance(self):
        xs = [i * 1.0 for i in range(40)]           # 10 m/s along x
        drift = [x + max(0.0, i - 20) * 0.5 for i, x in enumerate(xs)]   # from 2.0 s, 5 m/s more
        old, new = compare.positions(flight(0 + 1, xs), 1), compare.positions(flight(1, drift), 1)
        tight = compare.path(old, new, 0.10)
        self.assertEqual(tight['verdict'], 'changed')
        # At 2.5 s: 2.5 m apart, allowed max(10% of 25 m, 1 m) = 2.5 m; at 2.6 s 3.0 > 2.6.
        self.assertEqual(tight['split_t'], 2.6)
        loose = compare.path(old, new, 0.50)
        self.assertEqual(loose['verdict'], 'same', 'a looser tolerance keeps the same course')
        self.assertEqual(compare.path(old, [], 0.1)['verdict'], 'no data')

    def test_plots_are_thinned(self):
        xs = [float(i) for i in range(2000)]
        result = compare.path(compare.positions(flight(1, xs, dt=0.01), 1),
                              compare.positions(flight(1, xs, dt=0.01), 1), 0.1)
        self.assertLessEqual(len(result['old']), compare.PLOT_POINTS)
        self.assertLessEqual(len(result['distance']), compare.PLOT_POINTS)


class Events(unittest.TestCase):
    def test_changes_are_paired_with_their_shift(self):
        old = compare.events(flight(1, [0.0], modes=[(1.0, 2, 4), (2.0, 2, 14), (9.0, 1, 18)]), 1)
        self.assertEqual([(e['kind'], e['value'], e['t']) for e in old],
                         [('arming', 'Disarmed', 0.0), ('mode', 'Hold', 0.0), ('arming', 'Armed', 1.0),
                          ('mode', 'Offboard', 2.0), ('arming', 'Disarmed', 9.0), ('mode', 'Land', 9.0)])
        new = compare.events(flight(1, [0.0], modes=[(1.2, 2, 4), (2.1, 2, 14), (3.0, 2, 5)]), 1)
        diff = compare.event_diff(old, new)
        offboard = next(r for r in diff['rows'] if r['value'] == 'Offboard')
        self.assertEqual(offboard['shift'], 0.1)
        self.assertEqual((diff['missing'], diff['extra']), (2, 1), 'no disarm or land; an extra Return')

    def test_failsafe_and_landing(self):
        rows = [(STATUS, 0, {'timestamp': 1, 'arming_state': 2, 'nav_state': 14, 'failsafe': False}),
                (STATUS, 1, {'timestamp': 2_000_001, 'arming_state': 2, 'nav_state': 5, 'failsafe': True}),
                ('/fmu/out/vehicle_land_detected', 2, {'timestamp': 3_000_001, 'landed': True})]
        kinds = [(e['kind'], e['value'], e['t']) for e in compare.events(rows, 1)]
        self.assertIn(('failsafe', 'On', 2.0), kinds)
        self.assertIn(('landed', 'Landed', 3.0), kinds)
        self.assertNotIn(('failsafe', 'Off', 0.0), kinds, 'no failsafe is not an event')


class Commands(unittest.TestCase):
    def test_counts_by_source_and_the_takeover(self):
        recorded = [('/fmu/in/trajectory_setpoint', 0, {})] * 3 + [('/fmu/in/vehicle_command', 0, {})]
        lines = [json.dumps({'t': 0.1, 'topic': '/fmu/in/trajectory_setpoint', 'source': 'recording'}),
                 json.dumps({'t': 0.2, 'topic': '/fmu/in/trajectory_setpoint', 'source': 'your code'}),
                 json.dumps({'t': 0.2, 'takeover': {'t': 0.2}}), 'not json']
        result = compare.commands(recorded, lines, {'takeover': {'t': 0.2, 'topic': 'x'}, 'steering': 'your code'})
        setpoints = result['topics'][0]
        self.assertEqual((setpoints['recorded'], setpoints['from_recording'], setpoints['from_your_code']), (3, 1, 1))
        self.assertEqual(result['takeover']['t'], 0.2)


def detection(index):
    return {'data': json.dumps({'outputs': [{'name': 'scores', 'top': [[index, 0.9], [1, 0.05]]}]})}


class AI(unittest.TestCase):
    def test_top_labels_agree_at_the_same_moment(self):
        old = [('/det', 1_000_000_000 + i * 100_000_000, detection(7 if i < 8 else 3)) for i in range(10)]
        new = [('/det', 5_000_000_000 + i * 100_000_000 + 20_000_000, detection(7)) for i in range(10)]
        result = compare.ai('/det', old, new, 1_000_000_000, 5_000_000_000)
        self.assertEqual((result['old'], result['new'], result['paired'], result['agree']), (10, 10, 10, 8))
        self.assertEqual(result['disagreements'][0], {'t': 0.8, 'old': 3, 'new': 7})
        self.assertEqual(compare.top_label({'data': json.dumps({'outputs': [{'values': [0.1, 0.7, 0.2]}]})}), 1)
        self.assertIsNone(compare.top_label({'data': 'not json'}))


class Report(unittest.TestCase):
    def test_a_whole_report(self):
        xs = [i * 0.5 for i in range(30)]
        value = compare.report(flight(1, xs), flight(9, xs), [], [], {'tolerance': 20.0, 'takeover': {'t': 1.5}}, [])
        self.assertEqual((value['verdict'], value['tolerance'], value['takeover_t']), ('same', 0.2, 1.5))
        off = compare.report(flight(1, xs), [], [], [], {'tolerance': 'off'}, ['/det'])
        self.assertEqual((off['verdict'], off['tolerance']), ('no data', compare.DEFAULT_TOLERANCE))
        self.assertTrue(off['notes'])
        self.assertEqual(off['ai'][0]['old'], 0)


UTC_S = 1_700_000_000


def ap_flight(start_sec, easts, boot_rows=0, dt=0.1, statuses=()):
    """An ArduPilot flight: its status, then its ENU pose flying east every `dt`; optional rows
    stamped with the time since boot before its clock was set."""
    def header(us):
        return {'stamp': {'sec': us // 1_000_000, 'nanosec': (us % 1_000_000) * 1000}, 'frame_id': 'map'}
    rows = [(compare.AP_STATUS, i * 10**8, {'header': header(1_000_000 + i * 100_000), 'vehicle_type': 2, 'armed': False,
                                            'mode': 0, 'flying': False, 'failsafe': []}) for i in range(boot_rows)]
    log0, us0 = 10**10, start_sec * 1_000_000
    rows.append((compare.AP_STATUS, log0, {'header': header(us0), 'vehicle_type': 2, 'armed': False, 'mode': 4,
                                           'flying': False, 'failsafe': []}))
    for i, east in enumerate(easts):
        us = us0 + int(i * dt * 1e6)
        rows.append((compare.AP_POSE, log0 + int(i * dt * 1e9),
                     {'header': header(us), 'pose': {'position': {'x': east, 'y': 0.0, 'z': 2.0}}}))
    for t, armed, mode, flying, failsafe in statuses:
        rows.append((compare.AP_STATUS, log0 + int(t * 1e9),
                     {'header': header(us0 + int(t * 1e6)), 'vehicle_type': 2, 'armed': armed, 'mode': mode,
                      'flying': flying, 'failsafe': failsafe}))
    return rows


class ArduPilot(unittest.TestCase):
    def test_pose_is_turned_to_north_east_down_from_the_utc_status(self):
        rows = compare.in_final_base(ap_flight(UTC_S, [0.0, 1.0], boot_rows=3))
        self.assertEqual(len(rows), 3, 'the reports stamped before the clock was set are left out')
        anchor, _ = compare.anchors(rows)
        self.assertEqual(anchor, UTC_S * 1_000_000)
        self.assertEqual(compare.positions(rows, anchor), [(0.0, 0.0, 0.0, -2.0), (0.1, 0.0, 1.0, -2.0)])

    def test_status_changes_are_events_with_copter_mode_names(self):
        rows = ap_flight(UTC_S, [], statuses=[(1.0, True, 4, False, []), (2.0, True, 4, True, []),
                                              (9.0, True, 6, True, [24]), (12.0, False, 6, False, [])])
        found = [(e['t'], e['kind'], e['value']) for e in compare.events(rows, UTC_S * 1_000_000)]
        self.assertEqual(found, [(0.0, 'arming', 'Disarmed'), (0.0, 'mode', 'Guided'), (0.0, 'landed', 'Landed'),
                                 (1.0, 'arming', 'Armed'), (2.0, 'landed', 'Flying'), (9.0, 'mode', 'RTL'),
                                 (9.0, 'failsafe', 'On'), (12.0, 'arming', 'Disarmed'), (12.0, 'failsafe', 'Off'),
                                 (12.0, 'landed', 'Landed')])

    def test_an_unchanged_ardupilot_replay_keeps_its_course(self):
        easts = [i * 0.5 for i in range(40)]
        old = ap_flight(UTC_S, easts, boot_rows=2)
        new = ap_flight(UTC_S + 600, [e + 0.2 for e in easts])
        value = compare.report(old, new, [], [], {'tolerance': 10}, [])
        self.assertEqual(value['verdict'], 'same', value['notes'])
        self.assertAlmostEqual(value['path']['max_m'], 0.2)


class Notes(unittest.TestCase):
    def test_what_the_run_changed_on_purpose_is_noted(self):
        notes = compare.setting_notes({'target': 'px4-jmavsim', 'recorded_adapter': 'px4-gazebo',
                                       'conditions': ['wind'], 'home': False})
        self.assertEqual(len(notes), 3)
        self.assertIn('another simulator (px4-jmavsim, recorded in px4-gazebo)', notes[0])
        self.assertIn('wind', notes[1])
        self.assertEqual(compare.setting_notes({'target': 'px4-sih', 'recorded_adapter': 'px4-sih', 'home': True}), [])


if __name__ == '__main__':
    unittest.main()
