"""Local project library and project scaffolding inside the selected WSL distribution.

Frame extraction and inference on replay live in `scripts/agent/pipe.py` (`pipe.*`)."""
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import sys
import uuid

ROOT = Path.home() / '.local/share/drone-deck'
NAME = re.compile(r'^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$')
KINDS = ('onnx', 'ros2', 'mavsdk')
README = {
    'onnx': 'Use the managed ONNX environment with an ONNX model and NPZ named tensors. Optional integer '
            'targets export classification predictions.csv; latency is amortized batch time.\n'
            'To customise inference on replayed recordings (Data & models → Pipelines), copy '
            'scripts/agent/infer_node.py here as infer_node.py and edit it.\n',
    'mavsdk': 'Use the managed MAVSDK environment. This starter only reads telemetry and never arms or flies. '
              'It listens on udpin://0.0.0.0:14540 inside WSL, where PX4 SITL sends its API stream; pass '
              '--endpoint for another source. Drone Deck\'s scripts fan-out (default 127.0.0.1:14540) is for '
              'scripts running on Windows.\n',
}


def emit(value):
    print(json.dumps(value), flush=True)


def named(value):
    if not isinstance(value, str) or not NAME.fullmatch(value):
        raise ValueError('Use 1–64 letters, numbers, underscores or hyphens.')
    return value


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + '.' + uuid.uuid4().hex + '.tmp')
    temporary.write_text(json.dumps(value, indent=2), encoding='utf-8')
    temporary.replace(path)


def library():
    projects = []
    for path in sorted((ROOT / 'projects').glob('*/project.json'))[-100:]:
        try:
            value = json.loads(path.read_text())
            value['manifest'] = str(path)
            projects.append(value)
        except (OSError, ValueError):
            pass
    return dict(projects=projects, message='Local project library loaded.')


def project_create(request):
    name = named(request['name'])
    kind = request.get('kind', 'onnx')
    if kind not in KINDS:
        raise ValueError('Choose ONNX, ROS 2 or MAVSDK.')
    path = ROOT / 'projects' / name
    if path.exists():
        raise ValueError('That project already exists. Choose a new name; existing files are preserved.')
    path.mkdir(parents=True)
    # Incomplete source is kept for inspection on failure; user project material is never deleted.
    manifest = dict(name=name, kind=kind, path=str(path), model=str(request.get('model', '')),
                    distro=os.environ.get('WSL_DISTRO_NAME', ''),
                    created=datetime.now(timezone.utc).isoformat(), schema=1)
    template = Path(__file__).with_name('templates') / (kind + '.py')
    (path / 'main.py').write_text(template.read_text(), encoding='utf-8')
    (path / 'README.md').write_text(
        '# ' + name + '\n\nGenerated ' + kind + ' project.\n\nRun in the selected WSL distribution. '
        'No dependency installation or flight commands were performed by project creation.\n'
        + README.get(kind, 'Use the matching environment configured in Setup. This starter monitors data '
                           'and never arms or flies.\n'), encoding='utf-8')
    save(path / 'project.json', manifest)
    return dict(message='Project created without installing dependencies or starting code.', project=manifest)


def dispatch(request):
    action = request['action']
    return {'data_library': library, 'project_create': lambda: project_create(request)}[action]()


if __name__ == '__main__':
    request = {}
    try:
        request = json.load(sys.stdin)
        emit(dict(ok=True, action=request.get('action'), data=dispatch(request)))
    except Exception as error:
        emit(dict(ok=False, action=request.get('action'), error=str(error)))
