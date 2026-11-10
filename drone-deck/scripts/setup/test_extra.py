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
if __name__=='__main__':unittest.main()
