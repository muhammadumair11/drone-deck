"""Isolated diagnostics: mock tool discovery, never install or launch a tool."""
import importlib.util
import json
from pathlib import Path
import tempfile
from unittest.mock import patch

source = Path(r'C:\Users\HP\Desktop\DroneProjects\drone-control-center\runtime\setup\linux-tools.py')
spec = importlib.util.spec_from_file_location('setup_probe', source)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
rows = []

def binary(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text('diagnostic fixture only')
    return path

audit_root = Path(__file__).resolve().parent
with tempfile.TemporaryDirectory(prefix='fixture-', dir=audit_root) as temporary:
    base = Path(temporary)
    assert base.resolve().is_relative_to(audit_root)
    home = base / 'home'
    managed = home / '.local/share/drone-deck/tools'
    old = home / 'PX4-Autopilot'
    good = managed / 'PX4-Autopilot'
    (old / '.git').mkdir(parents=True)
    (good / '.git').mkdir(parents=True)
    binary(good / 'build/px4_sitl_default/bin/px4')
    with patch.object(module, 'ROOT', managed), patch.object(Path, 'home', return_value=home), patch.object(module, 'output', return_value='v1.17.0'), patch.object(module.os, 'access', return_value=True):
        result = module.probe('px4')
        rows.append({'case': 'Incomplete first PX4 checkout hides valid managed build', 'observed': result, 'confirmed': result['found'] and not result['compatible'] and result['path'] == str(old)})
        (good / 'build/px4_sitl_sih').mkdir()
        result = module.probe('px4', str(good))
        rows.append({'case': 'Empty SIH build directory masks executable default build', 'observed': result, 'confirmed': not result['compatible'] and result['build_target'] == 'px4_sitl_sih'})
    first = binary(managed / 'mavsdk/bin/python')
    second = binary(base / 'project-venv/bin/python')
    def version(args):
        return '3.10.2' if args[0] == str(first) else '4.1.0'
    with patch.object(module, 'ROOT', managed), patch.object(module, 'output', side_effect=version):
        result = module.probe('mavsdk', windows_candidate=str(second.parent.parent))
        rows.append({'case': 'Incompatible first MAVSDK environment hides compatible project environment', 'observed': result, 'confirmed': not result['compatible'] and result['version'] == '3.10.2'})
    custom = binary(base / 'custom-gazebo/gz')
    with patch.object(module.shutil, 'which', return_value=None):
        result = module.probe('gazebo', str(custom))
        rows.append({'case': 'Gazebo optional location is ignored', 'observed': result, 'confirmed': not result['found']})

print(json.dumps(rows, indent=2))
assert all(row['confirmed'] for row in rows)
