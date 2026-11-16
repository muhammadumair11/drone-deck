"""Explicit add-on recipes. No command in this module runs at import time."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import urllib.request

TOOLS={'gazebo','ardupilot','ros-workspace','onnx','ros-gz','foxglove-bridge','jmavsim','flightgear','ardupilot-dds'}
# jMAVSim inside a PX4 checkout: its Ant project and the jar `ant create_run_jar` builds.
JMAVSIM=Path('Tools/simulation/jmavsim/jMAVSim')
JMAVSIM_JAR=JMAVSIM/'out/production/jmavsim_run.jar'
# jMAVSim ships JOGL 2.4, whose 3D window crashes in Mesa's GLX on Ubuntu 24.04 under WSLg
# (glXQueryDrawable while setting the swap interval) and takes the simulation with it. JOGL 2.5.0
# runs. Its four Linux jars replace the old ones in jMAVSim's lib/, where `ant create_run_jar`
# (run again at every `make px4_sitl jmavsim`) packs them.
JOGL_URL='https://jogamp.org/deployment/v2.5.0/jar/'
JOGL={'jogl-all.jar':'245717cceabca264a210a899f8839d47bd127f50f80892ead2277dd89cbcd301',
      'gluegen-rt.jar':'3620c18536a8671fcb1c595d7448e9d31226b824117af6a4c6d45c657f4dabe3',
      'jogl-all-natives-linux-amd64.jar':'e97850f290d8e44ba07fa0500d7a071ff444209099f0372df3dba707cba3ddc1',
      'gluegen-rt-natives-linux-amd64.jar':'6d998d0c1f04f103894b769049086124505063cea86a82896194bb53c88b040a'}
def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
def jogl_ready(project):
    return all((project/'lib'/name).is_file() and sha256(project/'lib'/name)==digest for name,digest in JOGL.items())
def update_jogl(project):
    """Puts the pinned JOGL 2.5.0 jars in jMAVSim's lib/; nothing is replaced unless all four match."""
    lib=project/'lib';fetched=[]
    try:
        for name,digest in JOGL.items():
            if (lib/name).is_file() and sha256(lib/name)==digest:continue
            part=lib/(name+'.part')
            with urllib.request.urlopen(JOGL_URL+name,timeout=120) as response:part.write_bytes(response.read())
            fetched.append((part,lib/name))
            if sha256(part)!=digest:raise RuntimeError(f'{name} from jogamp.org does not match its pinned SHA-256; jMAVSim was not changed.')
        for part,target in fetched:part.replace(target)
    finally:
        for part,_ in fetched:part.unlink(missing_ok=True)
# ArduPilot's DDS client: SITL configured with --enable-DDS, whose types Micro-XRCE-DDS-Gen generates
# (v4.5.1 for ArduPilot before 4.7), and ardupilot_msgs for ROS 2 (/ap/status, the arm, mode and
# takeoff services). A Copter binary built with DDS carries the names of its DDS services.
AP_DDS_GEN=('https://github.com/ardupilot/Micro-XRCE-DDS-Gen.git','v4.5.1')
AP_MSGS=Path('Tools/ros2/ardupilot_msgs')
AP_DDS_MARK=b'arm_motorsService'
def ardupilot_checkouts(base,requested):
    return [requested] if requested else [base.ROOT/'ardupilot',Path.home()/'ardupilot']
def has_dds(binary):
    try:
        return AP_DDS_MARK in binary.read_bytes()
    except OSError:
        return False
# ROS 2 Jazzy apt packages: recipe id -> (package, ROS package directory under /opt/ros/jazzy/share).
ROS_PACKAGES={'ros-gz':('ros-jazzy-ros-gz','ros_gz_bridge'),'foxglove-bridge':('ros-jazzy-foxglove-bridge','foxglove_bridge')}
def ros_package(base,tool,share=Path('/opt/ros/jazzy/share')):
    """Probe an apt-installed ROS 2 Jazzy package: its package.xml and dpkg version."""
    package,folder=ROS_PACKAGES[tool]
    manifest=share/folder/'package.xml'
    version=base.output(['dpkg-query','-W','-f=${Version}',package])
    found=manifest.is_file()
    return dict(found=found,compatible=found,path=str(share/folder),executable=str(manifest),version=version or ('installed' if found else ''),
                reason=(f'{package} installed; a running bridge is checked when its profile starts.' if found else f'Install {package} from the ROS 2 apt repository.'))
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
    elif tool=='ardupilot-dds':
        workspace=root/'ap_ros_ws'
        setup=workspace/'install/setup.bash'
        messages=(workspace/'install/ardupilot_msgs').is_dir()
        for path in ardupilot_checkouts(base,requested):
            if not (path/'Tools/autotest/sim_vehicle.py').is_file():continue
            built=has_dds(path/'build/sitl/bin/arducopter')
            compatible=built and messages and setup.is_file()
            reason=('Copter SITL has its DDS client and ardupilot_msgs is built; a running ArduPilot system with ROS 2 shows the /ap/ topics.' if compatible else
                    'Build ardupilot_msgs for ROS 2 (this recipe does).' if built else
                    "The Copter SITL build has no DDS client. Rebuild it with ./waf configure --board sitl --enable-DDS (this recipe does, in Drone Deck's own checkout).")
            result.update(found=built or messages,compatible=compatible,path=str(path),executable=str(setup),
                          version=base.output(['git','-C',str(path),'describe','--tags','--always']),reason=reason)
            if compatible:break
        if not result['path']:
            result['reason']='No ArduPilot checkout found. Install ArduPilot first, or choose your checkout.'
    elif tool in ROS_PACKAGES:
        result.update(ros_package(base,tool))
    elif tool=='flightgear':
        executable=str(requested) if requested else (shutil.which('fgfs') or ('/usr/games/fgfs' if Path('/usr/games/fgfs').is_file() else ''))
        if executable and Path(executable).is_file():
            version=base.output([executable,'--version']).splitlines()
            compatible=bool(version)
            result.update(found=True,compatible=compatible,path=executable,executable=executable,version=version[0] if version else '',
                          reason='FlightGear runs; PX4 builds its FlightGear bridge on the first launch.' if compatible else 'fgfs did not report a version; reinstall the flightgear package.')
        else:
            result['reason']="FlightGear not found. Install Ubuntu's flightgear package."
    elif tool=='jmavsim':
        for path in ([requested] if requested else [Path.home()/'PX4-Autopilot',root/'PX4-Autopilot']):
            project=path/JMAVSIM
            if not (project/'build.xml').is_file():continue
            jar=path/JMAVSIM_JAR
            java=shutil.which('java')
            window=jogl_ready(project)
            compatible=jar.is_file() and bool(java) and window
            reason=('jMAVSim jar and Java found; PX4 builds its SITL target on the first launch.' if compatible else
                    'Install a Java runtime (java not found).' if jar.is_file() and not java else
                    "jMAVSim still has JOGL 2.4, whose 3D window crashes under WSLg. Reinstall jMAVSim with Drone Deck to update it to JOGL 2.5." if jar.is_file() else
                    'jMAVSim source found; build its jar (ant create_run_jar copy_res) before reuse.')
            result.update(found=True,compatible=compatible,path=str(path),executable=str(jar),
                          version=base.output(['git','-C',str(project),'describe','--tags','--always']),reason=reason)
            if compatible:break
        if not result['found']:
            result['reason']='No PX4 checkout with the jMAVSim submodule found. Choose the PX4 folder, or install PX4 first.'
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
    if tool in ROS_PACKAGES:
        if not Path('/opt/ros/jazzy/setup.bash').is_file():raise RuntimeError('Install ROS 2 Jazzy in Setup first; it configures the ROS apt repository.')
        env={**os.environ,'DEBIAN_FRONTEND':'noninteractive'}
        base.run(['apt-get','update'],env=env);base.run(['apt-get','install','-y','--no-install-recommends',ROS_PACKAGES[tool][0]],env=env)
        return
    if tool=='ardupilot':packages+=['build-essential','ccache','gawk','pkg-config','python3-dev','libexpat1-dev','libsqlite3-dev','zlib1g-dev']
    if tool=='jmavsim':packages+=['openjdk-17-jdk','ant']
    if tool=='flightgear':
        env={**os.environ,'DEBIAN_FRONTEND':'noninteractive'}
        base.run(['apt-get','update'],env=env);base.run(['apt-get','install','-y','--no-install-recommends','flightgear'],env=env)
        return
    if tool in ('ros-workspace','ardupilot-dds'):
        if not Path('/opt/ros/jazzy/setup.bash').is_file():raise RuntimeError('Install ROS 2 Jazzy in Setup before its message workspace.')
        packages+=['build-essential','cmake','python3-colcon-common-extensions','python3-rosdep','ros-jazzy-rosidl-default-generators']
    if tool=='ardupilot-dds':packages+=['default-jdk-headless']
    env={**os.environ,'DEBIAN_FRONTEND':'noninteractive'}
    base.run(['apt-get','update'],env=env);base.run(['apt-get','install','-y','--no-install-recommends',*packages],env=env)

# A connection that dies (the laptop sleeps) would leave a plain git download waiting forever.
GIT=['git','-c','http.lowSpeedLimit=1000','-c','http.lowSpeedTime=120']
def fresh_clone(base,args,path):
    """Clones into an app-owned `path`; a failed download leaves no half clone for the retry to trip on."""
    try:
        base.run([*GIT,'clone',*args,str(path)])
    except subprocess.CalledProcessError:
        shutil.rmtree(path,ignore_errors=True)
        raise

def clone(base,path,url,revision):
    if not (path/'.git').exists():fresh_clone(base,['--recursive','--branch',revision,url],path)
    if base.output(['git','-C',str(path),'describe','--exact-match','--tags','HEAD'])!=revision:raise RuntimeError('Managed checkout revision changed. Review it manually; no reset was performed.')
    base.run([*GIT,'-C',str(path),'submodule','update','--init','--recursive'])

# PX4 looks for each simulator once, when its build is first configured, and remembers "not found":
# a simulator installed after PX4 was built (jMAVSim needs Java and ant) gets no make target until
# those answers are forgotten and the build is configured again.
PX4_FORGET=('JAVA_ANT_PATH','Java_*','FGFS_PATH','gz-*','gz*_DIR')
def refresh_px4(base):
    """Makes Drone Deck's own PX4 build look again for simulators installed after it was configured."""
    build=base.ROOT/'PX4-Autopilot/build/px4_sitl_default'
    if not (build/'CMakeCache.txt').is_file() or not (base.ROOT/'.owned-PX4-Autopilot').is_file():return
    forget=[arg for name in PX4_FORGET for arg in ('-U',name)]
    env={**os.environ,'PATH':str(base.ROOT/'px4-python/bin')+':'+os.environ['PATH']}
    base.run(['cmake',*forget,'.'],cwd=build,env=env)

def install(base,tool):
    base.require_noble()
    if os.geteuid()==0:raise RuntimeError('Build user tools as a regular Linux user.')
    if tool in ('gazebo','flightgear'):
        refresh_px4(base)
        return
    if tool in ROS_PACKAGES:return
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
    elif tool=='jmavsim':
        # Built only inside Drone Deck's own PX4 checkout: an adopted checkout is never changed.
        path=base.ROOT/'PX4-Autopilot'
        if not (path/'.git').exists() or not (base.ROOT/'.owned-PX4-Autopilot').is_file():
            raise RuntimeError("jMAVSim builds inside Drone Deck's own PX4 checkout. Install PX4 with Drone Deck first, "
                               'or build jMAVSim in your own checkout (make px4_sitl jmavsim) and choose that folder.')
        base.run([*GIT,'-C',str(path),'submodule','update','--init',str(JMAVSIM)])
        update_jogl(path/JMAVSIM)
        base.run(['ant','create_run_jar','copy_res'],cwd=path/JMAVSIM)
        refresh_px4(base)
    elif tool=='ardupilot-dds':
        # Built only inside Drone Deck's own ArduPilot checkout: an adopted checkout is never changed.
        path=base.ROOT/'ardupilot'
        if not (path/'.git').exists() or not (base.ROOT/'.owned-ardupilot').is_file():
            raise RuntimeError("ArduPilot's DDS client builds inside Drone Deck's own ArduPilot checkout. Install ArduPilot with Drone Deck first, "
                               'or build yours with ./waf configure --board sitl --enable-DDS and choose that folder.')
        if not (path/AP_MSGS/'package.xml').is_file():
            raise RuntimeError(f'This ArduPilot checkout has no {AP_MSGS}; it is older than ArduPilot 4.5.')
        gen=base.owned_target('Micro-XRCE-DDS-Gen');url,revision=AP_DDS_GEN
        if not (gen/'.git').exists():fresh_clone(base,['--recurse-submodules','--branch',revision,url],gen)
        if base.output(['git','-C',str(gen),'describe','--exact-match','--tags','HEAD'])!=revision:raise RuntimeError('Managed checkout revision changed. Review it manually; no reset was performed.')
        base.run(['./gradlew','assemble'],cwd=gen)
        environment=base.ROOT/'ardupilot-python'
        env={**os.environ,'PATH':str(environment/'bin')+':'+os.environ['PATH']+':'+str(gen/'scripts')}
        base.run([str(environment/'bin/python'),'waf','configure','--board','sitl','--enable-DDS'],cwd=path,env=env)
        base.run([str(environment/'bin/python'),'waf','copter','-j2'],cwd=path,env=env)
        workspace=base.owned_target('ap_ros_ws');(workspace/'src').mkdir(parents=True,exist_ok=True)
        link=workspace/'src/ardupilot_msgs'
        if not link.exists():link.symlink_to(path/AP_MSGS)
        base.run(['bash','-c','source /opt/ros/jazzy/setup.bash && colcon build --packages-select ardupilot_msgs --executor sequential'],cwd=workspace)
        (workspace/'drone-deck-workspace.json').write_text(json.dumps(dict(ardupilot=base.output(['git','-C',str(path),'describe','--tags','--always']),generator=revision,ros='jazzy')))
    elif tool=='ros-workspace':
        path=base.owned_target('px4_ros_ws');(path/'src').mkdir(parents=True,exist_ok=True)
        clone(base,path/'src/px4_msgs','https://github.com/PX4/px4_msgs.git','v1.17.0')
        base.run(['bash','-c','source /opt/ros/jazzy/setup.bash && colcon build --packages-select px4_msgs --executor sequential'],cwd=path)
        (path/'drone-deck-workspace.json').write_text(json.dumps(dict(firmware='v1.17.0',messages='v1.17.0',ros='jazzy')))

