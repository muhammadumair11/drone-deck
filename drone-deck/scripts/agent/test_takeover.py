"""takeover.* without ROS: when the user's code takes over from the recording."""
import unittest

import arbiter
import takeover
from takeover import Takeover

SETPOINT = '/fmu/in/trajectory_setpoint'
COMMAND = '/fmu/in/vehicle_command'
ANCHOR = 1_000_000
MS = 1000


def setpoint(vx, x=float('nan')):
    return {'timestamp': 5, 'position': [x, x, -2.0], 'velocity': [vx, 0.0, 0.0], 'yaw': 0.5}


def fly(judge, yours, seconds=2.0, rate_ms=100, start_ms=0):
    """Recorded setpoint vx = 2 m/s every `rate_ms`; `yours(t_ms)` gives your vx (None: silent).
    Returns the takeover."""
    for t in range(start_ms, start_ms + int(seconds * 1000), rate_ms):
        now = ANCHOR + t * MS
        mine = yours(t)
        if mine is not None:
            judge.observe(SETPOINT, setpoint(mine), now - 10 * MS)
        taken = judge.judge(SETPOINT, setpoint(2.0), now, ANCHOR)
        if taken:
            return taken
    return judge.taken


class Fields(unittest.TestCase):
    def test_names_and_flattening(self):
        self.assertEqual(takeover.topic_name('/fmu/in/vehicle_command_v1'), 'vehicle_command')
        self.assertEqual(takeover.topic_name('/fmu/in/trajectory_setpoint'), 'trajectory_setpoint')
        self.assertEqual(takeover.flatten({'timestamp': 1, 'v': [1, 2], 'm': {'a': True}}),
                         {'v[0]': 1, 'v[1]': 2, 'm.a': True})

    def test_differences(self):
        diff = takeover.difference
        self.assertIsNone(diff(setpoint(2.0), setpoint(2.19), 0.10, 0.0), 'within 10%')
        self.assertEqual(diff(setpoint(2.0), setpoint(2.3), 0.10, 0.0)[0], 'velocity[0]')
        self.assertIsNone(diff(setpoint(0.0), setpoint(0.15), 0.10, 0.2), 'the floor allows small values')
        self.assertEqual(diff(setpoint(2.0, 1.0), setpoint(2.0), 0.10, 0.0)[0], 'position[0]',
                         'NaN (unused) matches only NaN')
        self.assertIsNone(diff({'a': 'nan'}, {'a': float('nan')}, 0.1, 0.0), 'NaN as text from a recording')
        self.assertEqual(diff({'flag': True}, {'flag': False}, 0.5, 1.0)[0], 'flag')
        self.assertEqual(diff({'a': 1.0}, {}, 0.1, 0.0), ('a', 1.0, None))


class Takeovers(unittest.TestCase):
    def test_identical_code_never_takes_over(self):
        judge = Takeover(0.10, hold_us=500 * MS)
        self.assertIsNone(fly(judge, lambda t: 2.0))
        self.assertEqual(judge.steering, 'recording')
        self.assertEqual(judge.report()['differed'], 0)

    def test_a_lasting_difference_takes_over_after_the_hold(self):
        judge = Takeover(0.10, hold_us=500 * MS)
        taken = fly(judge, lambda t: 2.3 if t >= 1000 else 2.0, seconds=3.0)
        self.assertEqual(taken['t'], 1.5, 'the difference began at 1.0 s and lasted 0.5 s')
        self.assertEqual(taken['topic'], SETPOINT)
        self.assertIn('velocity[0]: recorded 2, yours 2.3', taken['reason'])
        self.assertEqual(judge.steering, 'your code')

    def test_a_short_blip_does_not(self):
        judge = Takeover(0.10, hold_us=500 * MS)
        self.assertIsNone(fly(judge, lambda t: 2.3 if 1000 <= t < 1200 else 2.0, seconds=3.0))

    def test_zero_tolerance_hands_over_at_the_first_command(self):
        judge = Takeover(0.0)
        taken = fly(judge, lambda t: 2.0, start_ms=300)
        self.assertEqual(taken['t'], 0.3)
        self.assertIn('0%', taken['reason'])

    def test_off_only_watches(self):
        judge = Takeover(None)
        self.assertIsNone(fly(judge, lambda t: 50.0))
        self.assertFalse(judge.watching)
        self.assertEqual(judge.report()['compared'], 0)

    def test_silence_counts_after_the_grace(self):
        judge = Takeover(0.10, hold_us=500 * MS, grace_us=1000 * MS)
        taken = fly(judge, lambda t: None, seconds=3.0)
        self.assertEqual(taken['t'], 1.5, 'silent from 1.0 s (grace), held 0.5 s')
        self.assertIn('sent nothing', taken['reason'])

    def test_a_missing_vehicle_command_hands_over_at_once(self):
        judge = Takeover(0.10)
        judge.observe(COMMAND, {'command': 176}, ANCHOR + 100 * MS)
        self.assertIsNone(judge.judge(COMMAND, {'command': 176}, ANCHOR + 400 * MS, ANCHOR), 'yours came first')
        self.assertIsNone(judge.judge(COMMAND, {'command': 400}, ANCHOR + 1000 * MS, ANCHOR))
        judge.observe(COMMAND, {'command': 400}, ANCHOR + 1300 * MS)
        self.assertIsNone(judge.tick(ANCHOR + 2000 * MS, ANCHOR), 'yours came within 0.5 s')
        self.assertIsNone(judge.judge(COMMAND, {'command': 21}, ANCHOR + 3000 * MS, ANCHOR))
        self.assertIsNone(judge.tick(ANCHOR + 3400 * MS, ANCHOR))
        taken = judge.tick(ANCHOR + 3500 * MS, ANCHOR)
        self.assertEqual(taken['t'], 3.5)
        self.assertIn('vehicle_command 21', taken['reason'])

    def test_after_a_takeover_nothing_is_judged_again(self):
        judge = Takeover(0.10, hold_us=0)
        fly(judge, lambda t: 9.0, seconds=0.2)
        first = dict(judge.taken)
        self.assertIsNone(judge.judge(SETPOINT, setpoint(2.0), ANCHOR + 5_000 * MS, ANCHOR))
        self.assertEqual(judge.taken, first)


class Arguments(unittest.TestCase):
    def test_tolerance_and_status(self):
        self.assertIsNone(arbiter.tolerance_arg('off'))
        self.assertEqual(arbiter.tolerance_arg('10'), 0.1)
        with self.assertRaises(Exception):
            arbiter.tolerance_arg('150')
        judge = Takeover(0.10, hold_us=0)
        fly(judge, lambda t: 9.0, seconds=0.2)
        value = arbiter.status(arbiter.Player([]), arbiter.FreshClock(), 0.0, 0, takeover=judge)
        self.assertEqual(value['steering'], 'your code')
        self.assertEqual(value['takeover']['topic'], SETPOINT)
        self.assertEqual((value['tolerance'], value['hold_s']), (0.1, 0.0))


if __name__ == '__main__':
    unittest.main()
