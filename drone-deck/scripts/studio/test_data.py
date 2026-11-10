"""Temporary local data only; no model/runtime installation or vehicle traffic."""
import ast
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('studio_data', Path(__file__).with_name('data.py'))
data = importlib.util.module_from_spec(spec)
spec.loader.exec_module(data)


class DataTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='data-fixture-')
        self.root = Path(self.temporary.name).resolve()
        self.addCleanup(self.temporary.cleanup)
        patcher = patch.object(data, 'ROOT', self.root)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_dataset_and_evaluation_actions_are_gone(self):
        for action in ('dataset_create', 'dataset_verify', 'evaluate'):
            with self.assertRaises(KeyError):
                data.dispatch({'action': action})
        self.assertFalse(hasattr(data, 'evaluate'))

    def test_library_lists_projects_only(self):
        data.project_create(dict(name='monitor', kind='ros2'))
        library = data.dispatch({'action': 'data_library'})
        self.assertEqual([p['name'] for p in library['projects']], ['monitor'])
        self.assertNotIn('datasets', library)
        self.assertNotIn('reports', library)

    def test_project_scaffolding_does_not_overwrite_or_install(self):
        result = data.project_create(dict(name='monitor', kind='ros2'))['project']
        self.assertTrue((Path(result['path']) / 'main.py').is_file())
        self.assertFalse((Path(result['path']) / '.venv').exists())
        with self.assertRaises(ValueError):
            data.project_create(dict(name='monitor', kind='ros2'))
        with self.assertRaises(ValueError):
            data.project_create(dict(name='../outside', kind='ros2'))

    def test_inference_and_monitor_templates_are_editable_without_runtime_imports(self):
        for kind in ('onnx', 'mavsdk'):
            result = data.project_create(dict(name=kind, kind=kind))['project']
            path = Path(result['path'])
            ast.parse((path / 'main.py').read_text())
            self.assertFalse((path / '.venv').exists())
        readme = (self.root / 'projects/onnx/README.md').read_text(encoding='utf-8')
        self.assertIn('NPZ named tensors', readme)
        # The MAVSDK starter reads PX4 SITL's API stream inside WSL, where the project runs.
        starter = (self.root / 'projects/mavsdk/main.py').read_text(encoding='utf-8')
        self.assertIn("DEFAULT_ENDPOINT = 'udpin://0.0.0.0:14540'", starter)
        guide = (self.root / 'projects/mavsdk/README.md').read_text(encoding='utf-8')
        self.assertIn('udpin://0.0.0.0:14540', guide)
        self.assertIn('infer_node.py', readme)
        self.assertEqual(json.loads((self.root / 'projects/onnx/project.json').read_text())['kind'], 'onnx')
        with self.assertRaises(ValueError):
            data.project_create(dict(name='trainer', kind='training'))


if __name__ == '__main__':
    unittest.main()
