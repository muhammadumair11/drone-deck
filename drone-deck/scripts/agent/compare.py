"""Compares a re-simulation with the flight it replayed, and writes `compare.json` in the run folder.

    python3 compare.py <recording folder> <run folder>

Both flights are put on one clock: seconds from their own first vehicle status (PX4's
vehicle_status, ArduPilot's /ap/status: the anchor the command player times the replay from), using
the autopilot's own time stamps (PX4's `timestamp`, ArduPilot's `header.stamp`, from the clock base
the flight ended in); AI output, which has no autopilot time, uses log time from the same anchor.
ArduPilot's pose (ENU) is turned to north, east, down like PX4's local position.

- **Path**: the fresh vehicle's local position, interpolated at the recorded one's times. The path
  stays the same course while the distance between them is within max(tolerance × the recorded
  vehicle's distance from where it started, 1 m); the first time it is not is where the course
  changed. The tolerance is the run's (10% when it ran with takeover off).
- **Events**: arming, flight mode, failsafe and landed changes from vehicle status and land
  detection, paired by kind and value in order, with the time shift of each pair.
- **Commands**: messages per command topic in the recording, what the run sent from the recording
  and from your code (`commands.jsonl`), and the takeover.
- **AI output**: for each AI topic, messages in each flight and, for JSON detections like
  `infer_node.py` publishes, how often the top label agrees at the same moment. No accuracy is
  computed: neither flight is ground truth.

The functions below take plain rows and are tested without ROS; `main` reads the MCAP files.
"""
import bisect
import json
import math
import os
from pathlib import Path
import sys

sys.path.insert(0, os.environ.get('DRONE_DECK_AGENT') or str(Path(__file__).resolve().parent))

from arbiter import final_base, is_anchor, read_jsonl, stamp_us  # noqa: E402

SCHEMA = 1
PATH_FLOOR_M = 1.0
DEFAULT_TOLERANCE = 0.10
PLOT_POINTS = 400
AI_WINDOW_S = 0.5
NAV_STATES = {0: 'Manual', 1: 'Altitude', 2: 'Position', 3: 'Mission', 4: 'Hold', 5: 'Return',
              10: 'Acro', 12: 'Descend', 13: 'Termination', 14: 'Offboard', 15: 'Stabilized',
              17: 'Takeoff', 18: 'Land', 19: 'Follow', 20: 'Precision land', 21: 'Orbit',
              22: 'VTOL takeoff'}
ARMING = {1: 'Disarmed', 2: 'Armed'}
AP_STATUS = '/ap/status'
AP_POSE = '/ap/pose/filtered'
# ArduCopter's flight modes (ardupilot_msgs/Status.mode when vehicle_type is a copter or heli).
AP_COPTER_MODES = {0: 'Stabilize', 1: 'Acro', 2: 'Altitude hold', 3: 'Auto', 4: 'Guided', 5: 'Loiter',
                   6: 'RTL', 7: 'Circle', 9: 'Land', 11: 'Drift', 13: 'Sport', 14: 'Flip', 15: 'AutoTune',
                   16: 'Position hold', 17: 'Brake', 18: 'Throw', 19: 'Avoid ADS-B', 20: 'Guided (no GPS)',
                   21: 'Smart RTL', 22: 'Flow hold', 23: 'Follow', 24: 'ZigZag', 25: 'System ID',
                   26: 'Autorotate', 27: 'Auto RTL', 28: 'Turtle'}
AP_COPTERS = (2, 13)


def leaf(topic):
    name = topic.rstrip('/').rsplit('/', 1)[-1]
    head, _, version = name.rpartition('_v')
    return head if head and version.isdigit() else name


def in_final_base(rows):
    """`rows` from where the autopilot clock took its last base (ArduPilot: from time since boot to
    UTC); all of them when it never changed."""
    pairs = [(log_ns, s) for _, log_ns, m in rows if (s := stamp_us(m)) is not None]
    base = final_base(pairs)
    if base is None or base == min(log_ns for log_ns, _ in pairs):
        return rows
    return [r for r in rows if r[1] >= base]


def anchors(rows):
    """(autopilot µs, log ns) of the first vehicle status, or (None, None)."""
    best = None
    for topic, log_ns, message in rows:
        stamp = stamp_us(message)
        if is_anchor(topic) and stamp is not None and (best is None or stamp < best[0]):
            best = (stamp, log_ns)
    return best or (None, None)


def ned(topic, m):
    """(north, east, down) metres of a local position report, or None: PX4's
    vehicle_local_position (NED already) or ArduPilot's pose (ENU)."""
    if leaf(topic) == 'vehicle_local_position':
        xyz = [m.get(k) for k in ('x', 'y', 'z')]
    elif topic == AP_POSE:
        p = (m.get('pose') or {}).get('position') if isinstance(m.get('pose'), dict) else None
        if not isinstance(p, dict):
            return None
        east, north, up = (p.get(k) for k in ('x', 'y', 'z'))
        xyz = [north, east, -up if isinstance(up, (int, float)) else up]
    else:
        return None
    if not all(isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v) for v in xyz):
        return None
    return tuple(map(float, xyz))


def positions(rows, anchor_us):
    """[(s from the anchor, north, east, down)] from the local position reports, in time order."""
    out = []
    for topic, _, m in rows:
        if not isinstance(m, dict):
            continue
        stamp, xyz = stamp_us(m), ned(topic, m)
        if stamp is None or xyz is None:
            continue
        out.append(((stamp - anchor_us) / 1e6, *xyz))
    out.sort()
    return out


def at(series, t):
    """The series' (x, y, z) at `t`, linearly interpolated; None outside it."""
    times = [p[0] for p in series]
    i = bisect.bisect_left(times, t)
    if i < len(series) and times[i] == t:
        return series[i][1:]
    if i == 0 or i == len(series):
        return None
    (t0, *a), (t1, *b) = series[i - 1], series[i]
    f = (t - t0) / (t1 - t0)
    return tuple(a[k] + (b[k] - a[k]) * f for k in range(3))


def thin(points, limit=PLOT_POINTS):
    step = max(1, math.ceil(len(points) / limit))
    return [list(p) for p in points[::step]]


def path(old, new, tolerance, floor=PATH_FLOOR_M):
    """The path comparison: distances, the split time and the verdict."""
    result = {'tolerance': tolerance, 'floor_m': floor, 'old': thin([p[:3] for p in old]),
              'new': thin([p[:3] for p in new]), 'distance': [], 'allowed': []}
    if not old or not new:
        result.update(verdict='no data', compared=0)
        return result
    start = old[0][1:]
    distances, split, worst = [], None, (0.0, old[0][0])
    for t, *xyz in old:
        there = at(new, t)
        if there is None:
            continue
        d = math.dist(xyz, there)
        allowed = max(tolerance * math.dist(xyz, start), floor)
        distances.append((t, d, allowed))
        if d > worst[0] or not distances[:-1]:
            worst = (d, t)
        if split is None and d > allowed:
            split = t
    if not distances:
        result.update(verdict='no data', compared=0)
        return result
    result.update(
        compared=len(distances), verdict='changed' if split is not None else 'same',
        split_t=None if split is None else round(split, 3), max_m=round(worst[0], 3), max_t=round(worst[1], 3),
        mean_m=round(sum(d for _, d, _ in distances) / len(distances), 3),
        distance=thin([(t, round(d, 3)) for t, d, _ in distances]),
        allowed=thin([(t, round(a, 3)) for t, _, a in distances]))
    return result


def events(rows, anchor_us):
    """[{t, kind, value}] each time arming, mode, failsafe or landed changes."""
    out, last = [], {}
    for topic, _, m in sorted(rows, key=lambda r: stamp_us(r[2]) or 0):
        stamp = stamp_us(m)
        if stamp is None:
            continue
        t = round((stamp - anchor_us) / 1e6, 3)
        name = leaf(topic)
        found = []
        if topic == AP_STATUS:
            modes = AP_COPTER_MODES if m.get('vehicle_type') in AP_COPTERS else {}
            found = [('arming', 'Armed' if m.get('armed') else 'Disarmed'),
                     ('mode', modes.get(m.get('mode'), f'Mode {m.get("mode")}')),
                     ('failsafe', 'On' if m.get('failsafe') else 'Off'),
                     ('landed', 'Flying' if m.get('flying') else 'Landed')]
        elif name == 'vehicle_status':
            found = [('arming', ARMING.get(m.get('arming_state'), str(m.get('arming_state')))),
                     ('mode', NAV_STATES.get(m.get('nav_state'), f'Mode {m.get("nav_state")}')),
                     ('failsafe', 'On' if m.get('failsafe') else 'Off')]
        elif name == 'vehicle_land_detected':
            found = [('landed', 'Landed' if m.get('landed') else 'Flying')]
        for kind, value in found:
            if last.get(kind) != value:
                if kind in last or kind not in ('failsafe',) or value == 'On':
                    out.append({'t': t, 'kind': kind, 'value': value})
                last[kind] = value
    return out


def event_diff(old, new):
    """Pairs events by kind and value in order: [{kind, value, old_t, new_t, shift}]; a missing
    side is null."""
    rows, used = [], set()
    for e in old:
        match = next((i for i, n in enumerate(new) if i not in used and n['kind'] == e['kind']
                      and n['value'] == e['value']), None)
        if match is None:
            rows.append({'kind': e['kind'], 'value': e['value'], 'old_t': e['t'], 'new_t': None, 'shift': None})
            continue
        used.add(match)
        n = new[match]
        rows.append({'kind': e['kind'], 'value': e['value'], 'old_t': e['t'], 'new_t': n['t'],
                     'shift': round(n['t'] - e['t'], 3)})
    rows += [{'kind': n['kind'], 'value': n['value'], 'old_t': None, 'new_t': n['t'], 'shift': None}
             for i, n in enumerate(new) if i not in used]
    rows.sort(key=lambda r: r['old_t'] if r['old_t'] is not None else r['new_t'])
    return {'rows': rows, 'missing': sum(r['new_t'] is None for r in rows),
            'extra': sum(r['old_t'] is None for r in rows)}


def commands(recorded_rows, log_lines, run):
    recorded, sent = {}, {}
    for topic, _, _ in recorded_rows:
        recorded[topic] = recorded.get(topic, 0) + 1
    for line in log_lines:
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if isinstance(row, dict) and 'topic' in row:
            key = f'{row.get("source", "recording")}:{row["topic"]}'
            sent[key] = sent.get(key, 0) + 1
    topics = sorted(set(recorded) | {k.split(':', 1)[1] for k in sent})
    return {'topics': [{'topic': t, 'recorded': recorded.get(t, 0), 'from_recording': sent.get(f'recording:{t}', 0),
                        'from_your_code': sent.get(f'your code:{t}', 0)} for t in topics],
            'takeover': run.get('takeover') if isinstance(run.get('takeover'), dict) else None,
            'steering': run.get('steering', 'recording')}


def top_label(message):
    """The first output's top-scoring index from a JSON detection message, or None."""
    data = message.get('data') if isinstance(message, dict) else None
    try:
        value = json.loads(data) if isinstance(data, str) else None
    except ValueError:
        return None
    outputs = value.get('outputs') if isinstance(value, dict) else None
    if not isinstance(outputs, list) or not outputs or not isinstance(outputs[0], dict):
        return None
    first = outputs[0]
    if isinstance(first.get('top'), list) and first['top'] and isinstance(first['top'][0], list):
        return first['top'][0][0]
    values = first.get('values')
    if isinstance(values, list) and values and all(isinstance(v, (int, float)) for v in values):
        return max(range(len(values)), key=values.__getitem__)
    return None


def ai(topic, old_rows, new_rows, old_anchor_ns, new_anchor_ns):
    """Messages on one AI topic in each flight and how often the top label agrees."""
    def labelled(rows, anchor):
        return sorted(((log_ns - anchor) / 1e9, top_label(m)) for t, log_ns, m in rows if t == topic)
    old, new = labelled(old_rows, old_anchor_ns or 0), labelled(new_rows, new_anchor_ns or 0)
    result = {'topic': topic, 'old': len(old), 'new': len(new), 'paired': 0, 'agree': 0, 'disagreements': []}
    times = [t for t, _ in new]
    for t, label in old:
        if label is None:
            continue
        i = bisect.bisect_left(times, t)
        near = [k for k in (i - 1, i) if 0 <= k < len(new) and abs(new[k][0] - t) <= AI_WINDOW_S]
        if not near:
            continue
        other = new[min(near, key=lambda k: abs(new[k][0] - t))][1]
        if other is None:
            continue
        result['paired'] += 1
        if other == label:
            result['agree'] += 1
        elif len(result['disagreements']) < 10:
            result['disagreements'].append({'t': round(t, 3), 'old': label, 'new': other})
    return result


def setting_notes(run):
    """What the run changed on purpose, so a changed course is read with it in mind."""
    notes = []
    recorded, target = run.get('recorded_adapter'), run.get('target')
    if isinstance(target, str) and isinstance(recorded, str) and target and recorded and target != recorded:
        notes.append(f'Flown in another simulator ({target}, recorded in {recorded}): different physics '
                     'gives different paths even from the same commands.')
    conditions = [c for c in run.get('conditions') or [] if isinstance(c, str)]
    if conditions:
        notes.append(f'Conditions changed on purpose: {", ".join(conditions)}.')
    if run.get('home') is False:
        notes.append('The fresh vehicle did not start at the recorded home position.')
    return notes


def report(old_rows, new_rows, recorded_commands, log_lines, run, ai_topics):
    """The whole comparison from rows of both flights."""
    tolerance = run.get('tolerance')
    tolerance = tolerance / 100 if isinstance(tolerance, (int, float)) and not isinstance(tolerance, bool) \
        else DEFAULT_TOLERANCE
    old_rows, new_rows = in_final_base(old_rows), in_final_base(new_rows)
    (old_us, old_ns), (new_us, new_ns) = anchors(old_rows), anchors(new_rows)
    notes = []
    if old_us is None or new_us is None:
        notes.append('One of the flights has no vehicle status, so they cannot be put on one clock.')
        compared = {'verdict': 'no data', 'compared': 0, 'old': [], 'new': [], 'distance': [], 'allowed': []}
        old_events = new_events = []
    else:
        compared = path(positions(old_rows, old_us), positions(new_rows, new_us), tolerance)
        if compared['verdict'] == 'no data':
            notes.append('No local position in one of the flights.')
        old_events, new_events = events(old_rows, old_us), events(new_rows, new_us)
    taken = run.get('takeover') if isinstance(run.get('takeover'), dict) else None
    notes.extend(setting_notes(run))
    return {'schema': SCHEMA, 'verdict': compared['verdict'], 'tolerance': tolerance, 'path': compared,
            'events': event_diff(old_events, new_events),
            'commands': commands(recorded_commands, log_lines, run),
            'takeover_t': taken.get('t') if taken else None,
            'ai': [ai(t, old_rows, new_rows, old_ns, new_ns) for t in ai_topics],
            'notes': notes}


# -- files ----------------------------------------------------------------------------------------
def read(folder, topics, bag):
    import pipe
    if not topics:
        return []
    _, (_, _, messages) = pipe.open_messages(folder, sorted(set(topics)), None, None, bag=bag)
    return [(topic, log_ns, pipe.plain(decoded)) for topic, _, log_ns, decoded in messages]


def main(argv=None):
    recording, folder = (Path(a) for a in (argv or sys.argv[1:]))
    import pipe
    recipe = pipe.read_json(recording / 'recipe.json') or {}
    run = pipe.read_json(folder / 'run.json') or {}
    evidence = [t for t in recipe.get('evidence_topics') or [] if isinstance(t, str)]
    ai_topics = [t for t in run.get('ai_topics') or [] if isinstance(t, str)]
    command_topics = [t for t in recipe.get('command_topics') or [] if isinstance(t, str)]
    if recipe.get('source') in ('ulog', 'dataflash'):
        # A flight imported from its log: its exported reports and commands.
        old = read_jsonl(recording / 'evidence.jsonl', set(evidence)) \
            + read_jsonl(recording / 'commands.jsonl', set(command_topics))
    else:
        old = read(recording, evidence + ai_topics + command_topics, 'bag')
    recorded_commands = [r for r in old if r[0] in command_topics]
    old = [r for r in old if r[0] not in command_topics]
    try:
        from resim import variants
        new = read(folder, variants(evidence) + ai_topics, 'fresh')
    except ValueError:
        new = []
    log = folder / 'commands.jsonl'
    lines = log.read_text(errors='replace').splitlines() if log.is_file() else []
    value = report(old, new, recorded_commands, lines, run, ai_topics)
    pipe.save_json(folder / 'compare.json', value)
    print(json.dumps({'verdict': value['verdict'], 'split_t': value['path'].get('split_t')}))


if __name__ == '__main__':
    main()
