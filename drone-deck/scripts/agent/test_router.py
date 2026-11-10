"""MAVLink router tests over local sockets only (127.0.0.1, ephemeral ports)."""
import socket
import threading
import time
import unittest

import router


class Events:
    def __init__(self):
        self.items = []
        self.lock = threading.Lock()

    def event(self, name, **fields):
        with self.lock:
            self.items.append(dict(fields, event=name))

    def log(self, level, message):
        self.event('log', level=level, message=message)

    def last(self, name):
        with self.lock:
            matching = [e for e in self.items if e['event'] == name]
        return matching[-1] if matching else None


def wait_for(predicate, timeout=5):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return True
        time.sleep(.02)
    return False


class RouterTests(unittest.TestCase):
    def setUp(self):
        self.events = Events()
        self.router = router.Router(self.events)
        self.addCleanup(self.router.stop)
        status = self.router.start(udp_listen=0, tcp_port=0, udp_host='127.0.0.1', tcp_host='127.0.0.1')
        self.udp_port, self.tcp_port = status['udp_listen'], status['tcp_port']
        self.vehicle = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.vehicle.bind(('127.0.0.1', 0))
        self.vehicle.settimeout(5)
        self.addCleanup(self.vehicle.close)

    def client(self):
        conn = socket.create_connection(('127.0.0.1', self.tcp_port), timeout=5)
        self.addCleanup(conn.close)
        self.assertTrue(wait_for(lambda: self.router.status()['clients'] >= 1))
        return conn

    def receive(self, conn, size):
        data = b''
        while len(data) < size:
            chunk = conn.recv(size - len(data))
            if not chunk:
                break
            data += chunk
        return data

    def test_vehicle_bytes_reach_every_tcp_client_and_replies_reach_the_vehicle(self):
        first, second = self.client(), self.client()
        self.assertTrue(wait_for(lambda: self.router.status()['clients'] == 2))
        heartbeat = bytes([0xFD, 9, 0, 0, 0, 1, 1, 0, 0, 0]) + bytes(11)
        self.vehicle.sendto(heartbeat, ('127.0.0.1', self.udp_port))
        self.assertEqual(self.receive(first, len(heartbeat)), heartbeat)
        self.assertEqual(self.receive(second, len(heartbeat)), heartbeat)
        first.sendall(b'\xfd\x01command-bytes')
        data, _ = self.vehicle.recvfrom(1024)
        self.assertEqual(data, b'\xfd\x01command-bytes')
        self.assertTrue(wait_for(lambda: self.router.status()['to_vehicle'] == 1))
        status = self.router.status()
        self.assertEqual(status['state'], 'forwarding')
        self.assertEqual(status['packets'], 1)
        self.assertEqual(status['to_vehicle'], 1)
        self.assertEqual(status['vehicle'], '127.0.0.1:%d' % self.vehicle.getsockname()[1])
        self.assertTrue(wait_for(lambda: (self.events.last('router') or {}).get('state') == 'forwarding'))

    def test_the_first_sender_stays_the_vehicle_and_other_senders_are_foreign(self):
        conn = self.client()
        self.vehicle.sendto(b'own', ('127.0.0.1', self.udp_port))
        self.assertEqual(self.receive(conn, 3), b'own')
        other = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        other.bind(('127.0.0.1', 0))
        other.settimeout(0.3)
        self.addCleanup(other.close)
        other.sendto(b'stranger', ('127.0.0.1', self.udp_port))
        self.assertTrue(wait_for(lambda: self.router.status()['foreign'] == 1))
        self.vehicle.sendto(b'again', ('127.0.0.1', self.udp_port))
        # The stranger's datagram never reached the client: the next bytes are the vehicle's.
        self.assertEqual(self.receive(conn, 5), b'again')
        conn.sendall(b'command')
        data, _ = self.vehicle.recvfrom(1024)
        self.assertEqual(data, b'command')
        with self.assertRaises(socket.timeout):
            other.recvfrom(1024)
        status = self.router.status()
        self.assertEqual((status['packets'], status['foreign']), (2, 1))
        self.assertEqual(status['vehicle'], '127.0.0.1:%d' % self.vehicle.getsockname()[1])
        self.assertTrue(wait_for(lambda: (self.events.last('router') or {}).get('foreign') == 1))

    def test_large_client_writes_reach_the_vehicle_in_small_datagrams(self):
        conn = self.client()
        self.vehicle.sendto(b'hb', ('127.0.0.1', self.udp_port))
        self.assertEqual(self.receive(conn, 2), b'hb')
        self.vehicle.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 1 << 20)
        payload = bytes(range(256)) * 400  # 102,400 bytes: over the UDP datagram limit
        conn.sendall(payload)
        received = b''
        while len(received) < len(payload):
            data, _ = self.vehicle.recvfrom(65535)
            self.assertLessEqual(len(data), router.DATAGRAM)
            received += data
        self.assertEqual(received, payload)
        self.assertEqual(self.router.status()['dropped'], 0)

    def test_udp_listens_on_localhost_by_default(self):
        other = router.Router(self.events)
        self.addCleanup(other.stop)
        status = other.start(udp_listen=0, tcp_port=0, tcp_host='127.0.0.1')
        self.assertEqual(other.config[0], '127.0.0.1')
        self.assertEqual(status['foreign'], 0)

    def test_client_bytes_before_any_vehicle_are_dropped_not_misrouted(self):
        conn = self.client()
        conn.sendall(b'early')
        self.assertTrue(wait_for(lambda: self.router.status()['dropped'] == 1))
        self.assertEqual(self.router.status()['state'], 'listening')

    def test_disconnects_are_counted_and_stop_reports_stopped(self):
        conn = self.client()
        conn.close()
        self.assertTrue(wait_for(lambda: self.router.status()['clients'] == 0))
        self.assertEqual(self.router.stop()['state'], 'stopped')
        self.assertEqual(self.events.last('router')['state'], 'stopped')
        with self.assertRaises(OSError):
            socket.create_connection(('127.0.0.1', self.tcp_port), timeout=1).close()

    def test_a_client_that_stops_reading_is_dropped_and_others_keep_receiving(self):
        stalled = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        stalled.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4096)
        stalled.connect(('127.0.0.1', self.tcp_port))
        self.addCleanup(stalled.close)
        reader = self.client()
        self.assertTrue(wait_for(lambda: self.router.status()['clients'] == 2))
        received = bytearray()
        reader.settimeout(0.01)
        packet = bytes(1400)
        sent = 0
        # Far more than the stalled client's kernel buffers plus the router's 1 MiB allowance.
        while sent < 64 * 1024 * 1024 and self.router.status()['clients'] == 2:
            for _ in range(256):
                self.vehicle.sendto(packet, ('127.0.0.1', self.udp_port))
                sent += len(packet)
            try:
                while True:
                    received += reader.recv(1 << 20)
            except (socket.timeout, BlockingIOError):
                pass
        self.assertTrue(wait_for(lambda: self.router.status()['clients'] == 1))
        before = len(received)
        self.vehicle.sendto(b'after', ('127.0.0.1', self.udp_port))
        reader.settimeout(5)
        deadline = time.monotonic() + 5
        while not received.endswith(b'after') and time.monotonic() < deadline:
            received += reader.recv(1 << 20)
        self.assertGreater(len(received), before)
        self.assertTrue(received.endswith(b'after'))

    def test_same_ports_are_idempotent_and_busy_ports_are_reported(self):
        again = self.router.start(udp_listen=self.udp_port, tcp_port=self.tcp_port,
                                  udp_host='127.0.0.1', tcp_host='127.0.0.1')
        self.assertEqual(again['tcp_port'], self.tcp_port)
        other = router.Router(self.events)
        with self.assertRaisesRegex(RuntimeError, 'in use'):
            other.start(udp_listen=self.udp_port, tcp_port=0, udp_host='127.0.0.1', tcp_host='127.0.0.1')
        with self.assertRaisesRegex(RuntimeError, 'other ports'):
            self.router.start(udp_listen=self.udp_port + 1, tcp_port=0, udp_host='127.0.0.1',
                              tcp_host='127.0.0.1')
        with self.assertRaises(ValueError):
            router.port(70000, 'tcp_port')


if __name__ == '__main__':
    unittest.main()
