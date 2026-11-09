"""Read-only discovery and explicit install stages for the Windows setup manager."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import types
def extra_base():
    return types.SimpleNamespace(ROOT=ROOT,run=run,output=output,probe=probe,require_noble=require_noble,owned_target=owned_target)

ROOT = Path.home() / '.local/share/drone-deck/tools'
TOOLS = {'ros2', 'mavsdk', 'px4', 'dds', 'ardupilot', 'gazebo','jmavsim','ros-workspace','onnx'}

def run(args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)

def output(args):
    result = subprocess.run(args, capture_output=True, text=True, timeout=15)
    return result.stdout.strip() if result.returncode == 0 else ''

def ros_env():
    setup = '/opt/ros/jazzy/setup.bash'
    if not Path(setup).is_file():
        return None
    process = run(['bash', '-c', 'source "$1" && env -0', 'setup', setup], capture_output=True)
    return dict(item.decode().split('=', 1) for item in process.stdout.split(b'\0') if b'=' in item)

def probe(tool, location='', windows_candidate=''):
    if tool in ('gazebo','ardupilot','jmavsim','ros-workspace','onnx'):
        import extra_tools
        return extra_tools.probe(extra_base(),tool,location)
    if location == '@auto':
        location = ''
    requested = Path(location).expanduser() if location else None
    result = dict(tool=tool, found=False, compatible=False, path='', executable='', version='', reason='Not detected', managed=False,
                  environment=os.environ.get('WSL_DISTRO_NAME', ''))
    if tool == 'ros2':
        env = ros_env()
        if env:
            process = subprocess.run(['python3', '-c',
                'import rosbag2_py,yaml; assert "mcap" in rosbag2_py.get_registered_writers(); print("Jazzy + rosbag2 / MCAP")'],
                env=env, capture_output=True, text=True, timeout=15)
            result.update(found=True, path='/opt/ros/jazzy', executable='/opt/ros/jazzy/bin/ros2',
                          version='Jazzy', compatible=process.returncode == 0,
                          reason=process.stdout.strip() if process.returncode == 0 else process.stderr[-1000:])
    elif tool == 'mavsdk':
        candidates = [requested] if requested else [ROOT / 'mavsdk', Path(windows_candidate) if windows_candidate else ROOT / 'missing']
        fallback = None
        for path in candidates:
            python = path / 'bin/python' if path.is_dir() else path
            if not python or not python.is_file():
                continue
            version = output([str(python), '-c', 'import importlib.metadata; print(importlib.metadata.version("mavsdk"))'])
            if version:
                result.update(found=True, path=str(python.parent.parent), executable=str(python),
                              version=version, compatible=version.startswith('4.'),
                              reason='MAVSDK 4.x API' if version.startswith('4.') else 'The telemetry adapter requires MAVSDK 4.x')
                if result['compatible']:
                    break
                fallback = dict(result)
        if not result['compatible'] and fallback:
            result = fallback
    elif tool == 'px4':
        fallback = None
        for path in ([requested] if requested else [Path.home() / 'PX4-Autopilot', ROOT / 'PX4-Autopilot']):
            if path and (path / '.git').exists():
                version = output(['git', '-C', str(path), 'describe', '--tags', '--always'])
                target = 'px4_sitl_sih' if (path / 'build/px4_sitl_sih/bin/px4').is_file() and os.access(path / 'build/px4_sitl_sih/bin/px4', os.X_OK) else 'px4_sitl'
                binary = path / ('build/px4_sitl_sih/bin/px4' if target == 'px4_sitl_sih' else 'build/px4_sitl_default/bin/px4')
                result.update(found=True, path=str(path), executable=str(binary), version=version,
                              compatible=binary.is_file() and os.access(binary, os.X_OK), build_target=target,
                              reason='Built launch candidate; ROS message/firmware compatibility is checked separately')
                if result['compatible']:
                    break
                fallback = dict(result)
        if not result['compatible'] and fallback:
            result = fallback
    elif tool == 'dds':
        for path in ([requested] if requested else [ROOT / 'Micro-XRCE-DDS-Agent/build/MicroXRCEAgent', Path(shutil.which('MicroXRCEAgent') or '/nonexistent')]):
            executable = path / 'build/MicroXRCEAgent' if path and path.is_dir() else path
            if executable and executable.is_file() and os.access(executable, os.X_OK):
                version = output([str(executable), '--version']) or 'Unknown version'
                checkout = executable.parent.parent
                if (checkout / '.git').exists():
                    version = output(['git', '-C', str(checkout), 'describe', '--exact-match', '--tags', 'HEAD']) or version
                result.update(found=True, path=str(executable), executable=str(executable), version=version,
                              compatible='2.4.3' in version, reason='Recipe targets a DDS-v2 PX4 client; firmware settings must match')
                break
    if result['path']:
        result['managed'] = str(Path(result['path'])).startswith(str(ROOT) + '/')
    return result

def require_noble():
    release = dict(line.split('=',1) for line in Path('/etc/os-release').read_text().splitlines() if '=' in line)
    if release.get('ID','').strip('"') != 'ubuntu' or release.get('VERSION_ID','').strip('"') != '24.04':
        raise RuntimeError('Automatic Linux recipes currently support Ubuntu 24.04 only. Use existing/manual setup for other distributions.')

def dependencies(tool):
    if tool in ('gazebo','ardupilot','jmavsim','ros-workspace','onnx'):
        import extra_tools
        return extra_tools.dependencies(extra_base(),tool)
    require_noble()
    if os.geteuid() != 0:
        raise RuntimeError('This prerequisite stage needs the selected WSL distribution root user.')
    packages = ['ca-certificates', 'git', 'python3', 'python3-venv', 'python3-pip']
    if tool in ('px4', 'dds'):
        packages += ['build-essential', 'cmake', 'ninja-build', 'pkg-config', 'libssl-dev', 'libasio-dev', 'libtinyxml2-dev']
    if tool == 'px4':
        packages += ['astyle', 'ccache', 'cppcheck', 'file', 'gdb', 'lcov', 'libxml2-dev', 'libxml2-utils',
                     'python3-dev', 'python3-setuptools', 'python3-wheel', 'rsync', 'unzip', 'zip']
    env = {**os.environ, 'DEBIAN_FRONTEND': 'noninteractive'}
    run(['apt-get', 'update'], env=env)
    run(['apt-get', 'install', '-y', '--no-install-recommends', *packages], env=env)

def owned_target(name):
    ROOT.mkdir(parents=True, exist_ok=True)
    path = ROOT / name
    marker = ROOT / ('.owned-' + name)
    if path.exists() and not marker.is_file():
        raise RuntimeError(f'{path} already exists outside this installer. Use Detect/reuse or choose a fresh managed location.')
    marker.write_text('Drone Deck managed target\n')
    return path

def install(tool):
    if tool in ('gazebo','ardupilot','jmavsim','ros-workspace','onnx'):
        import extra_tools
        return extra_tools.install(extra_base(),tool)
    require_noble()
    if os.geteuid() == 0:
        raise RuntimeError('User tool environments must not be created as Linux root. Initialize a regular Linux user first.')
    if tool == 'mavsdk':
        path = owned_target('mavsdk')
        if not (path / 'bin/python').is_file():
            run(['python3', '-m', 'venv', str(path)])
        run([str(path / 'bin/python'), '-m', 'pip', 'install', 'mavsdk==4.1.0'])
        (path / 'drone-deck-packages.txt').write_text(output([str(path / 'bin/python'), '-m', 'pip', 'freeze']))
    elif tool == 'px4':
        path = owned_target('PX4-Autopilot')
        if not (path / '.git').exists():
            run(['git', 'clone', '--recursive', '--branch', 'v1.17.0', 'https://github.com/PX4/PX4-Autopilot.git', str(path)])
        if output(['git', '-C', str(path), 'describe', '--exact-match', '--tags', 'HEAD']) != 'v1.17.0':
            raise RuntimeError('Managed PX4 checkout no longer matches v1.17.0; review local changes manually.')
        # Retry incomplete recursive clones without resetting local source changes.
        run(['git', '-C', str(path), 'submodule', 'update', '--init', '--recursive'])
        env_path = owned_target('px4-python')
        if not (env_path / 'bin/python').exists():
            run(['python3', '-m', 'venv', str(env_path)])
        run([str(env_path / 'bin/python'), '-m', 'pip', 'install', '-r', str(path / 'Tools/setup/requirements.txt')])
        env = {**os.environ, 'PATH': str(env_path / 'bin') + ':' + os.environ['PATH'], 'DONT_RUN': '1'}
        run(['make', '-j2', 'px4_sitl_sih', 'sihsim_quadx'], cwd=path, env=env)
        (env_path / 'drone-deck-packages.txt').write_text(output([str(env_path / 'bin/python'), '-m', 'pip', 'freeze']))
    elif tool == 'dds':
        path = owned_target('Micro-XRCE-DDS-Agent')
        if not (path / '.git').exists():
            run(['git', 'clone', '--branch', 'v2.4.3', 'https://github.com/eProsima/Micro-XRCE-DDS-Agent.git', str(path)])
        if output(['git', '-C', str(path), 'describe', '--exact-match', '--tags', 'HEAD']) != 'v2.4.3':
            raise RuntimeError('Managed Agent checkout no longer matches v2.4.3; review it manually.')
        run(['cmake', '-S', str(path), '-B', str(path / 'build'), '-DCMAKE_BUILD_TYPE=Release'])
        run(['cmake', '--build', str(path / 'build'), '-j2'])
    else:
        raise RuntimeError('Unsupported user installation recipe.')

def preflight(tool):
    require_noble()
    minimum = {'mavsdk': 1, 'px4': 10, 'dds': 3,'gazebo':5,'ardupilot':8,'jmavsim':3,'ros-workspace':3,'onnx':2}.get(tool, 1)
    if shutil.disk_usage(Path.home()).free < minimum * 1024 ** 3:
        raise RuntimeError(f'This recipe needs at least {minimum} GiB free in the Linux filesystem; source builds may need more.')
    names = {'mavsdk': ['mavsdk'], 'px4': ['PX4-Autopilot', 'px4-python'], 'dds': ['Micro-XRCE-DDS-Agent'],'ardupilot':['ardupilot','ardupilot-python'],'jmavsim':['jMAVSim'],'ros-workspace':['px4_ros_ws'],'onnx':['onnx']}
    for name in names.get(tool, []):
        if (ROOT / name).exists() and not (ROOT / ('.owned-' + name)).is_file():
            raise RuntimeError('Existing unowned target must be adopted manually: ' + str(ROOT / name))

def verify(tool):
    managed = {'mavsdk': ROOT / 'mavsdk', 'px4': ROOT / 'PX4-Autopilot',
               'dds': ROOT / 'Micro-XRCE-DDS-Agent','ardupilot':ROOT/'ardupilot','jmavsim':ROOT/'jMAVSim','ros-workspace':ROOT/'px4_ros_ws','onnx':ROOT/'onnx'}
    return probe(tool, str(managed[tool]) if tool in managed else '')

if __name__ == '__main__':
    try:
        action, tool = sys.argv[1:3]
        if tool not in TOOLS:
            raise ValueError('Unknown tool recipe.')
        if action == 'probe':
            print(json.dumps(probe(tool, *(sys.argv[3:] + ['', ''])[:2])))
        elif action == 'dependencies':
            dependencies(tool)
        elif action == 'install':
            install(tool)
        elif action == 'preflight':
            preflight(tool)
        elif action == 'verify':
            print(json.dumps(verify(tool)))
        else:
            raise ValueError('Unknown setup stage.')
    except Exception as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1)
