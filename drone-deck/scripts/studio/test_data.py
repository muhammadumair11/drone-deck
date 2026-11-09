"""Temporary local data only; no model/runtime installation or vehicle traffic."""
import importlib.util
import ast
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
spec=importlib.util.spec_from_file_location('studio_data',Path(__file__).with_name('data.py'))
data=importlib.util.module_from_spec(spec);spec.loader.exec_module(data)
class DataTests(unittest.TestCase):
 def setUp(self):
  self.parent=Path(__file__).resolve().parent
  self.temporary=tempfile.TemporaryDirectory(prefix='data-fixture-',dir=self.parent);self.root=Path(self.temporary.name).resolve()
  assert self.root.is_relative_to(self.parent);self.addCleanup(self.temporary.cleanup)
  self.patcher=patch.object(data,'ROOT',self.root);self.patcher.start();self.addCleanup(self.patcher.stop)
  self.quiet=patch.object(data,'emit');self.quiet.start();self.addCleanup(self.quiet.stop)
  self.session='20261109T000000Z_flight_12345678';self.bag=self.root/'recordings'/self.session/'bag';self.bag.mkdir(parents=True)
  (self.bag.parent/'session.json').write_text(json.dumps(dict(status='Finalized',source='Simulation',clock='system time')))
  self.artifact=self.bag/'fixture.mcap';self.artifact.write_bytes(b'fixture bytes for provenance hashing')
 def dataset(self):return data.dataset_create(dict(name='dataset',session=self.session))['dataset']
 def test_version_is_repeatable_and_source_is_preserved(self):
  one=self.dataset();two=self.dataset();self.assertEqual(one['version'],two['version']);self.assertTrue(self.artifact.is_file())
  self.assertTrue(data.dataset_verify(one)['ready']);self.artifact.write_bytes(b'changed source');self.assertFalse(data.dataset_verify(one)['ready'])
 def test_evaluation_links_predictions_and_dataset_hash(self):
  dataset=self.dataset();csv=self.root/'predictions.csv';csv.write_text('sample_id,target,prediction,latency_ms\na,cat,cat,10\nb,dog,cat,20\n')
  report=data.evaluate(dict(name='dataset',version=dataset['version'],csv=str(csv),task='classification'))['report']
  self.assertEqual(report['metrics']['accuracy'],.5);self.assertEqual(report['metrics']['latency_mean_ms'],15)
  self.assertEqual(report['version'],dataset['version']);self.assertEqual(len(report['predictions_sha256']),64)
 def test_changed_dataset_manifest_is_not_promoted_to_verified(self):
  dataset=self.dataset();path=Path(dataset['manifest']);content=json.loads(path.read_text());content['notes']='changed';path.write_text(json.dumps(content))
  with self.assertRaisesRegex(ValueError,'manifest changed'):data.dataset_verify(dataset)
 def test_matching_inference_manifest_binds_model_to_the_evaluation(self):
  dataset=self.dataset();csv=self.root/'predictions.csv';csv.write_text('sample_id,target,prediction\na,1,1\n')
  run=dict(predictions_sha256=data.digest(csv),model_sha256='f'*64,runtime='fixture');(self.root/'run.json').write_text(json.dumps(run))
  report=data.evaluate(dict(name='dataset',version=dataset['version'],csv=str(csv)))['report'];self.assertEqual(report['inference']['model_sha256'],'f'*64)
  run['predictions_sha256']='mismatch';(self.root/'run.json').write_text(json.dumps(run))
  report=data.evaluate(dict(name='dataset',version=dataset['version'],csv=str(csv)))['report'];self.assertNotIn('inference',report)
 def test_regression_rejects_nonfinite_and_duplicate_samples(self):
  dataset=self.dataset();csv=self.root/'predictions.csv'
  for content in ['sample_id,target,prediction\na,1,nan\n','sample_id,target,prediction\na,1,2\na,3,4\n']:
   csv.write_text(content)
   with self.assertRaises(ValueError):data.evaluate(dict(name='dataset',version=dataset['version'],csv=str(csv),task='regression'))
 def test_path_traversal_and_active_capture_are_rejected(self):
  with self.assertRaises(ValueError):data.dataset_create(dict(name='../outside',session=self.session))
  (self.bag.parent/'session.json').write_text(json.dumps(dict(status='recording')))
  with self.assertRaises(ValueError):self.dataset()
 def test_project_scaffolding_does_not_overwrite_or_install(self):
  result=data.project_create(dict(name='monitor',kind='ros2'))['project']
  self.assertTrue((Path(result['path'])/'main.py').is_file());self.assertFalse((Path(result['path'])/'.venv').exists())
  with self.assertRaises(ValueError):data.project_create(dict(name='monitor',kind='ros2'))
 def test_inference_and_monitor_templates_are_editable_without_runtime_imports(self):
  for kind in ('onnx','mavsdk'):
   result=data.project_create(dict(name=kind,kind=kind))['project'];path=Path(result['path'])
   ast.parse((path/'main.py').read_text());self.assertFalse((path/'.venv').exists())
  self.assertIn('NPZ named tensors',(self.root/'projects/onnx/README.md').read_text())
  with self.assertRaises(ValueError):data.project_create(dict(name='trainer',kind='training'))
if __name__=='__main__':unittest.main()
