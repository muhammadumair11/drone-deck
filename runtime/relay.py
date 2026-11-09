import socket
import subprocess
import json

gateway = subprocess.check_output(['ip', 'route', 'show', 'default'], text=True).split()[2]
source = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
source.bind(('127.0.0.1', 19410))
destination = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
print(f'SIH display relay → {gateway}:19410. Existing Windows firewall rule must allow this gateway.', flush=True)
first = True
while True:
    packet, _ = source.recvfrom(65535)
    destination.sendto(packet, (gateway, 19410))
    if first:
        print('@readiness ' + json.dumps(dict(state='Forwarding', detail='Live SIH state received and forwarded to Windows. UDP delivery does not confirm firewall or visualizer rendering.')), flush=True)
        first = False
