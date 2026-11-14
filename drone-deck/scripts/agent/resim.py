"""resim.* methods: re-simulation of a recorded flight in a fresh simulator Drone Deck started.

- `resim.list` → `{flights: [{id, label, path, recipe, runs}], root}`: finalized recordings that
  keep a `recipe.json`, with a summary of it and their runs (`<recording>/resim/<run>/run.json`).
- `resim.prepare{recording}` → `{recipe, params}`: the recipe and the kept parameters
  (`params.params`, QGroundControl format, or null) the app needs before it starts the simulator.
- `resim.run{recording, namespace, domain, tolerance?, floor?, hold?, code?, ai_topics?, target?,
  conditions?, home?, skipped_params?}` starts two owned processes on the
  run's isolated, localhost-only domain: a rosbag2 recorder of the fresh vehicle's reports
  (`fresh/`) and the command player (`arbiter.py`), and returns when the player ends or the
  request is cancelled. `run.json` records the settings, the parameters the app left out, the
  commands replayed and the exit states; `commands.jsonl` lists each command replayed. With a
  `tolerance` (percent; null is off), the user's code, which the app started on the same domain,
  runs in shadow and takes over once it differs for `hold` seconds (`takeover.py`); `run.json`
  then records when, on which topic and why.
- After a run that recorded the fresh vehicle, `compare.py` writes `compare.json` beside it (path,
  events, commands, AI output; run in a worker with the ROS environment so either MCAP reader
  works) and `run.json` gets the verdict. `resim.compare{recording, run}` compares again and
  `resim.report{recording, run}` returns `compare.json`.
- `resim.import{label, folder?, mcap?}` files a real flight in the library: `folder` is what the
  app exported from its flight log (recipe, `commands.jsonl`, `evidence.jsonl`, parameters, the
  log); `mcap` is the drone's own ROS 2 recording, whose commands are preferred when it has any.
- `resim.status` → `{active}`: whether a run is still writing its folder.

The app starts and stops the simulator itself, through its staged launch; this module never
touches it.
"""
import contextlib
import json
import os
import re
import shutil
from pathlib import Path
import shlex
import subprocess
import threading
import uuid

import pipe

KEYS = {'record': 'resim-fresh', 'arbiter': 'resim-arbiter'}
PARAMS_LIMIT = 4 * 1024 * 1024
REPORT_LIMIT = 8 * 1024 * 1024
COMPARE_TIMEOUT_S = 600
# The command player's own limits: how long it waits for the fresh vehicle's first status, and
# how long it keeps relaying after the last command.
WAIT_S = 90
TAIL_S = 5


IMPORT_FILES = ('recipe.json', 'commands.jsonl', 'evidence.jsonl', 'params.params', 'flight.ulg')
IMPORT_LIMIT = 2 * 1024 ** 3


def recipe_summary(recipe):
    world = recipe.get('world') if isinstance(recipe.get('world'), dict) else {}
    return {'origin': (pipe.text(recipe.get('origin')) or 'simulated'), 'source': (pipe.text(recipe.get('source')) or ''),'adapter': (pipe.text(recipe.get('adapter')) or ''), 'autopilot': (pipe.text(recipe.get('autopilot')) or ''),
            'world': (pipe.text(world.get('id')) or ''), 'system': (pipe.text(recipe.get('system')) or ''),
            'commands': len(recipe.get('command_topics') or []), 'params': bool(recipe.get('params'))}


def variants(names):
    """Each `/fmu/out/` topic with PX4's versioned names too (`vehicle_status_v1`): a flight log
    names topics without the version a fresh PX4 may publish under."""
    out = []
    for name in names:
        out.append(name)
        if name.startswith('/fmu/out/') and not re.search(r'_v[0-9]+$', name):
            out += [f'{name}_v{n}' for n in (1, 2, 3)]
    return list(dict.fromkeys(out))


def topics(recipe, key):
    found = recipe.get(key) if isinstance(recipe.get(key), list) else []
    return [t for t in found if isinstance(t, str) and pipe.TOPIC.fullmatch(t)]


def run_plan(params, root, python='python3'):
    """Validates `resim.run` parameters and builds the two commands (no process starts)."""
    recording, _ = pipe.recording_path(root, params.get('recording'))
    recipe = pipe.read_json(recording / 'recipe.json')
    if not isinstance(recipe, dict):
        raise ValueError('This recording keeps no re-simulation recipe.')
    if not topics(recipe, 'command_topics'):
        raise ValueError('The recording kept no ROS 2 commands to replay.')
    namespace = params.get('namespace') or 'resim'
    if not isinstance(namespace, str) or not pipe.NAME.fullmatch(namespace):
        raise ValueError('The DDS namespace must be a simple name.')
    try:
        domain = int(params.get('domain', 79))
    except (TypeError, ValueError):
        raise ValueError('The run domain must be a number.') from None
    if not 1 <= domain <= 232:
        raise ValueError('The run domain must be between 1 and 232.')
    skipped = params.get('skipped_params') or []
    if not isinstance(skipped, list):
        raise ValueError('skipped_params must be a list.')
    tolerance, floor, hold = params.get('tolerance'), params.get('floor', 0.0), params.get('hold', 0.5)
    try:
        tolerance = None if tolerance is None else float(tolerance)
        floor, hold = float(floor), float(hold)
    except (TypeError, ValueError):
        raise ValueError('Tolerance, floor and hold time must be numbers.') from None
    if tolerance is not None and not 0 <= tolerance <= 100:
        raise ValueError('Tolerance must be between 0 and 100 percent, or off.')
    if not 0 <= floor <= 1000 or not 0 <= hold <= 60:
        raise ValueError('The floor must be 0–1000 and the hold time 0–60 seconds.')
    code = params.get('code')
    if code is not None and not (isinstance(code, str) and pipe.NAME.fullmatch(code)):
        raise ValueError('Your code: use the project name.')
    ai_topics = params.get('ai_topics') or []
    if not isinstance(ai_topics, list) or len(ai_topics) > 20 \
            or not all(isinstance(x, str) and pipe.TOPIC.fullmatch(x) for x in ai_topics):
        raise ValueError('AI topics must be up to 20 absolute topic names, for example /drone_deck/detections.')
    target = params.get('target')
    if target is not None and not (isinstance(target, str) and pipe.NAME.fullmatch(target)):
        raise ValueError('The target simulator must be an adapter id.')
    conditions = params.get('conditions') or []
    if not isinstance(conditions, list) or len(conditions) > 10             or not all(isinstance(c, str) and pipe.NAME.fullmatch(c) for c in conditions):
        raise ValueError('Conditions must be a short list of names.')
    run = f'{pipe.stamp()}_{uuid.uuid4().hex[:8]}'
    folder = recording / 'resim' / run
    reports = variants(topics(recipe, 'evidence_topics')) + [x for x in ai_topics if x not in topics(recipe, 'evidence_topics')]
    commands = {
        'arbiter': [python, '-u', str(pipe.HERE / 'arbiter.py'), '--recording', str(recording),
                    '--namespace', namespace, '--out', str(folder), '--wait', str(WAIT_S), '--tail', str(TAIL_S),
                    '--tolerance', 'off' if tolerance is None else f'{tolerance:g}',
                    '--floor', f'{floor:g}', '--hold', f'{hold:g}'],
    }
    if reports:
        commands['record'] = ['ros2', 'bag', 'record', '-s', 'mcap', '-o', str(folder / 'fresh'),
                              '--disable-keyboard-controls', '--topics', *reports]
    env = dict(pipe.LOCAL_ONLY, ROS_DOMAIN_ID=str(domain), DRONE_DECK_AGENT=str(pipe.HERE))
    return {'run': run, 'folder': folder, 'recording': recording, 'recipe': recipe, 'namespace': namespace,
            'domain': domain, 'skipped': skipped[:5000], 'commands': commands, 'env': env,
            'tolerance': tolerance, 'floor': floor, 'hold': hold, 'code': code, 'ai_topics': ai_topics,
            'target': target or (pipe.text(recipe.get('adapter')) or ''), 'conditions': conditions,
            'home': bool(params.get('home'))}


def run_folder(root, params):
    """(recording, run folder) named by `resim.report` / `resim.compare` parameters."""
    recording, _ = pipe.recording_path(root, params.get('recording'))
    name = params.get('run')
    if not isinstance(name, str) or not pipe.NAME.fullmatch(name):
        raise ValueError('Select a run.')
    folder = recording / 'resim' / name
    if folder.is_symlink() or not (folder / 'run.json').is_file():
        raise ValueError(f'Run {name} does not exist.')
    return recording, folder


def read_mcap_topics(path):
    """Topic names in an MCAP file, from its summary."""
    try:
        from mcap.reader import make_reader
    except ImportError:
        raise RuntimeError(pipe.MISSING_READER) from None
    with path.open('rb') as stream:
        summary = make_reader(stream).get_summary()
    if summary is None:
        raise ValueError('The MCAP file has no summary; it may still be recording or damaged.')
    return sorted({channel.topic for channel in summary.channels.values()})


def compare_worker(recording, folder, python='python3'):
    """Runs `compare.py` with the ROS environment; returns its summary or raises."""
    env = dict(pipe.ros_environment(), DRONE_DECK_AGENT=str(pipe.HERE))
    done = subprocess.run([python, '-u', str(pipe.HERE / 'compare.py'), str(recording), str(folder)],
                          capture_output=True, text=True, timeout=COMPARE_TIMEOUT_S, env=env)
    if done.returncode:
        lines = (done.stderr or '').strip().splitlines()
        raise RuntimeError(lines[-1] if lines else f'compare.py exited with code {done.returncode}')
    return json.loads(done.stdout.strip().splitlines()[-1])


def player_status(folder):
    """The command player's last `status.json`, or {}."""
    value = pipe.read_json(folder / 'status.json', limit=64_000)
    return value if isinstance(value, dict) else {}


def progress_text(status):
    if not status.get('anchored'):
        return 'Waiting for the fresh vehicle\'s first status'
    taken = status.get('takeover') if isinstance(status.get('takeover'), dict) else None
    if taken:
        return f'Your code steering since {float(taken.get("t", 0)):.1f} s · {float(status.get("t", 0)):.1f} s'
    return (f'{status.get("sent", 0)} of {status.get("commands", 0)} commands replayed · '
            f'{float(status.get("t", 0)):.1f} s')


class Resim:
    """The agent's resim.* methods. `processes` is the agent's `procs.Processes`."""

    POLL_S = 0.5

    def __init__(self, out, processes, root=pipe.RECORDINGS, comparer=compare_worker):
        self.out = out
        self.procs = processes
        self.comparer = comparer
        self.root = Path(os.path.expanduser(root))
        self.lock = threading.Lock()
        self.active = None

    def call(self, name, params, cancel, progress):
        table = {'list': self.list, 'status': self.status,
                 'prepare': lambda: self.prepare(params or {}),
                 'run': lambda: self.run(params or {}, cancel, progress),
                 'report': lambda: self.report(params or {}),
                 'compare': lambda: self.compare(*run_folder(self.root, params or {})),
                 'import': lambda: self.import_flight(params or {})}
        if name not in table:
            raise ValueError(f'Unknown re-simulation method resim.{name}.')
        return table[name]()

    def status(self):
        return {'active': self.active is not None}

    def import_flight(self, params, mcap_topics=None):
        """Files a real flight (exported log folder and/or MCAP) as a re-simulable recording."""
        label = str(params.get('label') or 'Imported flight').strip()[:80] or 'Imported flight'
        folder = Path(params['folder']) if params.get('folder') else None
        mcap = Path(params['mcap']) if params.get('mcap') else None
        if folder is None and mcap is None:
            raise ValueError('Choose a flight log or a ROS 2 recording to import.')
        if folder is not None and not (folder / 'recipe.json').is_file():
            raise ValueError('The exported flight is missing its recipe.')
        if mcap is not None and (mcap.suffix.lower() != '.mcap' or not mcap.is_file()
                                 or mcap.stat().st_size > IMPORT_LIMIT):
            raise ValueError('Choose an .mcap file (up to 2 GiB) the Linux environment can read.')
        recipe = pipe.read_json(folder / 'recipe.json', limit=4_000_000) if folder else None
        topics = []
        if mcap is not None:
            topics = (mcap_topics or read_mcap_topics)(mcap)
            commands = [x for x in topics if x.startswith('/fmu/in/')]
            if commands:
                recipe = dict(recipe or {}, source='', command_topics=commands,
                              evidence_topics=[x for x in topics if x.startswith('/fmu/out/')])
            elif recipe is None:
                raise ValueError('The ROS 2 recording has no /fmu/in/ commands to replay.')
        recipe = dict({'schema': 1, 'origin': 'real', 'system': label, 'adapter': 'px4-sih', 'autopilot': 'px4',
                       'world': {'id': 'internal', 'params': {}}, 'command_topics': [], 'evidence_topics': [],
                       'anchor': 'first vehicle status', 'drone_deck': ''}, **(recipe or {}))
        slug = re.sub(r'[^a-z0-9]+', '-', label.lower()).strip('-')[:60] or 'flight'
        path = self.root / f'{pipe.stamp()}-{slug}'
        path.mkdir(parents=True)
        if folder is not None:
            for name in IMPORT_FILES[1:]:
                if (folder / name).is_file():
                    shutil.copyfile(folder / name, path / name)
        if mcap is not None:
            (path / 'bag').mkdir()
            shutil.copyfile(mcap, path / 'bag' / mcap.name)
        pipe.save_json(path / 'recipe.json', recipe)
        pipe.save_json(path / 'session.json', {'status': 'finalized', 'label': label, 'origin': 'real',
                                               'source': 'import', 'created': pipe.now(), 'domain': 0})
        return {'id': path.name, 'label': label, 'recipe': recipe_summary(recipe)}

    def report(self, params):
        _, folder = run_folder(self.root, params)
        value = pipe.read_json(folder / 'compare.json', limit=REPORT_LIMIT)
        if not isinstance(value, dict):
            raise ValueError('This run has not been compared yet. Compare it again.')
        return value

    def compare(self, recording, folder):
        """Compares a run with its flight; the verdict (or why not) goes into run.json."""
        run = pipe.read_json(folder / 'run.json') or {}
        try:
            summary = self.comparer(recording, folder)
            run.update(verdict=summary.get('verdict'), split_t=summary.get('split_t'))
            run.pop('compare_error', None)
        except Exception as error:  # noqa: BLE001 - kept in run.json and returned
            run['compare_error'] = str(error) or type(error).__name__
        pipe.save_json(folder / 'run.json', run)
        return run

    def list(self):
        flights = []
        if not self.root.is_dir():
            return {'flights': flights, 'root': str(self.root)}
        for path in sorted(self.root.iterdir(), key=lambda p: p.name, reverse=True)[:300]:
            if not path.is_dir() or path.is_symlink() or not pipe.SESSION.fullmatch(path.name):
                continue
            recipe = pipe.read_json(path / 'recipe.json', limit=64 * 1024)
            session = pipe.read_json(path / 'session.json')
            if not isinstance(recipe, dict) or session is None \
                    or str(session.get('status', '')).lower() != 'finalized':
                continue
            flights.append({'id': path.name, 'label': str(session.get('label') or path.name), 'path': str(path),
                            'recipe': recipe_summary(recipe), 'runs': self._runs(path)})
            if len(flights) >= 100:
                break
        return {'flights': flights, 'root': str(self.root)}

    def _runs(self, recording):
        rows, base = [], recording / 'resim'
        if not base.is_dir():
            return rows
        for path in sorted(base.iterdir(), reverse=True)[:50]:
            value = pipe.read_json(path / 'run.json') if path.is_dir() else None
            if not isinstance(value, dict):
                continue
            status = pipe.text(value.get('status')) or 'Unknown'
            if status == 'Running' and path != self.active:
                status = 'Interrupted'
            taken = value.get('takeover') if isinstance(value.get('takeover'), dict) else None
            rows.append({'name': path.name, 'path': str(path), 'status': status,
                         'code': (pipe.text(value.get('code')) or ''),
                         'target': (pipe.text(value.get('target')) or ''),
                         'conditions': [c for c in value.get('conditions') or [] if isinstance(c, str)],
                         'takeover': (f'{taken.get("t")} s, {pipe.text(taken.get("topic")) or ""}: '
                                      f'{pipe.text(taken.get("reason")) or ""}') if taken else '',
                         'created': (pipe.text(value.get('created')) or ''), 'commands': pipe.count(value.get('commands')) or 0,
                         'sent': pipe.count(value.get('sent')) or 0, 'error': (pipe.text(value.get('error')) or ''),
                         'verdict': pipe.text(value.get('verdict')) or '',
                         'split_t': value.get('split_t') if isinstance(value.get('split_t'), (int, float)) else None,
                         'compare_error': pipe.text(value.get('compare_error')) or ''})
        return rows

    def prepare(self, params):
        recording, _ = pipe.recording_path(self.root, params.get('recording'))
        recipe = pipe.read_json(recording / 'recipe.json', limit=64 * 1024)
        if not isinstance(recipe, dict):
            raise ValueError('This recording keeps no re-simulation recipe.')
        kept, text = recording / 'params.params', None
        if kept.is_file() and not kept.is_symlink() and kept.stat().st_size <= PARAMS_LIMIT:
            text = kept.read_text(errors='replace')
        return {'recipe': recipe, 'params': text}

    def run(self, params, cancel, progress):
        plan = run_plan(params, self.root)
        if not self.lock.acquire(blocking=False):
            raise RuntimeError('A re-simulation is already running. Stop it first.')
        try:
            return self._run(plan, cancel, progress)
        finally:
            self.active = None
            self.lock.release()

    def _start(self, part, plan):
        # As in pipe.Pipelines._start: hold the table's lock until the entry is in hand.
        with getattr(self.procs, 'lock', None) or contextlib.nullcontext():
            self.procs.run(KEYS[part], pipe.ros_command(plan['commands'][part]), plan['env'])
            return getattr(self.procs, 'entries', {}).get(KEYS[part])

    @staticmethod
    def _running(entry):
        return entry is not None and not entry.finished.is_set()

    def _run(self, plan, cancel, progress):
        folder = plan['folder']
        folder.mkdir(parents=True)
        self.active = folder
        run = {'schema': 1, 'kind': 'resimulation', 'run': plan['run'], 'recording': plan['recording'].name,
               'status': 'Running', 'created': pipe.now(), 'adapter': (pipe.text(plan['recipe'].get('adapter')) or ''),
               'world': plan['recipe'].get('world'), 'namespace': plan['namespace'], 'domain': plan['domain'],
               'tolerance': 'off' if plan['tolerance'] is None else plan['tolerance'], 'floor': plan['floor'],
               'hold_s': plan['hold'], 'code': plan['code'], 'ai_topics': plan['ai_topics'], 'steering': 'recording',
               'target': plan['target'], 'recorded_adapter': (pipe.text(plan['recipe'].get('adapter')) or ''),
               'conditions': plan['conditions'], 'home': plan['home'],
               'skipped_params': plan['skipped'],
               'commands_run': {k: shlex.join(v) for k, v in plan['commands'].items()}, 'exits': {}}
        pipe.save_json(folder / 'run.json', run)
        entries, failure = {}, None
        try:
            if 'record' in plan['commands']:
                progress('Starting the recorder of the fresh vehicle')
                entries['record'] = self._start('record', plan)
            progress('Starting the command player')
            entries['arbiter'] = self._start('arbiter', plan)
            while self._running(entries['arbiter']) and not cancel.is_set():
                if 'record' in entries and not self._running(entries['record']):
                    failure = 'The recorder of the fresh vehicle exited during the run. See its output.'
                    break
                progress(progress_text(player_status(folder)))
                cancel.wait(self.POLL_S)
        except Exception as error:  # noqa: BLE001 - recorded in run.json, then re-raised
            failure = str(error) or type(error).__name__
        finally:
            progress('Stopping the run processes')
            for part, grace in (('arbiter', 5), ('record', 20)):
                if part not in entries:
                    continue
                try:
                    self.procs.stop(KEYS[part], grace)
                except Exception as error:  # noqa: BLE001 - keep stopping the other
                    failure = failure or f'Stopping {part}: {error}'
                entry = entries[part]
                if entry is not None:
                    entry.finished.wait(grace + 5)
                child = getattr(entry, 'child', None)
                run['exits'][part] = None if child is None else child.returncode
            code = run['exits'].get('arbiter')
            if failure is None and not cancel.is_set() and code not in (0, None):
                failure = (f'The command player exited with code {code}: '
                           f'{player_status(folder).get("error") or "see its output in Activity / Process output"}.')
            status = player_status(folder)
            run.update(status='Failed' if failure else 'Cancelled' if cancel.is_set() else 'Completed',
                       finished=pipe.now(), commands=status.get('commands', 0), sent=status.get('sent', 0),
                       before_anchor=status.get('before_anchor', 0),
                       steering=status.get('steering', 'recording'),
                       compared=status.get('compared', 0), differed=status.get('differed', 0), outputs=[str(p) for p in sorted((folder / 'fresh').glob('*.mcap'))][:32])
            if isinstance(status.get('takeover'), dict):
                run['takeover'] = status['takeover']
            if failure:
                run['error'] = failure
            pipe.save_json(folder / 'run.json', run)
        if failure:
            raise RuntimeError(f'{failure} Run record: {folder / "run.json"}')
        if run['outputs']:
            progress('Comparing the two flights')
            run = self.compare(plan['recording'], folder)
        return dict(run, folder=str(folder))
