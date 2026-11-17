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
    self.assertFalse(result['compatible']);self.assertIn('JOGL 2.4',result['reason'])
    with patch.object(extra_tools,'jogl_ready',return_value=True):result=extra_tools.probe(base,'jmavsim',str(px4))
   self.assertTrue(result['compatible']);self.assertEqual(result['executable'],str(jar))
   self.assertTrue(result['managed'],'inside the managed root')
 def test_jmavsim_builds_only_in_the_managed_px4_checkout(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   root=Path(directory);base=Mock(ROOT=root)
   with patch.object(extra_tools.os,'geteuid',return_value=1000,create=True):
    with self.assertRaisesRegex(RuntimeError,"own PX4 checkout"):extra_tools.install(base,'jmavsim')
    base.run.assert_not_called()
    (root/'PX4-Autopilot'/'.git').mkdir(parents=True);(root/'.owned-PX4-Autopilot').write_text('x')
    with patch.object(extra_tools,'update_jogl') as jogl:extra_tools.install(base,'jmavsim')
    jogl.assert_called_once_with(root/'PX4-Autopilot'/extra_tools.JMAVSIM)
  calls=[call.args[0] for call in base.run.call_args_list]
  self.assertEqual(calls[1],['ant','create_run_jar','copy_res'])
  self.assertEqual(base.run.call_args_list[1].kwargs['cwd'],root/'PX4-Autopilot'/extra_tools.JMAVSIM)
  self.assertEqual(len(calls),2,'PX4 was never built, so it has nothing to look for again')
 def test_a_simulator_installed_after_px4_was_built_makes_px4_look_for_it_again(self):
  # PX4 cached "Java not found" when it was built before jMAVSim; without this, `make px4_sitl jmavsim` has no target.
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   root=Path(directory);build=root/'PX4-Autopilot/build/px4_sitl_default'
   (root/'PX4-Autopilot'/'.git').mkdir(parents=True);build.mkdir(parents=True);(build/'CMakeCache.txt').write_text('x')
   with patch.object(extra_tools.os,'geteuid',return_value=1000,create=True):
    for tool in ('gazebo','flightgear','jmavsim'):
     base=Mock(ROOT=root)
     if tool=='jmavsim':(root/'.owned-PX4-Autopilot').write_text('x')
     with patch.object(extra_tools,'update_jogl'):extra_tools.install(base,tool)
     refresh=[c for c in base.run.call_args_list if c.args[0][0]=='cmake']
     if tool=='jmavsim':
      self.assertEqual(len(refresh),1,tool)
      self.assertEqual(refresh[0].args[0],['cmake','-U','JAVA_ANT_PATH','-U','Java_*','-U','FGFS_PATH','-U','gz-*','-U','gz*_DIR','-U','GSTREAMER*','-U','__pkg_config_checked_GSTREAMER*','.'])
      self.assertEqual(refresh[0].kwargs['cwd'],build)
      self.assertTrue(refresh[0].kwargs['env']['PATH'].startswith(str(root/'px4-python/bin')))
     else:
      self.assertEqual(refresh,[],f'{tool}: an adopted PX4 build is never reconfigured')
   with patch.object(extra_tools.os,'geteuid',return_value=1000,create=True):
    base=Mock(ROOT=root);extra_tools.install(base,'gazebo')
   self.assertEqual(base.run.call_args_list[0].args[0][0],'cmake','Gazebo installed after PX4 refreshes the owned build')
 def test_px4_camera_video_is_patched_to_420_once_and_only_in_a_v117_shaped_source(self):
  # QGroundControl shows "Waiting for video" for the High 4:4:4 stream PX4 v1.17 sends without this.
  chain='_source, queue1, videoRate, capsFilter, converter, queue2, encoder, payloader'
  original=('\tgst_caps_unref(rateCaps);\n\n'
            f'\tgst_bin_add_many(GST_BIN(_pipeline), {chain},\n\t\t\t sink, NULL);\n'
            f'\tif (!gst_element_link_many({chain}, sink,\n')
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   px4=Path(directory);source=px4/extra_tools.GST_CAMERA;source.parent.mkdir(parents=True)
   self.assertFalse(extra_tools.patch_gst_camera(px4),'no plugin source: nothing to do')
   source.write_text(original)
   self.assertTrue(extra_tools.patch_gst_camera(px4))
   patched=source.read_text()
   self.assertIn('"format", G_TYPE_STRING, "I420"',patched)
   self.assertEqual(patched.count('converter, formatFilter, queue2'),2,'added to the bin and linked')
   self.assertFalse(extra_tools.patch_gst_camera(px4),'already patched');self.assertEqual(source.read_text(),patched)
   source.write_text('different upstream source\n')
   self.assertFalse(extra_tools.patch_gst_camera(px4));self.assertEqual(source.read_text(),'different upstream source\n')
 def test_gazebo_dependencies_include_gstreamer_for_px4_camera_video(self):
  self.assertIn('libgstreamer1.0-dev',extra_tools.GSTREAMER);self.assertIn('gstreamer1.0-plugins-ugly',extra_tools.GSTREAMER)
 def test_a_failed_addon_download_leaves_no_half_clone(self):
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   path=Path(directory)/'ardupilot';base=Mock()
   def die(args,**kwargs):
    path.mkdir();raise extra_tools.subprocess.CalledProcessError(128,args)
   base.run.side_effect=die
   with self.assertRaises(extra_tools.subprocess.CalledProcessError):extra_tools.clone(base,path,'https://example.invalid/repo','tag')
   self.assertFalse(path.exists())
   self.assertEqual(base.run.call_args.args[0][:6],[*extra_tools.GIT,'clone'])
 def test_jogl_is_replaced_only_when_every_pinned_jar_matches(self):
  # JOGL 2.4's window crashes under WSLg; 2.5.0 is fetched and checked against pinned SHA-256s.
  with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
   project=Path(directory);lib=project/'lib';lib.mkdir()
   for name in extra_tools.JOGL:(lib/name).write_bytes(b'jogl 2.4')
   good={name:name.encode() for name in extra_tools.JOGL}
   pins={name:extra_tools.hashlib.sha256(data).hexdigest() for name,data in good.items()}
   def serve(data):
    def urlopen(url,timeout):
     response=Mock();response.read.return_value=data[url.rsplit('/',1)[1]]
     response.__enter__=Mock(return_value=response);response.__exit__=Mock(return_value=False)
     return response
    return urlopen
   with patch.dict(extra_tools.JOGL,pins):
    self.assertFalse(extra_tools.jogl_ready(project))
    bad=dict(good,**{'gluegen-rt.jar':b'tampered'})
    with patch.object(extra_tools.urllib.request,'urlopen',side_effect=serve(bad)):
     with self.assertRaisesRegex(RuntimeError,'SHA-256'):extra_tools.update_jogl(project)
    self.assertEqual((lib/'jogl-all.jar').read_bytes(),b'jogl 2.4','nothing replaced after a mismatch')
    self.assertEqual(sorted(p.name for p in lib.iterdir()),sorted(extra_tools.JOGL),'no .part files left')
    with patch.object(extra_tools.urllib.request,'urlopen',side_effect=serve(good)) as fetch:extra_tools.update_jogl(project)
    self.assertTrue(extra_tools.jogl_ready(project))
    self.assertTrue(all(c.args[0].startswith('https://jogamp.org/deployment/v2.5.0/jar/') for c in fetch.call_args_list))
    with patch.object(extra_tools.urllib.request,'urlopen') as again:extra_tools.update_jogl(project)
    again.assert_not_called()
 def test_jmavsim_is_a_known_tool_verified_in_the_px4_checkout(self):
  self.assertIn('jmavsim',tools.TOOLS)
  with patch.object(extra_tools,'probe',return_value={}) as probe:tools.verify('jmavsim')
  self.assertEqual(probe.call_args.args[1:],('jmavsim',str(tools.ROOT/'PX4-Autopilot')))
 def test_flightgear_is_an_apt_package_with_no_user_stage(self):
  base=Mock(ROOT=Path('/nonexistent'))
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
  self.assertEqual(calls[0][:9],[*extra_tools.GIT,'clone','--recurse-submodules','--branch','v4.5.1'])
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
