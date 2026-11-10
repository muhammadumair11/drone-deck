"""MAVLink UDP <-> TCP router inside WSL.

PX4 SITL sends its ground-station link to UDP 14550 on localhost inside WSL. The router listens
there and serves the same bytes on a TCP port that Windows reaches through WSL's localhost
forwarding. The first UDP sender is the vehicle for the router's lifetime: bytes from any TCP
client go back to that address only, and datagrams from any other address (another flight stack,
or a mavproxy forwarding a real flight controller) are counted as `foreign` and never forwarded.
A new vehicle address needs a router restart. The router forwards raw bytes only: it never
parses, filters by content or originates MAVLink.
"""
import os
import selectors
import socket
import threading
import time

CLIENT_BUFFER = 1024 * 1024
FRESH = 3.0
REPORT_INTERVAL = 1.0
MAX_CLIENTS = 16
# Client bytes go to the vehicle in datagrams of at most this size. TCP coalesces client writes,
# and a datagram over 65,507 bytes cannot be sent at all. The flight stacks' MAVLink parsers keep
# their state across datagrams, so splitting a frame is safe for a router that never parses.
DATAGRAM = 1400


def port(value, name):
    try:
        number = int(value)
    except (TypeError, ValueError):
        raise ValueError(f'{name} must be a port number.')
    if not 0 <= number <= 65535:
        raise ValueError(f'{name} must be between 0 and 65535.')
    return number


class Router:
    def __init__(self, out, clock=time.monotonic):
        self.out = out
        self.clock = clock
        self.lock = threading.Lock()
        # One start or stop at a time: two concurrent starts must not leave two loops.
        self.control = threading.Lock()
        self.thread = None
        self.stop_event = threading.Event()
        self.config = None
        self.stats = self._idle('stopped')

    @staticmethod
    def _idle(state):
        return {'state': state, 'clients': 0, 'packets': 0, 'to_vehicle': 0, 'dropped': 0,
                'foreign': 0, 'vehicle': None, 'udp_listen': None, 'tcp_port': None}

    def snapshot(self):
        with self.lock:
            return dict(self.stats)

    def _publish(self, **changes):
        with self.lock:
            self.stats.update(changes)
            value = dict(self.stats)
        self.out.event('router', **value)

    def start(self, udp_listen=14550, tcp_port=5760, udp_host='127.0.0.1', tcp_host='0.0.0.0'):
        with self.control:
            return self._start(udp_listen, tcp_port, udp_host, tcp_host)

    def _start(self, udp_listen, tcp_port, udp_host, tcp_host):
        config = (str(udp_host), port(udp_listen, 'udp_listen'), str(tcp_host), port(tcp_port, 'tcp_port'))
        with self.lock:
            if self.thread is not None and self.thread.is_alive():
                if all(want in (0, have) if isinstance(want, int) else want == have
                       for want, have in zip(config, self.config)):
                    return dict(self.stats)
                raise RuntimeError('The MAVLink router already runs with other ports. Stop it first.')
        udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        try:
            udp.bind((config[0], config[1]))
        except OSError as error:
            udp.close()
            raise RuntimeError(f'UDP {config[1]} is in use inside WSL ({error.strerror}). '
                               'Another MAVLink router or ground station may own it.')
        tcp = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        if os.name != 'nt':  # On Windows the option would let a second listener share the port.
            tcp.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            tcp.bind((config[2], config[3]))
            tcp.listen(MAX_CLIENTS)
        except OSError as error:
            udp.close()
            tcp.close()
            raise RuntimeError(f'TCP {config[3]} is in use inside WSL ({error.strerror}).')
        udp.setblocking(False)
        tcp.setblocking(False)
        stop_event = threading.Event()
        with self.lock:
            self.config = (config[0], udp.getsockname()[1], config[2], tcp.getsockname()[1])
            self.stop_event = stop_event
            self.stats = self._idle('listening')
            self.stats.update(udp_listen=udp.getsockname()[1], tcp_port=tcp.getsockname()[1])
            self.thread = threading.Thread(target=self._loop, args=(udp, tcp, stop_event), daemon=True)
            self.thread.start()
            value = dict(self.stats)
        self.out.event('router', **value)
        return value

    def stop(self):
        with self.control:
            return self._stop()

    def _stop(self):
        with self.lock:
            thread = self.thread
            self.stop_event.set()
        if thread is not None:
            thread.join(timeout=3)
        with self.lock:
            was_running = self.thread is not None
            self.thread = None
            self.config = None
        if was_running:
            self._publish(**self._idle('stopped'))
        return self.snapshot()

    def status(self):
        return self.snapshot()

    def _loop(self, udp, tcp, stop_event):
        selector = selectors.DefaultSelector()
        clients = {}
        vehicle = None
        last_rx = None
        packets = to_vehicle = dropped = foreign = 0
        reported = None
        last_counts = None
        next_report = self.clock()

        def close(conn):
            clients.pop(conn, None)
            try:
                selector.unregister(conn)
            except (KeyError, ValueError):
                pass
            conn.close()

        def want_write(conn, on):
            selector.modify(conn, selectors.EVENT_READ | (selectors.EVENT_WRITE if on else 0), 'client')

        try:
            selector.register(udp, selectors.EVENT_READ, 'udp')
            selector.register(tcp, selectors.EVENT_READ, 'accept')
            while not stop_event.is_set():
                for key, mask in selector.select(timeout=0.2):
                    kind = key.data
                    if kind == 'udp':
                        for _ in range(256):
                            try:
                                data, address = udp.recvfrom(65535)
                            except (BlockingIOError, InterruptedError):
                                break
                            except OSError:
                                break
                            if vehicle is None:
                                vehicle = address
                            elif address != vehicle:
                                foreign += 1
                                continue
                            last_rx = self.clock()
                            packets += 1
                            for conn, buffer in list(clients.items()):
                                if len(buffer) + len(data) > CLIENT_BUFFER:
                                    # A client that stopped reading is dropped, never the vehicle link.
                                    close(conn)
                                    continue
                                if not buffer:
                                    want_write(conn, True)
                                buffer += data
                    elif kind == 'accept':
                        try:
                            conn, _ = tcp.accept()
                        except (BlockingIOError, InterruptedError):
                            continue
                        if len(clients) >= MAX_CLIENTS:
                            conn.close()
                            continue
                        conn.setblocking(False)
                        conn.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                        clients[conn] = bytearray()
                        selector.register(conn, selectors.EVENT_READ, 'client')
                    else:
                        conn = key.fileobj
                        if conn not in clients:
                            continue
                        if mask & selectors.EVENT_READ:
                            try:
                                data = conn.recv(65536)
                            except (BlockingIOError, InterruptedError):
                                data = None
                            except OSError:
                                data = b''
                            if data == b'':
                                close(conn)
                                continue
                            if data:
                                if vehicle is None:
                                    dropped += 1
                                else:
                                    for at in range(0, len(data), DATAGRAM):
                                        try:
                                            udp.sendto(data[at:at + DATAGRAM], vehicle)
                                            to_vehicle += 1
                                        except OSError:
                                            dropped += 1
                        if mask & selectors.EVENT_WRITE and conn in clients:
                            buffer = clients[conn]
                            try:
                                sent = conn.send(buffer)
                            except (BlockingIOError, InterruptedError):
                                sent = 0
                            except OSError:
                                close(conn)
                                continue
                            del buffer[:sent]
                            if not buffer:
                                want_write(conn, False)
                now = self.clock()
                state = 'forwarding' if last_rx is not None and now - last_rx < FRESH else 'listening'
                shape = (state, len(clients))
                current = (state, len(clients), packets, to_vehicle, dropped, foreign)
                with self.lock:
                    self.stats.update(state=state, clients=len(clients), packets=packets,
                                      to_vehicle=to_vehicle, dropped=dropped, foreign=foreign,
                                      vehicle=None if vehicle is None else f'{vehicle[0]}:{vehicle[1]}')
                # Events on a state or client change at once, counters at most once a second.
                if shape != reported or (now >= next_report and current != last_counts):
                    self._publish()
                    last_counts = current
                    reported = shape
                    next_report = now + REPORT_INTERVAL
        except Exception as error:  # noqa: BLE001 - reported to the app, never silent
            self._publish(state='failed', clients=0)
            self.out.log('error', f'MAVLink router stopped: {error}')
        finally:
            for conn in list(clients):
                close(conn)
            selector.close()
            udp.close()
            tcp.close()
