"""arbiter.* without ROS: clock mapping, the anchor, the schedule, timestamp rewriting and the
player against a scripted fresh clock."""
import json
import unittest

import arbiter


def status(log_ns, stamp_us, topic='/fmu/out/vehicle_status_v1'):
    return (topic, log_ns, {'timestamp': stamp_us, 'nav_state': 14})


def setpoint(log_ns, x, topic='/fmu/in/trajectory_setpoint'):
    return (topic, log_ns, {'timestamp': 1, 'position': [x, 0.0, -2.0]})


class Topics(unittest.TestCase):
    def test_namespaces_and_the_anchor(self):
        self.assertEqual(arbiter.namespaced('/fmu/in/vehicle_command', 'resim'), '/resim/fmu/in/vehicle_command')
        self.assertEqual(arbiter.unnamespaced('/resim/fmu/out/vehicle_status_v1', 'resim'), '/fmu/out/vehicle_status_v1')
        self.assertIsNone(arbiter.unnamespaced('/fmu/out/vehicle_status_v1', 'resim'))
        self.assertTrue(arbiter.is_anchor('/fmu/out/vehicle_status_v1'))
        self.assertTrue(arbiter.is_anchor('/fmu/out/vehicle_status'))
        self.assertFalse(arbiter.is_anchor('/fmu/out/vehicle_status_flags'))


class Clock(unittest.TestCase):
    def test_log_time_maps_through_the_autopilot_timestamps(self):
        # The simulator ran at half speed between the two reports.
        clock = arbiter.ClockMap([(1_000_000_000, 10_000_000), (3_000_000_000, 11_000_000)])
        self.assertEqual(clock.autopilot_us(2_000_000_000), 10_500_000)
        self.assertEqual(clock.autopilot_us(500_000_000), 9_500_000, 'real time before the first pair')
        self.assertEqual(clock.autopilot_us(4_000_000_000), 12_000_000, 'real time after the last')

    def test_pairs_that_go_back_are_dropped(self):
        clock = arbiter.ClockMap([(1, 100), (2, 50), (3, 300), (3, 400), (0, 0)])
        self.assertEqual((clock.log, clock.ap), ([1, 3], [100, 300]))
        with self.assertRaisesRegex(ValueError, 'timestamps'):
            arbiter.ClockMap([]).autopilot_us(1)


class Schedule(unittest.TestCase):
    def test_commands_are_timed_from_the_first_vehicle_status(self):
        evidence = [status(2_000_000_000, 20_000_000), status(4_000_000_000, 22_000_000),
                    ('/fmu/out/vehicle_local_position', 1_000_000_000, {'timestamp': 19_000_000})]
        commands = [setpoint(3_000_000_000, 1.0), setpoint(1_500_000_000, 0.5),
                    setpoint(2_500_000_000, 0.8), ('/fmu/in/vehicle_command', 2_500_000_000, {'command': 400})]
        schedule, early = arbiter.build_schedule(commands, evidence)
        self.assertEqual(early, 1, 'the command sent before the anchor is not replayed')
        self.assertEqual([(o, t) for o, t, _ in schedule],
                         [(500_000, '/fmu/in/trajectory_setpoint'), (500_000, '/fmu/in/vehicle_command'),
                          (1_000_000, '/fmu/in/trajectory_setpoint')],
                         'ordered by time, recorded order among equal times')

    def test_a_recording_without_a_vehicle_status_is_refused(self):
        with self.assertRaisesRegex(ValueError, 'vehicle status'):
            arbiter.build_schedule([setpoint(1, 0.0)], [('/fmu/out/vehicle_odometry', 1, {'timestamp': 5})])

    def test_rewrite_keeps_the_sample_distance(self):
        message = {'timestamp': 1_000, 'timestamp_sample': 900, 'x': 1}
        out = arbiter.rewrite(message, 50_000)
        self.assertEqual(out, {'timestamp': 50_000, 'timestamp_sample': 49_900, 'x': 1})
        self.assertEqual(message['timestamp'], 1_000, 'the recorded message is not changed')


class Playing(unittest.TestCase):
    def test_nothing_is_published_before_the_fresh_anchor(self):
        player = arbiter.Player([(0, '/fmu/in/a', {'timestamp': 1})])
        clock = arbiter.FreshClock()
        self.assertEqual(player.due(clock, 10.0), [])
        clock.observe(0, 10.0)
        self.assertEqual(player.due(clock, 10.0), [], 'a zero timestamp is not a report')
        clock.observe(5_000_000, 10.0)
        due = player.due(clock, 10.0)
        self.assertEqual([(t, m['timestamp']) for t, m, _ in due], [('/fmu/in/a', 5_000_000)])
        self.assertTrue(player.done)

    def test_commands_follow_the_fresh_autopilot_not_the_wall_clock(self):
        schedule = [(0, '/a', {}), (100_000, '/b', {}), (1_000_000, '/c', {})]
        player, clock = arbiter.Player(schedule), arbiter.FreshClock()
        clock.observe(5_000_000, 0.0)
        self.assertEqual([t for t, _, _ in player.due(clock, 0.05)], ['/a'])
        self.assertEqual([t for t, _, _ in player.due(clock, 0.15)], ['/b'])
        # The simulator stalls: wall time passes, the autopilot reports nothing new.
        self.assertEqual(player.due(clock, 5.0), [], 'no more than 0.2 s past the last report')
        clock.observe(5_900_000, 5.0)
        self.assertEqual(player.due(clock, 5.05), [])
        due = player.due(clock, 5.1)
        self.assertEqual([(t, m['timestamp']) for t, m, _ in due], [('/c', 6_000_000)])
        self.assertTrue(player.done)

    def test_a_report_from_the_past_does_not_move_the_clock_back(self):
        clock = arbiter.FreshClock()
        clock.observe(2_000_000, 1.0)
        clock.observe(1_000_000, 1.1)
        self.assertEqual(clock.now_us(1.1), 2_100_000)
        self.assertEqual(clock.anchor, 2_000_000)

    def test_the_command_log(self):
        line = arbiter.log_line(1_234_567, '/fmu/in/vehicle_command')
        self.assertEqual(json.loads(line), {'t': 1.234567, 'topic': '/fmu/in/vehicle_command', 'source': 'recording'})


class FlightLog(unittest.TestCase):
    def test_an_imported_flight_is_read_from_its_exported_lines(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            (folder / 'commands.jsonl').write_text('\n'.join([
                json.dumps({'topic': '/fmu/in/trajectory_setpoint', 't_ns': 3_000_000_000,
                            'message': {'timestamp': 3_000_000, 'velocity': ['nan', 1.0, 0.0]}}),
                'not json',
                json.dumps({'topic': '/fmu/in/other', 't_ns': 1, 'message': {}})]) + '\n')
            (folder / 'evidence.jsonl').write_text(json.dumps(
                {'topic': '/fmu/out/vehicle_status', 't_ns': 2_000_000_000, 'message': {'timestamp': 2_000_000}}) + '\n')
            recipe = {'source': 'ulog', 'command_topics': ['/fmu/in/trajectory_setpoint'],
                      'evidence_topics': ['/fmu/out/vehicle_status'],
                      'command_types': {'/fmu/in/trajectory_setpoint': 'px4_msgs/msg/TrajectorySetpoint'}}
            types, commands, evidence = arbiter.read_recording(folder, recipe)
            self.assertEqual(types, {'/fmu/in/trajectory_setpoint': 'px4_msgs/msg/TrajectorySetpoint'})
            self.assertEqual(len(commands), 1)
            self.assertTrue(commands[0][2]['velocity'][0] != commands[0][2]['velocity'][0], 'NaN restored')
            schedule, early = arbiter.build_schedule(commands, evidence)
            self.assertEqual(([o for o, _, _ in schedule], early), ([1_000_000], 0), 'one second after the status')


if __name__ == '__main__':
    unittest.main()
