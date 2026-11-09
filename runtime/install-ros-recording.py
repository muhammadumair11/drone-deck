"""Install the official ROS Jazzy recording packages on Ubuntu Noble; run as root."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import urllib.request

def main():
    if os.geteuid() != 0 or 'VERSION_CODENAME=noble' not in Path('/etc/os-release').read_text():
        raise SystemExit('Requires root inside Ubuntu 24.04 (Noble).')
    request = urllib.request.Request(
        'https://api.github.com/repos/ros-infrastructure/ros-apt-source/releases/latest',
        headers={'User-Agent': 'Drone-Deck-ROS-Setup'})
    with urllib.request.urlopen(request, timeout=45) as response:
        release = json.load(response)
    asset = next(a for a in release['assets']
                 if a['name'].startswith('ros2-apt-source_') and a['name'].endswith('.noble_all.deb'))
    package = Path('/tmp/drone-deck-ros2-apt-source.deb')
    print('Official ROS repository package:', release['tag_name'], asset['name'], flush=True)
    with urllib.request.urlopen(asset['browser_download_url'], timeout=60) as response:
        content = response.read()
    digest = asset.get('digest', '')
    if digest.startswith('sha256:') and hashlib.sha256(content).hexdigest() != digest[7:]:
        raise SystemExit('Repository package checksum mismatch.')
    package.write_bytes(content)
    subprocess.run(['dpkg', '-i', str(package)], check=True)
    env = {**os.environ, 'DEBIAN_FRONTEND': 'noninteractive'}
    subprocess.run(['apt-get', 'update'], check=True, env=env)
    subprocess.run(['apt-get', 'install', '-y', '--no-install-recommends',
                    'ros-jazzy-ros-base', 'ros-jazzy-rosbag2-storage-mcap'], check=True, env=env)
    print('ROS_RECORDING_INSTALL_COMPLETE', flush=True)

if __name__ == '__main__':
    main()
