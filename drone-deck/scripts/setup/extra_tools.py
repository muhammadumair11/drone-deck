"""Explicit add-on recipes. No command in this module runs at import time."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import urllib.request
import zipfile

TOOLS={'gazebo','ardupilot','jmavsim','ros-workspace','onnx'}
def probe(base,tool,location=''):
    root=base.ROOT
    result=dict(tool=tool,found=False,compatible=False,path='',executable='',version='',reason='Not detected',managed=False,environment=os.environ.get('WSL_DISTRO_NAME',''))
    requested=Path(location).expanduser() if location and location!='@auto' else None
    if tool=='gazebo':
        executable=str(requested/'gz' if requested and requested.is_dir() else requested) if requested else shutil.which('gz')
        if executable:
            version=base.output([executable,'sim','--versions'])
            compatible=any(part.strip().startswith('8.') for part in version.splitlines())
            result.update(found=Path(executable).is_file(),compatible=compatible,path=executable,executable=executable,version=version,reason='Harmonic runtime verified; the PX4 bridge/world is checked by its launch target.' if compatible else 'This adapter requires Gazebo Harmonic (gz-sim 8.x).')
    elif tool=='ardupilot':
        candidates=[requested] if requested else [Path.home()/'ardupilot',root/'ardupilot']
        for path in candidates:
            binary=path/'build/sitl/bin/arducopter'
            if (path/'Tools/autotest/sim_vehicle.py').is_file():
                compatible=binary.is_file() and os.access(binary,os.X_OK)
                result.update(found=True,compatible=compatible,path=str(path),executable=str(binary),version=base.output(['git','-C',str(path),'describe','--tags','--always']),reason='Built multirotor SITL; ground-station connection uses TCP 5760.' if compatible else 'Checkout found; build the Copter SITL target before reuse.')
                if compatible:break
    elif tool=='jmavsim':
        path=requested or root/'jMAVSim'
        jar=path/'out/production/jmavsim_run.jar'
        valid=False
        if jar.is_file():
            try:
                with zipfile.ZipFile(jar) as archive:valid='me/drton/jmavsim/Simulator.class' in archive.namelist()
            except zipfile.BadZipFile:pass
        result.update(found=jar.is_file(),compatible=valid,path=str(path),executable=str(jar),version=base.output(['git','-C',str(path),'rev-parse','HEAD']),reason='Built distributable; the Windows setup stage installs/verifies its native Java launch.' if valid else 'No built jMAVSim distribution.')
    elif tool=='ros-workspace':
        path=requested or root/'px4_ros_ws'
        setup=path/'install/setup.bash'
        compatible=setup.is_file() and base.output(['git','-C',str(path/'src/px4_msgs'),'describe','--exact-match','--tags','HEAD'])=='v1.17.0'
        result.update(found=setup.is_file(),compatible=compatible,path=str(path),executable=str(setup),version='px4_msgs v1.17.0 / Jazzy',reason='Matching messages built; start the DDS Agent and verify live /fmu/out topics.' if compatible else 'Build a PX4 1.17 message workspace in Setup.')
    elif tool=='onnx':
        path=requested or root/'onnx'
        python=path/'bin/python'
        version=base.output([str(python),'-c','import onnxruntime,numpy;print(onnxruntime.__version__)']) if python.is_file() else ''
        compatible=version=='1.23.2'
        result.update(found=python.is_file(),compatible=compatible,path=str(path),executable=str(python),version=version,reason='Isolated CPU runtime verified' if compatible else 'Install the selected isolated CPU environment.')
    result['managed']=bool(result['path']) and str(result['path']).startswith(str(root)+'/')
    return result

def dependencies(base,tool):
    base.require_noble()
    if os.geteuid()!=0:raise RuntimeError('Shared prerequisites require the selected Linux root account.')
    packages=['ca-certificates','git','python3','python3-venv','python3-pip']
    if tool=='gazebo':
        packages+=['curl','lsb-release','gnupg']
        base.run(['apt-get','update']);base.run(['apt-get','install','-y','--no-install-recommends',*packages])
        key=Path('/usr/share/keyrings/pkgs-osrf-archive-keyring.gpg')
        if not key.exists():
            with urllib.request.urlopen('https://packages.osrfoundation.org/gazebo.gpg',timeout=45) as response:key.write_bytes(response.read())
        source=Path('/etc/apt/sources.list.d/drone-deck-gazebo.list')
        architecture=base.output(['dpkg','--print-architecture'])
        source.write_text(f'deb [arch={architecture} signed-by={key}] https://packages.osrfoundation.org/gazebo/ubuntu-stable noble main\n')
        base.run(['apt-get','update']);base.run(['apt-get','install','-y','--no-install-recommends','gz-harmonic'])
        return
    if tool=='jmavsim':packages+=['ant','openjdk-17-jdk','zip']
    if tool=='ardupilot':packages+=['build-essential','ccache','gawk','pkg-config','python3-dev','libexpat1-dev','libsqlite3-dev','zlib1g-dev']
    if tool=='ros-workspace':
        if not Path('/opt/ros/jazzy/setup.bash').is_file():raise RuntimeError('Install ROS 2 Jazzy in Setup before its message workspace.')
        packages+=['build-essential','cmake','python3-colcon-common-extensions','python3-rosdep','ros-jazzy-rosidl-default-generators']
    env={**os.environ,'DEBIAN_FRONTEND':'noninteractive'}
    base.run(['apt-get','update'],env=env);base.run(['apt-get','install','-y','--no-install-recommends',*packages],env=env)

def clone(base,path,url,revision):
    if not (path/'.git').exists():base.run(['git','clone','--recursive','--branch',revision,url,str(path)])
    if base.output(['git','-C',str(path),'describe','--exact-match','--tags','HEAD'])!=revision:raise RuntimeError('Managed checkout revision changed. Review it manually; no reset was performed.')
    base.run(['git','-C',str(path),'submodule','update','--init','--recursive'])

def install(base,tool):
    base.require_noble()
    if os.geteuid()==0:raise RuntimeError('Build user tools as a regular Linux user.')
    if tool=='gazebo':return
    if tool=='onnx':
        path=base.owned_target('onnx')
        if not (path/'bin/python').is_file():base.run(['python3','-m','venv',str(path)])
        base.run([str(path/'bin/python'),'-m','pip','install','onnxruntime==1.23.2','numpy==2.2.6'])
        (path/'environment.txt').write_text(base.output([str(path/'bin/python'),'-m','pip','freeze']))
    elif tool=='ardupilot':
        path=base.owned_target('ardupilot');clone(base,path,'https://github.com/ArduPilot/ardupilot.git','Copter-4.6.3')
        environment=base.owned_target('ardupilot-python')
        if not (environment/'bin/python').is_file():base.run(['python3','-m','venv',str(environment)])
        base.run([str(environment/'bin/python'),'-m','pip','install','empy==3.3.4','pexpect==4.9.0','future==1.0.0','lxml==5.4.0','pymavlink==2.4.49'])
        env={**os.environ,'PATH':str(environment/'bin')+':'+os.environ['PATH']}
        base.run([str(environment/'bin/python'),'waf','configure','--board','sitl'],cwd=path,env=env)
        base.run([str(environment/'bin/python'),'waf','copter','-j2'],cwd=path,env=env)
        (environment/'environment.txt').write_text(base.output([str(environment/'bin/python'),'-m','pip','freeze']))
    elif tool=='ros-workspace':
        path=base.owned_target('px4_ros_ws');(path/'src').mkdir(parents=True,exist_ok=True)
        clone(base,path/'src/px4_msgs','https://github.com/PX4/px4_msgs.git','v1.17.0')
        base.run(['bash','-c','source /opt/ros/jazzy/setup.bash && colcon build --packages-select px4_msgs --executor sequential'],cwd=path)
        (path/'drone-deck-workspace.json').write_text(json.dumps(dict(firmware='v1.17.0',messages='v1.17.0',ros='jazzy')))
    elif tool=='jmavsim':
        parent=base.probe('px4')
        if not parent['found']:raise RuntimeError('Install/reuse PX4 1.17 before building its visualizer.')
        source=Path(parent['path'])
        revision=base.output(['git','-C',str(source),'rev-parse','v1.17.0:Tools/simulation/jmavsim/jMAVSim'])
        if len(revision)!=40:raise RuntimeError('Could not read the pinned PX4 1.17 visualizer revision.')
        path=base.owned_target('jMAVSim')
        if not (path/'.git').exists():
            base.run(['git','clone','https://github.com/PX4/jMAVSim.git',str(path)])
            base.run(['git','-C',str(path),'checkout','--detach',revision])
        if base.output(['git','-C',str(path),'rev-parse','HEAD'])!=revision:raise RuntimeError('Managed visualizer revision changed; review it manually.')
        base.run(['git','-C',str(path),'submodule','update','--init','--recursive'])
        base.run(['ant','distro'],cwd=path)
