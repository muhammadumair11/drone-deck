"""The ArduPilot command player without ROS: ArduPilot's time stamps, the clock base switch, the
calls worked out from /ap/status, the schedule and the shadow answers."""
import unittest

import arbiter
import arbiter_ap as ap
from takeover import Takeover

S = 1_000_000_000  # one second in log time (ns)
UTC = 1_700_000_000  # seconds: the clock ArduPilot keeps once GPS set it


def header(sec, nanosec=0):
    return {'stamp': {'sec': sec, 'nanosec': nanosec}, 'frame_id': ''}


def status(log_ns, sec, mode=0, armed=False, flying=False, failsafe=(), vehicle_type=2):
    return (ap.STATUS, log_ns, {'header': header(sec), 'vehicle_type': vehicle_type, 'armed': armed,
                                'mode': mode, 'flying': flying, 'external_control': True,
                                'failsafe': list(failsafe)})


def pose(log_ns, sec, z):
    return (ap.POSE, log_ns, {'header': header(sec), 'pose': {'position': {'x': 0.0, 'y': 0.0, 'z': z},
                                                              'orientation': {'w': 1.0}}})


def cmd_vel(log_ns, sec, vx):
    return ('/ap/cmd_vel', log_ns, {'header': header(sec), 'twist': {'linear': {'x': vx, 'y': 0.0, 'z': 0.0},
                                                                    'angular': {'x': 0.0, 'y': 0.0, 'z': 0.0}}})


class Stamps(unittest.TestCase):
    def test_px4_timestamps_ros_headers_and_bare_times(self):
        self.assertEqual(arbiter.stamp_us({'timestamp': 42}), 42)
        self.assertEqual(arbiter.stamp_us({'header': header(3, 500_000)}), 3_000_500)
        self.assertEqual(arbiter.stamp_us({'sec': 2, 'nanosec': 1000}), 2_000_001)
        self.assertIsNone(arbiter.stamp_us({'header': header(0)}))
        self.assertIsNone(arbiter.stamp_us({'data': 1}))
        self.assertTrue(arbiter.is_anchor('/ap/status'))
        self.assertFalse(arbiter.is_anchor('/ap/time'))

    def test_the_clock_base_switch_from_boot_time_to_utc_starts_a_new_base(self):
        pairs = [(1 * S, 1_000_000), (2 * S, 2_000_000), (3 * S, UTC * 1_000_000), (4 * S, (UTC + 1) * 1_000_000)]
        self.assertEqual(arbiter.final_base(pairs), 3 * S)
        stalled = [(1 * S, 1_000_000), (20 * S, 2_000_000), (21 * S, 3_000_000)]
        self.assertEqual(arbiter.final_base(stalled), 1 * S, 'a stalled simulator is not a new base')
        self.assertIsNone(arbiter.final_base([]))

    def test_a_ros_header_is_restamped_on_the_fresh_clock(self):
        message = {'header': header(5, 0), 'twist': {'linear': {'x': 1.0}}}
        out = arbiter.rewrite(message, 7_250_000)
        self.assertEqual(out['header']['stamp'], {'sec': 7, 'nanosec': 250_000_000})
        self.assertEqual(message['header']['stamp']['sec'], 5, 'the recorded message is not changed')
        self.assertEqual(arbiter.rewrite({'arm': True}, 9), {'arm': True}, 'a service request has no stamp')


class Derived(unittest.TestCase):
    def test_mode_arming_and_takeoff_are_worked_out_from_the_status(self):
        statuses = [status(0, UTC), status(1 * S, UTC + 1, mode=4), status(2 * S, UTC + 2, mode=4, armed=True),
                    status(3 * S, UTC + 3, mode=4, armed=True, flying=True),
                    status(20 * S, UTC + 20, mode=9, armed=True, flying=True),
                    status(30 * S, UTC + 30, mode=9, armed=False)]
        poses = [pose(4 * S, UTC + 4, 4.0), pose(6 * S, UTC + 6, 9.96), pose(9 * S, UTC + 9, 30.0)]
        commands = [cmd_vel(8 * S, UTC + 8, 1.0)]
        calls = ap.derive_calls(statuses, poses, commands)
        self.assertEqual(calls, [(0, ap.MODE, {'mode': 0}), (1 * S, ap.MODE, {'mode': 4}),
                                 (2 * S, ap.ARM, {'arm': True}), (3 * S, ap.TAKEOFF, {'alt': 10.0}),
                                 (20 * S, ap.MODE, {'mode': 9})])

    def test_a_failsafe_changes_nothing_and_disarming_is_left_to_the_vehicle(self):
        statuses = [status(0, UTC, mode=4, armed=True, flying=True),
                    status(1 * S, UTC + 1, mode=6, armed=True, flying=True, failsafe=[21]),
                    status(2 * S, UTC + 2, mode=6, armed=False, flying=False)]
        calls = ap.derive_calls(statuses, [pose(0, UTC, 12.0)])
        self.assertEqual([c[1] for c in calls], [ap.MODE, ap.ARM, ap.TAKEOFF],
                         'a recording started in flight arms and takes off at once')
        self.assertEqual(calls[2][2], {'alt': 12.0})

    def test_takeoff_needs_a_copter_in_guided_and_a_pose(self):
        plane = [status(0, UTC, mode=15, vehicle_type=3), status(1 * S, UTC + 1, mode=15, armed=True, vehicle_type=3),
                 status(2 * S, UTC + 2, mode=15, armed=True, flying=True, vehicle_type=3)]
        self.assertNotIn(ap.TAKEOFF, [c[1] for c in ap.derive_calls(plane)])
        copter = [status(0, UTC, mode=4, armed=True), status(1 * S, UTC + 1, mode=4, armed=True, flying=True)]
        self.assertEqual(ap.derive_calls(copter)[-1], (1 * S, ap.TAKEOFF, {'alt': ap.DEFAULT_TAKEOFF_M}))


class Schedule(unittest.TestCase):
    def test_commands_and_calls_are_timed_from_the_first_status_on_the_utc_clock(self):
        # Booted at log 0 (time since boot), clock set from GPS at log 10 s.
        reports = [status(0, 1), status(5 * S, 5), status(10 * S, UTC, mode=0), status(11 * S, UTC + 1, mode=4),
                   status(12 * S, UTC + 2, mode=4, armed=True)]
        reports += [('/ap/time', t * S, {'sec': UTC + t - 10, 'nanosec': 0}) for t in range(10, 20)]
        recorded = [cmd_vel(15 * S, UTC + 5, 2.0)]
        schedule, early, derived = ap.schedule_for(recorded, reports)
        self.assertEqual(early, 0)
        self.assertEqual(derived, 3, 'mode at the first UTC status, then Guided and arming')
        self.assertEqual([(offset, topic) for offset, topic, _ in schedule],
                         [(0, ap.MODE), (1_000_000, ap.MODE), (2_000_000, ap.ARM), (5_000_000, '/ap/cmd_vel')])

    def test_a_flight_from_its_log_brings_its_calls_and_is_read_from_its_lines(self):
        import json
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            calls = [{'topic': ap.ARM, 't_ns': 11 * S, 'message': {'arm': True}},
                     {'topic': '/ap/cmd_vel', 't_ns': 12 * S, 'message': cmd_vel(0, UTC + 2, 1.0)[2]}]
            (folder / 'commands.jsonl').write_text(''.join(json.dumps(c) + '\n' for c in calls))
            evidence = {'topic': ap.STATUS, 't_ns': 10 * S, 'message': status(0, 10, mode=4)[2]}
            (folder / 'evidence.jsonl').write_text(json.dumps(evidence) + '\n')
            recipe = {'source': 'dataflash', 'command_topics': [ap.ARM, '/ap/cmd_vel'], 'evidence_topics': [ap.STATUS],
                      'command_types': {'/ap/cmd_vel': 'geometry_msgs/msg/TwistStamped'}}
            types, recorded, reports = arbiter.read_recording(folder, recipe)
        self.assertEqual(types, {'/ap/cmd_vel': 'geometry_msgs/msg/TwistStamped'})
        schedule, early, derived = ap.schedule_for(recorded, reports, derive=False)
        self.assertEqual((early, derived), (0, 0), 'the log already says which calls were made')
        self.assertEqual([(offset, topic) for offset, topic, _ in schedule],
                         [(1_000_000, ap.ARM), (2_000_000, '/ap/cmd_vel')])

    def test_a_recording_without_a_status_is_refused(self):
        with self.assertRaisesRegex(ValueError, '/ap/status'):
            ap.schedule_for([cmd_vel(1 * S, UTC, 1.0)], [('/ap/time', S, {'sec': UTC, 'nanosec': 0})])
        with self.assertRaisesRegex(ValueError, 'time stamps'):
            ap.schedule_for([cmd_vel(1 * S, UTC, 1.0)], [])


class Shadow(unittest.TestCase):
    def test_shadow_answers_say_what_the_fresh_vehicle_did(self):
        fresh = {'armed': True, 'mode': 4, 'flying': False}
        self.assertEqual(ap.shadow_answer(ap.ARM, {'arm': True}, fresh), {'result': True})
        self.assertEqual(ap.shadow_answer(ap.MODE, {'mode': 9}, fresh), {'status': False, 'curr_mode': 4})
        self.assertEqual(ap.shadow_answer(ap.TAKEOFF, {'alt': 5.0}, fresh), {'status': False})
        self.assertEqual(ap.qos_kind(ap.STATUS), 'latched')
        self.assertEqual(ap.qos_kind('/ap/time'), 'reliable')
        self.assertEqual(ap.qos_kind('/ap/pose/filtered'), 'stream')
        self.assertTrue(ap.is_command('/ap/cmd_vel') and ap.is_command('/ap/joy'))
        self.assertFalse(ap.is_command('/ap/status'))

    def test_your_calls_match_the_recorded_ones_within_their_window(self):
        judge = Takeover(0.1, 0.2, 500_000, grace_us=0)
        anchor = 1_000_000
        judge.observe(ap.ARM, {'arm': True}, anchor + 100_000)
        self.assertIsNone(judge.judge(ap.ARM, {'arm': True}, anchor + 1_500_000, anchor), 'your arm came first')
        self.assertIsNone(judge.judge(ap.MODE, {'mode': 4}, anchor + 2_000_000, anchor))
        judge.observe(ap.MODE, {'mode': 4}, anchor + 3_000_000)
        self.assertIsNone(judge.tick(anchor + 5_000_000, anchor), 'yours came within 2 s')
        self.assertIsNone(judge.judge(ap.TAKEOFF, {'alt': 10.0}, anchor + 6_000_000, anchor))
        self.assertIsNone(judge.tick(anchor + 8_900_000, anchor))
        taken = judge.tick(anchor + 9_000_000, anchor)
        self.assertEqual(taken['topic'], ap.TAKEOFF)
        self.assertIn('within 3 s', taken['reason'])

    def test_twist_headers_are_not_compared(self):
        judge = Takeover(0.1, 0.0, 0, grace_us=0)
        recorded = cmd_vel(0, UTC, 2.0)[2]
        yours = cmd_vel(0, UTC + 30, 2.05)[2]
        judge.observe('/ap/cmd_vel', yours, 10)
        self.assertIsNone(judge.judge('/ap/cmd_vel', recorded, 20, 0))


if __name__ == '__main__':
    unittest.main()
