"""Dependency-light worker tests; --integration runs an actual ROS sensor round trip."""
import argparse
import contextlib
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
import recordings as rec

class RecordingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='drone-recording-test-')
        self.addCleanup(self.temp.cleanup)
        self.server = rec.Server(self.temp.name)

    def test_rejects_paths_and_shell_input(self):
        for value in ('../outside', '/tmp/outside', 'flight;touch hacked', ''):
            with self.assertRaises(ValueError):
                self.server.session_path(value)
        for selected in ([], ['/imu; echo hacked'], ['$(whoami)'], ['/imu', 1]):
            with self.assertRaises(ValueError):
                rec.topics(selected)
        self.assertEqual(rec.topics(['/imu', '/imu', '/camera/image']), ['/imu', '/camera/image'])
        outside = Path(self.temp.name).parent
        link = Path(self.temp.name) / 'linked'
        link.symlink_to(outside, target_is_directory=True)
        with self.assertRaises(ValueError):
            self.server.session_path('linked')

    def test_missing_ros_is_an_actionable_error(self):
        with patch.object(rec, 'SETUP', '/not-installed/setup.bash'):
            with self.assertRaisesRegex(RuntimeError, 'ROS 2 Jazzy is missing'):
                self.server.check()

    def test_unsafe_replay_domains_are_rejected_before_any_launch(self):
        self.server.ready = True
        path = self.server.session_path('flight')
        path.mkdir()
        rec.atomic_json(path / 'session.json', {'domain': 3})
        metadata = {'storage_identifier': 'mcap', 'message_count': 2}
        with patch.object(rec, 'bag_metadata', return_value=metadata):
            for current, replay in ((0, 0), (0, 3)):
                with self.assertRaisesRegex(ValueError, 'differ'):
                    self.server.replay({'session': 'flight', 'capture_domain': current, 'domain': replay})
        self.assertFalse(self.server.jobs)

    def test_replay_filters_control_inputs_and_is_local_only(self):
        self.server.ready = True
        path = self.server.session_path('flight')
        path.mkdir()
        rec.atomic_json(path / 'session.json', {'domain': 0})
        metadata = {'storage_identifier': 'mcap', 'message_count': 3,
                    'topics_with_message_count': [
                        {'topic_metadata': {'name': name}}
                        for name in ['/imu', '/fmu/in/vehicle_command', '/robot/cmd_vel']]}
        with patch.object(rec, 'bag_metadata', return_value=metadata), patch.object(self.server, 'start_job') as launch:
            result = self.server.replay({'session': 'flight', 'domain': 77})
        self.assertEqual(result['topics'], ['/imu'])
        self.assertTrue(launch.call_args.kwargs['local'])
        self.assertEqual(launch.call_args.args[2], 77)

    def test_unfinalized_sessions_are_not_reported_as_complete(self):
        path = self.server.session_path('interrupted')
        path.mkdir()
        rec.atomic_json(path / 'session.json', {'label': 'test', 'status': 'recording'})
        rows = self.server.sessions()
        self.assertEqual(rows[0]['status'], 'Interrupted; inspect files')
        self.server.finalize(path, 0)
        self.assertEqual(json.loads((path / 'session.json').read_text())['status'], 'Incomplete; inspect logs')

    def test_broken_metadata_does_not_hide_the_recording_library(self):
        path = self.server.session_path('broken-metadata')
        (path / 'bag').mkdir(parents=True)
        rec.atomic_json(path / 'session.json', {'label': 'broken', 'status': 'recording'})
        (path / 'bag' / 'metadata.yaml').write_text('rosbag2_bagfile_information: [')
        self.server.finalize(path, 0)
        rows = self.server.sessions()
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]['status'], 'Incomplete; inspect logs')

    def test_command_uses_an_explicit_domain_without_changing_parent_environment(self):
        self.server.env = {**os.environ, 'ROS_DISCOVERY_SERVER': 'remote:11811'}
        command, env = self.server.command(['bag', 'play', 'path with spaces'], 77, local=True)
        self.assertEqual(command[-1], 'path with spaces')
        self.assertEqual(env['ROS_DOMAIN_ID'], '77')
        self.assertEqual(env['ROS_AUTOMATIC_DISCOVERY_RANGE'], 'LOCALHOST')
        self.assertNotIn('ROS_DISCOVERY_SERVER', env)
        self.assertIn('ROS_DISCOVERY_SERVER', self.server.env)

def integration():
    """Record generated IMU messages, finalize MCAP, then receive replay in a second domain."""
    with tempfile.TemporaryDirectory(prefix='drone-ros-roundtrip-') as directory:
        server = rec.Server(directory)
        print('Dependencies:', server.check(), flush=True)
        env = {**server.env, 'ROS_DOMAIN_ID': '89', 'ROS_LOCALHOST_ONLY': '1'}
        publisher_code = '''
import rclpy
from rclpy.node import Node
from rclpy.qos import qos_profile_sensor_data
from sensor_msgs.msg import Imu
rclpy.init()
node=Node('drone_deck_test_sensor')
pub=node.create_publisher(Imu,'/drone_deck_test/imu',qos_profile_sensor_data)
def tick():
    message=Imu(); message.header.stamp=node.get_clock().now().to_msg()
    message.header.frame_id='test_imu'; message.linear_acceleration.z=9.81
    pub.publish(message)
timer=node.create_timer(0.02,tick)
try: rclpy.spin(node)
except KeyboardInterrupt: pass
finally: node.destroy_node(); rclpy.try_shutdown()
'''
        publisher = subprocess.Popen(['python3', '-u', '-c', publisher_code], env=env,
                                     stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                                     start_new_session=True, text=True)
        listener = None
        try:
            time.sleep(2)
            discovered = server.dispatch({'action': 'topics', 'domain': 89})
            assert any(t['name'] == '/drone_deck_test/imu' for t in discovered), discovered
            started = server.record({'name': 'synthetic-imu', 'source': 'Generated IMU test',
                                     'topics': ['/drone_deck_test/imu'], 'domain': 89})
            time.sleep(5)
            server.stop()
            # Exercise the actual JSON worker and app-close/EOF finalization path.
            ipc_root = Path(directory) / 'ipc'
            worker = subprocess.Popen(['python3', '-u', str(Path(rec.__file__)), '--root', str(ipc_root)],
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            worker.stdin.write(json.dumps({'action': 'check'}) + '\n')
            worker.stdin.write(json.dumps({'action': 'record', 'name': 'eof-test',
                                'topics': ['/drone_deck_test/imu'], 'domain': 89}) + '\n')
            worker.stdin.flush()
            time.sleep(7)
            worker.stdin.close()
            worker.stdin = None
            output, errors = worker.communicate(timeout=40)
            assert worker.returncode == 0, output + errors
            ipc_rows = rec.Server(ipc_root).sessions()
            assert len(ipc_rows) == 1 and ipc_rows[0]['status'] == 'Finalized', output + errors
            assert ipc_rows[0]['messages'] > 10, ipc_rows
            print('WORKER_EOF_FINALIZATION_PASSED', ipc_rows[0]['messages'], flush=True)
            os.killpg(publisher.pid, signal.SIGINT)
            publisher.wait(timeout=10)
            rows = server.sessions()
            row = next(row for row in rows if row['id'] == started['id'])
            assert row['status'] == 'Finalized', row
            assert row['messages'] > 20, row
            assert row['topics'][0]['type'] == 'sensor_msgs/msg/Imu', row
            file = next((Path(row['path']) / 'bag').glob('*.mcap'))
            with file.open('rb') as handle:
                assert handle.read(8) == b'\x89MCAP0\r\n'
            listener_code = '''
import rclpy,time
from rclpy.node import Node
from rclpy.qos import qos_profile_sensor_data
from sensor_msgs.msg import Imu
rclpy.init(); node=Node('drone_deck_replay_probe'); values=[]
sub=node.create_subscription(Imu,'/drone_deck_test/imu',lambda m: values.append(m.linear_acceleration.z),qos_profile_sensor_data)
end=time.monotonic()+14
while time.monotonic()<end and len(values)<5: rclpy.spin_once(node,timeout_sec=0.2)
print('REPLAY_RECEIVED',len(values),flush=True)
assert len(values)>=5 and all(abs(v-9.81)<0.001 for v in values)
node.destroy_node(); rclpy.shutdown()
'''
            replay_env = {**server.env, 'ROS_DOMAIN_ID': '90', 'ROS_LOCALHOST_ONLY': '1',
                          'ROS_AUTOMATIC_DISCOVERY_RANGE': 'LOCALHOST'}
            listener = subprocess.Popen(['python3', '-u', '-c', listener_code], env=replay_env,
                                        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                        start_new_session=True)
            time.sleep(2)
            server.replay({'session': row['id'], 'domain': 90, 'capture_domain': 89, 'rate': 0.5})
            output, errors = listener.communicate(timeout=18)
            assert listener.returncode == 0, output + errors
            server.stop()
            print(output.strip(), flush=True)
            print('ROS_MCAP_ROUNDTRIP_PASSED', json.dumps({
                'messages': row['messages'], 'duration': row['duration'], 'bytes': row['bytes'],
                'storage': 'mcap', 'type': row['topics'][0]['type'],
                'capture_domain': 89, 'replay_domain': 90}), flush=True)
        finally:
            server.stop()
            for child in (publisher, listener):
                if child and child.poll() is None:
                    os.killpg(child.pid, signal.SIGINT)
                    try: child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        os.killpg(child.pid, signal.SIGKILL); child.wait()

if __name__ == '__main__':
    if '--integration' in sys.argv:
        integration()
    else:
        unittest.main()
