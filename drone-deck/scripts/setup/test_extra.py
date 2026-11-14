"""Exercise reviewed recipe planning without apt, pip, git, network or installs."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch
import extra_tools
spec=importlib.util.spec_from_file_location('linux_tools',Path(__file__).with_name('linux-tools.py'))
tools=importlib.util.module_from_spec(spec);spec.loader.exec_module(tools)

class ExtraTests(unittest.TestCase):
 def test_cpu_install_uses_owned_environment_and_pinned_wheels(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   path=Path(directory);base=Mock();base.owned_target.return_value=path;base.output.return_value='fixture==1'
   with patch.object(extra_tools.os,'geteuid',return_value=1000,create=True):extra_tools.install(base,'onnx')
   calls=[call.args[0] for call in base.run.call_args_list]
   self.assertIn([str(path/'bin/python'),'-m','pip','install','onnxruntime==1.23.2','numpy==2.2.6'],calls)
   base.owned_target.assert_called_once_with('onnx');self.assertTrue((path/'environment.txt').is_file())
 def test_addon_verification_uses_the_managed_location(self):
  for tool,folder in [('onnx','onnx'),('ros-workspace','px4_ros_ws'),('ardupilot','ardupilot')]:
   with patch.object(extra_tools,'probe',return_value={}) as probe:tools.verify(tool)
   self.assertEqual(probe.call_args.args[1:],(tool,str(tools.ROOT/folder)))
 def test_custom_gazebo_probe_uses_the_requested_executable(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   path=Path(directory)/'custom gz';path.touch();base=Mock(ROOT=Path(directory));base.output.return_value='8.9.0'
   result=extra_tools.probe(base,'gazebo',str(path));self.assertTrue(result['compatible'])
   base.output.assert_called_once_with([str(path),'sim','--versions'])
 def test_changed_managed_checkout_is_not_reset(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   path=Path(directory);(path/'.git').mkdir();base=Mock();base.output.return_value='different-tag'
   with self.assertRaisesRegex(RuntimeError,'changed'):extra_tools.clone(base,path,'https://example.invalid/repo','pinned-tag')
   base.run.assert_not_called()
 def test_ros_package_probe_reads_the_installed_manifest(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   share=Path(directory);base=Mock();base.output.return_value='2.1.0-1noble'
   self.assertFalse(extra_tools.ros_package(base,'ros-gz',share)['found'])
   (share/'ros_gz_bridge').mkdir();(share/'ros_gz_bridge'/'package.xml').write_text('<package/>')
   found=extra_tools.ros_package(base,'ros-gz',share)
   self.assertTrue(found['compatible']);self.assertEqual(found['version'],'2.1.0-1noble')
   base.output.assert_called_with(['dpkg-query','-W','-f=${Version}','ros-jazzy-ros-gz'])
 def test_ros_package_install_needs_ros_and_installs_only_its_package(self):
  base=Mock()
  with patch.object(extra_tools.os,'geteuid',return_value=0,create=True):
   with patch.object(extra_tools.Path,'is_file',return_value=False):
    with self.assertRaisesRegex(RuntimeError,'ROS 2 Jazzy'):extra_tools.dependencies(base,'foxglove-bridge')
   base.run.assert_not_called()
   with patch.object(extra_tools.Path,'is_file',return_value=True):extra_tools.dependencies(base,'foxglove-bridge')
  calls=[call.args[0] for call in base.run.call_args_list]
  self.assertEqual(calls,[['apt-get','update'],['apt-get','install','-y','--no-install-recommends','ros-jazzy-foxglove-bridge']])
  with patch.object(extra_tools.os,'geteuid',return_value=1000,create=True):extra_tools.install(base,'ros-gz')
  self.assertEqual(len(base.run.call_args_list),2,'no user-level stage for apt packages')
 def test_jmavsim_probe_needs_the_built_jar_and_java(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   px4=Path(directory)/'PX4-Autopilot';project=px4/extra_tools.JMAVSIM;project.mkdir(parents=True)
   base=Mock(ROOT=Path(directory));base.output.return_value='v1.17.0'
   self.assertFalse(extra_tools.probe(base,'jmavsim',str(px4))['found'],'no Ant project yet')
   (project/'build.xml').write_text('<project/>')
   result=extra_tools.probe(base,'jmavsim',str(px4))
   self.assertTrue(result['found']);self.assertFalse(result['compatible']);self.assertIn('build its jar',result['reason'])
   jar=px4/extra_tools.JMAVSIM_JAR;jar.parent.mkdir(parents=True);jar.write_bytes(b'PK')
   with patch.object(extra_tools.shutil,'which',return_value='/usr/bin/java'):
    result=extra_tools.probe(base,'jmavsim',str(px4))
   self.assertTrue(result['compatible']);self.assertEqual(result['executable'],str(jar))
   self.assertTrue(result['managed'],'inside the managed root')
 def test_jmavsim_builds_only_in_the_managed_px4_checkout(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   root=Path(directory);base=Mock(ROOT=root)
   with patch.object(extra_tools.os,'geteuid',return_value=1000,create=True):
    with self.assertRaisesRegex(RuntimeError,"own PX4 checkout"):extra_tools.install(base,'jmavsim')
    base.run.assert_not_called()
    (root/'PX4-Autopilot'/'.git').mkdir(parents=True);(root/'.owned-PX4-Autopilot').write_text('x')
    extra_tools.install(base,'jmavsim')
  calls=[call.args[0] for call in base.run.call_args_list]
  self.assertEqual(calls[1],['ant','create_run_jar','copy_res'])
  self.assertEqual(base.run.call_args_list[1].kwargs['cwd'],root/'PX4-Autopilot'/extra_tools.JMAVSIM)
 def test_jmavsim_is_a_known_tool_verified_in_the_px4_checkout(self):
  self.assertIn('jmavsim',tools.TOOLS)
  with patch.object(extra_tools,'probe',return_value={}) as probe:tools.verify('jmavsim')
  self.assertEqual(probe.call_args.args[1:],('jmavsim',str(tools.ROOT/'PX4-Autopilot')))
 def test_flightgear_is_an_apt_package_with_no_user_stage(self):
  base=Mock()
  with patch.object(extra_tools.os,'geteuid',return_value=0,create=True):extra_tools.dependencies(base,'flightgear')
  calls=[call.args[0] for call in base.run.call_args_list]
  self.assertEqual(calls[-1],['apt-get','install','-y','--no-install-recommends','flightgear'])
  with patch.object(extra_tools.os,'geteuid',return_value=1000,create=True):extra_tools.install(base,'flightgear')
  self.assertEqual(len(base.run.call_args_list),2,'nothing to build')
 def test_flightgear_probe_reads_the_version_of_the_chosen_fgfs(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   fgfs=Path(directory)/'fgfs';fgfs.touch();base=Mock(ROOT=Path(directory));base.output.return_value='2020.3.19\nmore'
   result=extra_tools.probe(base,'flightgear',str(fgfs))
   self.assertTrue(result['compatible']);self.assertEqual(result['version'],'2020.3.19')
   self.assertFalse(extra_tools.probe(base,'flightgear',str(Path(directory)/'missing'))['found'])
 def test_ardupilot_dds_probe_needs_the_dds_build_and_the_messages(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   root=Path(directory);checkout=root/'ardupilot';base=Mock(ROOT=root);base.output.return_value='Copter-4.6.3'
   (checkout/'Tools/autotest').mkdir(parents=True);(checkout/'Tools/autotest/sim_vehicle.py').touch()
   binary=checkout/'build/sitl/bin/arducopter';binary.parent.mkdir(parents=True);binary.write_bytes(b'\0plain copter\0')
   result=extra_tools.probe(base,'ardupilot-dds',str(checkout))
   self.assertFalse(result['found']);self.assertIn('--enable-DDS',result['reason'])
   binary.write_bytes(b'\0'+extra_tools.AP_DDS_MARK+b'\0')
   result=extra_tools.probe(base,'ardupilot-dds',str(checkout))
   self.assertTrue(result['found']);self.assertFalse(result['compatible']);self.assertIn('ardupilot_msgs',result['reason'])
   (root/'ap_ros_ws/install/ardupilot_msgs').mkdir(parents=True);(root/'ap_ros_ws/install/setup.bash').touch()
   result=extra_tools.probe(base,'ardupilot-dds',str(checkout))
   self.assertTrue(result['compatible']);self.assertEqual(result['executable'],str(root/'ap_ros_ws/install/setup.bash'))
 def test_ardupilot_dds_builds_only_in_the_managed_checkout_with_the_generator_on_path(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   root=Path(directory);base=Mock(ROOT=root)
   base.owned_target.side_effect=lambda name:root/name
   base.output.side_effect=lambda args:extra_tools.AP_DDS_GEN[1] if 'describe' in args and '--exact-match' in args else 'Copter-4.6.3'
   with patch.object(extra_tools.os,'geteuid',return_value=1000,create=True):
    with self.assertRaisesRegex(RuntimeError,"own ArduPilot checkout"):extra_tools.install(base,'ardupilot-dds')
    base.run.assert_not_called()
    checkout=root/'ardupilot';(checkout/'.git').mkdir(parents=True);(root/'.owned-ardupilot').write_text('x')
    (checkout/extra_tools.AP_MSGS).mkdir(parents=True);(checkout/extra_tools.AP_MSGS/'package.xml').write_text('<package/>')
    with patch.dict(extra_tools.os.environ,{'PATH':'/usr/bin'}),patch.object(extra_tools.Path,'symlink_to') as link:
     extra_tools.install(base,'ardupilot-dds')
    link.assert_called_once_with(checkout/extra_tools.AP_MSGS)
  calls=[call.args[0] for call in base.run.call_args_list]
  self.assertEqual(calls[0][:5],['git','clone','--recurse-submodules','--branch','v4.5.1'])
  self.assertEqual(calls[1],['./gradlew','assemble'])
  configure=base.run.call_args_list[2]
  self.assertEqual(configure.args[0][1:],['waf','configure','--board','sitl','--enable-DDS'])
  self.assertTrue(configure.kwargs['env']['PATH'].endswith(str(root/'Micro-XRCE-DDS-Gen'/'scripts')))
  self.assertIn('colcon build --packages-select ardupilot_msgs',calls[-1][-1])
 def test_ardupilot_dds_is_a_known_tool_verified_in_the_ardupilot_checkout(self):
  self.assertIn('ardupilot-dds',tools.TOOLS)
  with patch.object(extra_tools,'probe',return_value={}) as probe:tools.verify('ardupilot-dds')
  self.assertEqual(probe.call_args.args[1:],('ardupilot-dds',str(tools.ROOT/'ardupilot')))
  base=Mock()
  with patch.object(extra_tools.os,'geteuid',return_value=0,create=True),patch.object(extra_tools.Path,'is_file',return_value=True):
   extra_tools.dependencies(base,'ardupilot-dds')
  self.assertIn('default-jdk-headless',base.run.call_args_list[-1].args[0])
if __name__=='__main__':unittest.main()
