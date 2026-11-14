"""When the user's code takes over a re-simulation from the recording.

The user's code runs from the start of the run and publishes its commands on the usual
`/fmu/in/*` topics, which reach only the command player (the fresh PX4 listens under its own
namespace). For each recorded command the player sends, it compares the user's latest command on
the same topic:

- numbers (setpoints, flags) differ when |recorded − yours| > max(tolerance × |recorded|, floor);
  NaN (PX4's "unused") matches only NaN; text and true/false must be equal; the time stamps
  (PX4's `timestamp*`, a ROS `header`) are left out;
- a `vehicle_command` matches when yours sent the same command id within 0.5 s of it, and an
  ArduPilot service call (arm, mode switch, takeoff) when yours made the same call within 2 s
  (3 s for takeoff: the recording's calls are worked out from /ap/status, which follows the
  call); a missing one is a difference at once;
- a topic your code has not published (or not for 0.5 s) is a difference, after a short grace at
  the start while your code comes up.

A difference that lasts `hold` hands control to your code for the rest of the run: one way, the
recording never steers again. Tolerance `None` (off) only watches; tolerance 0 hands over at the
first command.
"""
import math

GRACE_US = 5_000_000
STALE_US = 500_000
COMMAND_WINDOW_US = 500_000
COMMAND_TOPIC = 'vehicle_command'
# ArduPilot's command services, as the player names them: (what the call is keyed by, window µs).
SERVICES = {'/ap/arm_motors': ('arm', 2_000_000), '/ap/mode_switch': ('mode', 2_000_000),
            '/ap/experimental/takeoff': (None, 3_000_000)}
LONGEST_WINDOW_US = max(w for _, w in SERVICES.values())


def topic_name(topic):
    """`/fmu/in/vehicle_command_v1` → `vehicle_command`."""
    name = topic.rstrip('/').rsplit('/', 1)[-1]
    head, _, version = name.rpartition('_v')
    return head if head and version.isdigit() else name


def discrete(topic, message):
    """(key, window µs, words) of a command that happens once rather than streams: PX4's
    vehicle_command (keyed by its command id) and ArduPilot's arm, mode and takeoff calls. None for
    a streamed command."""
    if topic_name(topic) == COMMAND_TOPIC:
        command = message.get('command') if isinstance(message, dict) else None
        return command, COMMAND_WINDOW_US, f'vehicle_command {command}'
    if topic in SERVICES:
        field, window = SERVICES[topic]
        value = message.get(field) if field and isinstance(message, dict) else None
        key = (topic, value)
        return key, window, f'the {topic} call' + (f' ({field} {value})' if field else '')
    return None


def flatten(value, prefix=''):
    """{path: leaf} for a message dict; the time stamps (`timestamp*`, a ROS `header`) are left
    out: they differ by design."""
    out = {}
    if isinstance(value, dict):
        for key, item in value.items():
            if prefix == '' and (str(key).startswith('timestamp') or key == 'header'):
                continue
            out.update(flatten(item, f'{prefix}.{key}' if prefix else str(key)))
    elif isinstance(value, (list, tuple)):
        for i, item in enumerate(value):
            out.update(flatten(item, f'{prefix}[{i}]'))
    else:
        out[prefix] = value
    return out


def number(value):
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str) and value in ('nan', 'inf', '-inf'):  # pipe.plain writes these as text
        return float(value)
    return None


def difference(recorded, yours, tolerance, floor):
    """None when `yours` matches `recorded`, else (field, recorded value, your value)."""
    a, b = flatten(recorded), flatten(yours)
    for field, x in a.items():
        if field not in b:
            return field, x, None
        y = b[field]
        nx, ny = number(x), number(y)
        if nx is None or ny is None:
            if x != y:
                return field, x, y
            continue
        if math.isnan(nx) or math.isnan(ny):
            if not (math.isnan(nx) and math.isnan(ny)):
                return field, x, y
            continue
        if math.isinf(nx) or math.isinf(ny):
            if nx != ny:
                return field, x, y
            continue
        if abs(nx - ny) > max(tolerance * abs(nx), floor):
            return field, x, y
    return None


def show(value):
    return 'nothing' if value is None else f'{value:.3g}' if isinstance(value, float) else str(value)


class Takeover:
    def __init__(self, tolerance, floor=0.0, hold_us=500_000, grace_us=GRACE_US):
        self.tolerance = tolerance
        self.floor = floor
        self.hold_us = hold_us
        self.grace_us = grace_us
        self.start = None
        self.latest = {}       # topic → (fresh µs, message)
        self.sent = []         # your one-off commands: (fresh µs, key)
        self.pending = []      # recorded ones awaiting yours: (deadline µs, key, topic, words, window µs)
        self.since = {}        # topic → (fresh µs the difference began, reason)
        self.taken = None      # {'t', 'topic', 'reason'} once your code steers
        self.compared = 0
        self.differed = 0

    @property
    def watching(self):
        return self.tolerance is not None

    @property
    def steering(self):
        return 'your code' if self.taken else 'recording'

    def observe(self, topic, message, now_us):
        """A command your code published."""
        self.latest[topic] = (now_us, message)
        once = discrete(topic, message)
        if once is not None:
            key = once[0]
            self.sent = [s for s in self.sent if now_us - s[0] <= LONGEST_WINDOW_US] + [(now_us, key)]
            self.pending = [p for p in self.pending if p[1] != key]

    def _take(self, now_us, anchor_us, topic, reason):
        self.taken = {'t': round((now_us - anchor_us) / 1e6, 3), 'topic': topic, 'reason': reason}
        return self.taken

    def judge(self, topic, recorded, now_us, anchor_us):
        """Compares the recorded command the player sends now with yours. Returns the takeover
        when it happens here, else None."""
        if not self.watching or self.taken:
            return None
        if self.start is None:
            self.start = now_us
        if self.tolerance == 0:
            return self._take(now_us, anchor_us, topic, 'Tolerance 0%: your code flies from the start.')
        self.compared += 1
        name = topic_name(topic)
        once = discrete(topic, recorded)
        if once is not None:
            key, window, words = once
            if any(k == key and now_us - t <= window for t, k in self.sent):
                return None
            self.pending.append((now_us + window, key, topic, words, window))
            return None
        seen = self.latest.get(topic)
        if seen is None or now_us - seen[0] > STALE_US:
            if now_us - self.start < self.grace_us:
                return None
            reason = f'Your code sent nothing on {topic}.'
        else:
            found = difference(recorded, seen[1], self.tolerance, self.floor)
            if found is None:
                self.since.pop(topic, None)
                return None
            field, x, y = found
            reason = f'{name}.{field}: recorded {show(x)}, yours {show(y)}.'
        self.differed += 1
        began, _ = self.since.setdefault(topic, (now_us, reason))
        if now_us - began >= self.hold_us:
            return self._take(now_us, anchor_us, topic, reason)
        return None

    def tick(self, now_us, anchor_us):
        """A recorded one-off command yours never matched within its window hands over at once."""
        if not self.watching or self.taken or not self.pending:
            return None
        for deadline, _, topic, words, window in self.pending:
            if now_us >= deadline:
                self.pending = []
                return self._take(now_us, anchor_us, topic,
                                  f'Your code did not send {words} within {window / 1e6:g} s of the recording.')
        return None

    def report(self):
        value = {'tolerance': self.tolerance, 'floor': self.floor, 'hold_s': self.hold_us / 1e6,
                 'compared': self.compared, 'differed': self.differed, 'steering': self.steering}
        if self.taken:
            value['takeover'] = self.taken
        return value
