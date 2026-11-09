"""Regression tests with disposable files; never install or launch a flight tool."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('discovery',Path(__file__).with_name('linux-tools.py'))
discovery=importlib.util.module_from_spec(spec)
spec.loader.exec_module(discovery)

class DiscoveryTests(unittest.TestCase):
    def setUp(self):
        self.parent=Path(__file__).resolve().parent
        self.temporary=tempfile.TemporaryDirectory(prefix='discovery-fixture-',dir=self.parent)
        self.base=Path(self.temporary.name).resolve()
        assert self.base.is_relative_to(self.parent)
        self.addCleanup(self.temporary.cleanup)
        self.home=self.base/'home'
        self.managed=self.home/'.local/share/drone-deck/tools'
        self.root_patch=patch.object(discovery,'ROOT',self.managed)
        self.root_patch.start();self.addCleanup(self.root_patch.stop)
    def file(self,path):
        path.parent.mkdir(parents=True,exist_ok=True);path.write_text('fixture')
        return path
    def px4(self,path,built=True):
        (path/'.git').mkdir(parents=True)
        if built:self.file(path/'build/px4_sitl_default/bin/px4')
        return path
    def test_unbuilt_checkout_does_not_hide_built_managed_checkout(self):
        self.px4(self.home/'PX4-Autopilot',False)
        working=self.px4(self.managed/'PX4-Autopilot')
        with patch.object(Path,'home',return_value=self.home),patch.object(discovery,'output',return_value='v1.17.0'),patch.object(discovery.os,'access',return_value=True):
            candidate=discovery.probe('px4')
        self.assertTrue(candidate['compatible']);self.assertEqual(candidate['path'],str(working))
    def test_stale_sih_directory_does_not_hide_default_binary(self):
        working=self.px4(self.managed/'PX4-Autopilot')
        (working/'build/px4_sitl_sih').mkdir()
        with patch.object(discovery,'output',return_value='v1.17.0'),patch.object(discovery.os,'access',return_value=True):
            candidate=discovery.probe('px4',str(working))
        self.assertTrue(candidate['compatible']);self.assertEqual(candidate['build_target'],'px4_sitl')
    def test_incompatible_mavsdk_does_not_hide_project_environment(self):
        old=self.file(self.managed/'mavsdk/bin/python')
        working=self.file(self.base/'project/bin/python')
        with patch.object(discovery,'output',side_effect=lambda args:'3.10.2' if args[0]==str(old) else '4.1.0'):
            candidate=discovery.probe('mavsdk',windows_candidate=str(working.parent.parent))
        self.assertTrue(candidate['compatible']);self.assertEqual(candidate['executable'],str(working))
    def test_gazebo_custom_path_is_used(self):
        executable=self.file(self.base/'custom/gz')
        with patch.object(discovery.shutil,'which',return_value=None),patch.object(discovery,'output',return_value='8.0.0'):
            candidate=discovery.probe('gazebo',str(executable))
        self.assertTrue(candidate['found']);self.assertEqual(candidate['executable'],str(executable))
    def test_px4_retry_recovers_submodules_before_build(self):
        calls=[]
        self.px4(self.managed/'PX4-Autopilot',False)
        self.file(self.managed/'.owned-PX4-Autopilot')
        self.file(self.managed/'.owned-px4-python')
        self.file(self.managed/'px4-python/bin/python')
        with patch.object(discovery,'require_noble'),patch.object(discovery.os,'geteuid',create=True,return_value=1000),patch.object(discovery,'run',side_effect=lambda args,**kwargs:calls.append(args)),patch.object(discovery,'output',return_value='v1.17.0'):
            discovery.install('px4')
        update=next(i for i,c in enumerate(calls) if 'submodule' in c)
        build=next(i for i,c in enumerate(calls) if c[0]=='make')
        self.assertLess(update,build)
        self.assertFalse(any('clone' in c for c in calls))

if __name__=='__main__':unittest.main()
