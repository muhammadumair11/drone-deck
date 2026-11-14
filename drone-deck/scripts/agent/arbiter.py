"""Drone Deck re-simulation arbiter: plays a recording's autopilot commands into a fresh simulator
at the time they were sent. This module is the PX4 player and the timing both autopilots share;
ArduPilot's player is `arbiter_ap.py` (`--autopilot ardupilot`).

Started as an owned agent process on the run's isolated, localhost-only domain:

    python -u arbiter.py --recording <folder> --namespace resim --out <run folder> --wait 90 --tail 5

The fresh PX4 runs its DDS client in a namespace (`PX4_UXRCE_DDS_NS=resim`), so only this node can
reach it: it publishes the recorded commands on `/resim/fmu/in/*` and relays the fresh vehicle's
`/resim/fmu/out/*` back to `/fmu/out/*`, where your own code reads it unchanged.

Timing is kept in autopilot time, not wall time. Each recorded command's log time is mapped to the
recorded autopilot's clock through the `timestamp` field of the evidence messages around it, and
measured from the anchor: the first vehicle status in the recording. In the fresh run, the clock
starts at the fresh autopilot's first vehicle status and follows its later reports, so a PC that
runs the simulator below real time does not shift the commands. Nothing is published before that
anchor, and every forwarded message's `timestamp` is rewritten to the fresh clock.

With `--tolerance`, your code runs in shadow: what it publishes on `/fmu/in/*` is compared with
each recorded command (`takeover.py`), and once a difference lasts `--hold` it steers the fresh
vehicle for the rest of the run, one way; `off` (the default) only watches.

`status.json` in the run folder says how far the replay is and who steers (`status`); the player exits 0 a few
seconds (`--tail`) after the last command, and 3 when the fresh vehicle never reports (`--wait`).

rclpy is imported in `main` only, so the scheduling functions can be tested without ROS.
"""
import argparse
import bisect
import json
import os
from pathlib import Path
import sys
import time

sys.path.insert(0, os.environ.get('DRONE_DECK_AGENT') or str(Path(__file__).resolve().parent))

ANCHOR = 'vehicle_status'
# ArduPilot's status report (ardupilot_msgs/Status), sent on each change and twice a second.
AP_ANCHOR = '/ap/status'
# How far the fresh clock may run ahead of the autopilot's last report (µs): a stalled simulator
# stalls the replay instead of letting commands race ahead of it.
AHEAD_LIMIT_US = 200_000
# A clock that moves this much against log time changed its base (µs): ArduPilot stamps with the
# time since boot until its clock is set (GPS), then with UTC. A stalled simulator moves far less.
BASE_JUMP_US = 3600 * 1_000_000
# Recipe sources of real flights exported from a flight log (PX4's ULog, ArduPilot's DataFlash).
FROM_LOG = ('ulog', 'dataflash')


def is_anchor(topic):
    """vehicle_status and its versioned names (`vehicle_status_v1`), and ArduPilot's /ap/status."""
    if topic.rstrip('/') == AP_ANCHOR:
        return True
    name = topic.rstrip('/').rsplit('/', 1)[-1]
    return name == ANCHOR or name.startswith(ANCHOR + '_v')


def stamp_us(message):
    """The autopilot time of a message (µs): PX4's `timestamp`, a ROS `header.stamp`, or a bare
    builtin_interfaces/Time (ArduPilot's /ap/time). None when it has none."""
    if not isinstance(message, dict):
        return None
    stamp = message.get('timestamp')
    if isinstance(stamp, int) and not isinstance(stamp, bool):
        return stamp if stamp > 0 else None
    header = message.get('header')
    time = header.get('stamp') if isinstance(header, dict) else message
    if isinstance(time, dict) and isinstance(time.get('sec'), int) and isinstance(time.get('nanosec'), int):
        value = time['sec'] * 1_000_000 + time['nanosec'] // 1000
        return value if value > 0 else None
    return None


def final_base(pairs):
    """The log time (ns) from which the (log ns, autopilot µs) pairs keep their last clock base,
    or None without pairs: ArduPilot's switch from time since boot to UTC is a jump far larger
    than any stall."""
    start, previous = None, None
    for log_ns, ap_us in sorted(pairs):
        offset = ap_us - log_ns / 1000
        if previous is None or abs(offset - previous) > BASE_JUMP_US:
            start = log_ns
        previous = offset
    return start


def namespaced(topic, namespace):
    """`/fmu/in/x` → `/resim/fmu/in/x`."""
    return f'/{namespace.strip("/")}/{topic.lstrip("/")}'


def unnamespaced(topic, namespace):
    """`/resim/fmu/out/x` → `/fmu/out/x`; None for a topic outside the namespace."""
    prefix = f'/{namespace.strip("/")}/'
    return '/' + topic[len(prefix):] if topic.startswith(prefix) else None


class ClockMap:
    """Recording log time (ns) → recorded autopilot time (µs), piecewise linear through the
    evidence messages' (log time, timestamp) pairs. Outside them, it runs at real time from the
    nearest pair. Pairs that go back in either clock are dropped."""

    def __init__(self, pairs):
        self.log, self.ap = [], []
        for log_ns, ap_us in sorted(pairs):
            if ap_us <= 0:
                continue
            if self.log and (log_ns <= self.log[-1] or ap_us <= self.ap[-1]):
                continue
            self.log.append(log_ns)
            self.ap.append(ap_us)

    def __bool__(self):
        return bool(self.log)

    def autopilot_us(self, log_ns):
        if not self.log:
            raise ValueError('The recording has no autopilot timestamps to time its commands by.')
        i = bisect.bisect_right(self.log, log_ns)
        if i == 0:
            return self.ap[0] + (log_ns - self.log[0]) / 1000
        if i == len(self.log):
            return self.ap[-1] + (log_ns - self.log[-1]) / 1000
        l0, l1, a0, a1 = self.log[i - 1], self.log[i], self.ap[i - 1], self.ap[i]
        return a0 + (a1 - a0) * (log_ns - l0) / (l1 - l0)


def build_schedule(commands, evidence):
    """Commands as [(offset µs from the anchor, topic, message dict)], in order, plus how many were
    sent before the anchor (never replayed).

    `commands`: (topic, log_time_ns, message dict) from the recorded command topics.
    `evidence`: (topic, log_time_ns, message dict) from the recorded autopilot reports."""
    stamped = [(topic, log_ns, stamp_us(message)) for topic, log_ns, message in evidence]
    stamped = [row for row in stamped if row[2] is not None]
    base = final_base([(log_ns, stamp) for _, log_ns, stamp in stamped])
    pairs, anchor = [], None
    for topic, log_ns, stamp in stamped:
        if log_ns < base:
            continue  # an earlier clock base (ArduPilot before its clock was set)
        pairs.append((log_ns, stamp))
        if is_anchor(topic) and (anchor is None or stamp < anchor):
            anchor = stamp
    clock = ClockMap(pairs)
    if anchor is None:
        raise ValueError('The recording has no vehicle status to start the replay from.')
    schedule, early = [], 0
    for order, (topic, log_ns, message) in enumerate(commands):
        offset = clock.autopilot_us(log_ns) - anchor
        if offset < 0:
            early += 1
            continue
        schedule.append((round(offset), order, topic, message))
    schedule.sort()
    return [(offset, topic, message) for offset, _, topic, message in schedule], early


def rewrite(message, now_us):
    """A copy of `message` stamped at `now_us` on the fresh clock: PX4's `timestamp`
    (`timestamp_sample` keeps its distance from it), or the `header.stamp` of a ROS message. A
    message with neither (a service request) is copied unchanged."""
    out = dict(message)
    if 'timestamp' not in message:
        if isinstance(message.get('header'), dict):
            now = int(now_us)
            out['header'] = dict(message['header'], stamp={'sec': now // 1_000_000,
                                                           'nanosec': (now % 1_000_000) * 1000})
        return out
    old = message.get('timestamp')
    out['timestamp'] = int(now_us)
    sample = message.get('timestamp_sample')
    if isinstance(sample, int) and isinstance(old, int) and sample > 0:
        out['timestamp_sample'] = max(0, int(now_us) - (old - sample))
    return out


class FreshClock:
    """The fresh autopilot's time (µs), from its vehicle status reports and the local monotonic
    clock between them, never more than `AHEAD_LIMIT_US` past the last report."""

    def __init__(self):
        self.anchor = None
        self.last = None

    def observe(self, stamp_us, mono_s):
        if not isinstance(stamp_us, int) or stamp_us <= 0:
            return
        if self.anchor is None:
            self.anchor = stamp_us
        if self.last is None or stamp_us >= self.last[0]:
            self.last = (stamp_us, mono_s)

    def now_us(self, mono_s):
        if self.last is None:
            return None
        stamp, seen = self.last
        return stamp + min(max(0.0, (mono_s - seen) * 1e6), AHEAD_LIMIT_US)


class Player:
    """Hands out the scheduled commands once the fresh clock reaches them."""

    def __init__(self, schedule):
        self.schedule = schedule
        self.next = 0

    @property
    def done(self):
        return self.next >= len(self.schedule)

    def due(self, clock, mono_s):
        """[(topic, rewritten message, offset µs)] due now; nothing before the fresh anchor."""
        now = clock.now_us(mono_s)
        if now is None or clock.anchor is None:
            return []
        out = []
        while self.next < len(self.schedule):
            offset, topic, message = self.schedule[self.next]
            if clock.anchor + offset > now:
                break
            out.append((topic, rewrite(message, clock.anchor + offset), offset))
            self.next += 1
        return out


def status(player, clock, mono_s, before_anchor, error=None, takeover=None):
    """What `status.json` says: commands replayed of the total, replay time since the anchor (s),
    who steers (and the takeover, `takeover.Takeover.report`), and an error that ended the run."""
    now = clock.now_us(mono_s)
    value = {'commands': len(player.schedule), 'sent': player.next, 'before_anchor': before_anchor,
             'anchored': clock.anchor is not None,
             't': round((now - clock.anchor) / 1e6, 3) if now is not None and clock.anchor is not None else 0.0,
             'steering': 'recording', 'done': player.done}
    if takeover is not None:
        value.update(takeover.report())
    if error:
        value['error'] = error
    return value


def tolerance_arg(text):
    """`off` → None (only watch), else a percentage 0–100 as a fraction."""
    if text == 'off':
        return None
    value = float(text)
    if not 0 <= value <= 100:
        raise argparse.ArgumentTypeError('tolerance is a percentage from 0 to 100, or off')
    return value / 100


def write_status(folder, value):
    temporary = folder / 'status.json.tmp'
    temporary.write_text(json.dumps(value))
    os.replace(temporary, folder / 'status.json')


def log_line(offset_us, topic, source='recording'):
    """One `commands.jsonl` line: what was forwarded, when (seconds from the anchor), from where."""
    return json.dumps({'t': round(offset_us / 1e6, 6), 'topic': topic, 'source': source}) + '\n'


def restore(value):
    """JSON from a flight log keeps NaN and infinities as text; ROS needs the floats back."""
    if isinstance(value, dict):
        return {k: restore(v) for k, v in value.items()}
    if isinstance(value, list):
        return [restore(v) for v in value]
    if value in ('nan', 'inf', '-inf'):
        return float(value)
    return value


def read_jsonl(path, topics):
    """(topic, log ns, message) rows of a flight exported from its log (`commands.jsonl`,
    `evidence.jsonl`); a line that does not parse is skipped."""
    rows = []
    if not path.is_file():
        return rows
    with path.open(encoding='utf-8', errors='replace') as stream:
        for line in stream:
            try:
                row = json.loads(line)
            except ValueError:
                continue
            if isinstance(row, dict) and row.get('topic') in topics and isinstance(row.get('t_ns'), int) \
                    and isinstance(row.get('message'), dict):
                rows.append((row['topic'], row['t_ns'], restore(row['message'])))
    return rows


# -- ROS ---------------------------------------------------------------------------------------
def read_recording(recording, recipe):
    commands = list(recipe.get('command_topics') or [])
    evidence = list(recipe.get('evidence_topics') or [])
    if not commands:
        raise ValueError('The recipe names no command topics to replay.')
    if recipe.get('source') in FROM_LOG:
        # A real flight imported from its log: the commands and reports it exported, timed by
        # the autopilot's own clock (log time is its timestamp).
        types = {t: k for t, k in (recipe.get('command_types') or {}).items() if isinstance(k, str)}
        return (types, read_jsonl(recording / 'commands.jsonl', set(commands)),
                read_jsonl(recording / 'evidence.jsonl', set(evidence)))
    import pipe
    _, (_, _, messages) = pipe.open_messages(recording, commands + evidence, None, None)
    types, recorded, reports = {}, [], []
    for topic, kind, log_ns, decoded in messages:
        types[topic] = kind
        row = (topic, log_ns, pipe.plain(decoded))
        (recorded if topic in commands else reports).append(row)
    return types, recorded, reports


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--recording', required=True, type=Path)
    parser.add_argument('--namespace', default='resim')
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--wait', type=float, default=90.0,
                        help="seconds to wait for the fresh vehicle's first status")
    parser.add_argument('--tail', type=float, default=5.0,
                        help='seconds to keep relaying after the last command')
    parser.add_argument('--tolerance', type=tolerance_arg, default=None,
                        help='percent your code may differ before it takes over, or off')
    parser.add_argument('--floor', type=float, default=0.0,
                        help='absolute difference always allowed (same units as the field)')
    parser.add_argument('--hold', type=float, default=0.5, help='seconds a difference must last')
    parser.add_argument('--autopilot', choices=('px4', 'ardupilot'), default='px4')
    parser.add_argument('--vehicle-domain', type=int, default=None,
                        help="ArduPilot: the fresh vehicle's own ROS domain (it takes no namespace)")
    args = parser.parse_args(argv)
    if args.autopilot == 'ardupilot':
        import arbiter_ap
        raise SystemExit(arbiter_ap.main(args))
    import pipe
    from takeover import Takeover

    import rclpy
    from rclpy.qos import QoSProfile, ReliabilityPolicy, HistoryPolicy, DurabilityPolicy
    from rosidl_runtime_py.set_message import set_message_fields
    from rosidl_runtime_py.utilities import get_message

    recipe = json.loads((args.recording / 'recipe.json').read_text())
    types, recorded, reports = read_recording(args.recording, recipe)
    args.out.mkdir(parents=True, exist_ok=True)
    try:
        schedule, early = build_schedule(recorded, reports)
    except ValueError as error:
        write_status(args.out, {'commands': 0, 'sent': 0, 'anchored': False, 'error': str(error)})
        raise SystemExit(2) from None
    print(json.dumps({'kind': 'arbiter', 'commands': len(schedule), 'before_anchor': early}), flush=True)

    rclpy.init()
    node = rclpy.create_node('drone_deck_arbiter')
    # PX4's DDS client publishes best effort, transient local; a volatile subscriber matches it.
    qos = QoSProfile(reliability=ReliabilityPolicy.BEST_EFFORT, durability=DurabilityPolicy.TRANSIENT_LOCAL,
                     history=HistoryPolicy.KEEP_LAST, depth=1)
    listen = QoSProfile(reliability=ReliabilityPolicy.BEST_EFFORT, durability=DurabilityPolicy.VOLATILE,
                        history=HistoryPolicy.KEEP_LAST, depth=5)
    classes = {topic: get_message(kind) for topic, kind in types.items() if kind}
    publishers = {topic: node.create_publisher(classes[topic], namespaced(topic, args.namespace), qos)
                  for topic in {t for _, t, _ in schedule} if topic in classes}
    clock, player = FreshClock(), Player(schedule)
    judge = Takeover(args.tolerance, args.floor, int(args.hold * 1e6))
    relays = []
    yours = {}  # your code's /fmu/in topics → the namespaced publisher used after a takeover

    def announce(taken):
        print(json.dumps({'kind': 'arbiter', 'takeover': taken}), flush=True)
        log.write(json.dumps({'t': taken['t'], 'takeover': taken}) + '\n')
        write_status(args.out, report())

    def listen_to(topic, kind):
        cls = get_message(kind)
        out = node.create_publisher(cls, namespaced(topic, args.namespace), qos)
        yours[topic] = out

        def heard(message):
            now = clock.now_us(time.monotonic())
            if judge.taken:
                out.publish(message)
                if now is not None and clock.anchor is not None:
                    log.write(log_line(now - clock.anchor, topic, 'your code'))
            elif now is not None:
                judge.observe(topic, pipe.plain(message), now)
        relays.append(node.create_subscription(cls, topic, heard, listen))

    def relay(fresh_topic, kind):
        local = unnamespaced(fresh_topic, args.namespace)
        out = node.create_publisher(get_message(kind), local, qos)

        def forward(message):
            if is_anchor(local):
                clock.observe(int(message.timestamp), time.monotonic())
            out.publish(message)
        relays.append(node.create_subscription(get_message(kind), fresh_topic, forward, listen))

    seen = set()
    log = (args.out / 'commands.jsonl').open('a')
    started = time.monotonic()
    marks = {'scan': 0.0, 'status': 0.0}

    def report(error=None):
        return status(player, clock, time.monotonic(), early, error, judge)

    def tick():
        mono = time.monotonic()
        if mono - marks['scan'] >= 1.0:
            marks['scan'] = mono
            for name, kinds in node.get_topic_names_and_types():
                if name in seen or not kinds:
                    continue
                if unnamespaced(name, args.namespace) and '/fmu/out/' in name:
                    seen.add(name)
                    relay(name, kinds[0])
                elif judge.watching and name.startswith('/fmu/in/'):
                    seen.add(name)
                    listen_to(name, kinds[0])
        if mono - marks['status'] >= 0.5:
            marks['status'] = mono
            write_status(args.out, report())
        for topic, message, offset in player.due(clock, mono):
            if judge.taken:
                continue  # one way: the recording never steers again
            typed = classes[topic]()
            set_message_fields(typed, message)
            publishers[topic].publish(typed)
            log.write(log_line(offset, topic))
            taken = judge.judge(topic, message, clock.anchor + offset, clock.anchor)
            if taken:
                announce(taken)
        now = clock.now_us(mono)
        if now is not None and clock.anchor is not None:
            taken = judge.tick(now, clock.anchor)
            if taken:
                announce(taken)
        if player.done:
            log.flush()

    node.create_timer(0.002, tick)
    code = 0
    try:
        while rclpy.ok() and not player.done:
            rclpy.spin_once(node, timeout_sec=0.05)
            if clock.anchor is None and time.monotonic() - started > args.wait:
                error = (f"No vehicle status from the fresh autopilot under /{args.namespace}/fmu/out within "
                         f"{args.wait:g} s. Check that PX4 took the run's DDS namespace and ROS domain.")
                write_status(args.out, report(error))
                code = 3
                break
        if code == 0:
            print(json.dumps({'kind': 'arbiter', 'done': True}), flush=True)
            until = time.monotonic() + args.tail
            while rclpy.ok() and time.monotonic() < until:  # the fresh vehicle settles
                rclpy.spin_once(node, timeout_sec=0.1)
            write_status(args.out, report())
    except KeyboardInterrupt:
        write_status(args.out, report())
    finally:
        log.close()
        node.destroy_node()
        if rclpy.ok():
            rclpy.shutdown()
    raise SystemExit(code)


if __name__ == '__main__':
    main()
