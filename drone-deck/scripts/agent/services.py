"""rec.* and data.* methods: the rosbag2/MCAP recorder and the local data helpers.

They reuse `scripts/recordings.py` and `scripts/studio/data.py`, loaded on first use, and route
what those modules print into agent events: recorder output becomes `rec` events, data progress
becomes `progress` events for the request that caused it. ROS and PyYAML are only imported by
those modules inside the methods that need them.
"""
import importlib.util
from pathlib import Path
import threading

SCRIPTS = Path(__file__).resolve().parent.parent
RECORDINGS = '~/.local/share/drone-deck/recordings'


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / relative)
    if spec is None or spec.loader is None:
        raise RuntimeError(f'Helper is missing: {relative}')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Recorder:
    ACTIONS = ('check', 'topics', 'record', 'stop', 'list', 'replay')

    def __init__(self, out, root=RECORDINGS, module=None, tick=0.25):
        self.out = out
        self.root = root
        self.module = module
        self.tick_interval = tick
        self.server = None
        self.lock = threading.Lock()
        self.closed = threading.Event()

    def _server(self):
        if self.server is None:
            module = self.module or load('drone_deck_recordings', 'recordings.py')
            module.emit = lambda value: self.out.event('rec', data=value)
            self.server = module.Server(self.root)
            threading.Thread(target=self._tick, daemon=True).start()
        return self.server

    def _tick(self):
        while not self.closed.wait(self.tick_interval):
            with self.lock:
                try:
                    self.server.tick()
                except Exception as error:  # noqa: BLE001 - reported, the ticker keeps going
                    self.out.log('error', f'Recording worker: {error}')

    def call(self, action, params):
        if action not in self.ACTIONS:
            raise ValueError(f'Unknown recording method rec.{action}.')
        request = dict(params or {})
        request['action'] = action
        with self.lock:
            return self._server().dispatch(request)

    def close(self):
        """Finalizes an active recording or replay, as the old worker did on shutdown."""
        self.closed.set()
        with self.lock:
            if self.server is not None:
                self.server.stop()


class Data:
    """data.* calls run concurrently (listing the library never waits behind creating a
    project); each call's progress goes to the request that caused it."""
    METHODS = {'library': 'data_library', 'project_create': 'project_create'}

    def __init__(self, module=None):
        self.module = module
        self.lock = threading.Lock()
        self.current = threading.local()

    def _emit(self, value):
        progress = getattr(self.current, 'progress', None)
        if progress is not None and isinstance(value, dict) and 'progress' in value:
            progress(str(value['progress']))

    def _loaded(self):
        with self.lock:
            if self.module is None:
                self.module = load('drone_deck_studio_data', 'studio/data.py')
            self.module.emit = self._emit
            return self.module

    def call(self, name, params, progress):
        action = self.METHODS.get(name)
        if action is None:
            raise ValueError(f'Unknown data method data.{name}.')
        request = dict(params or {})
        request['action'] = action
        module = self._loaded()
        self.current.progress = progress
        try:
            return module.dispatch(request)
        finally:
            self.current.progress = None
