"""JSON-lines output shared by every agent thread: replies and events, one object per line."""
import json
import threading


class Out:
    """Writes one JSON object per line. A broken stream closes it; later writes are dropped."""

    def __init__(self, stream):
        self.stream = stream
        self.lock = threading.Lock()
        self.closed = False

    def send(self, value):
        text = json.dumps(value, ensure_ascii=False, separators=(',', ':'), default=str)
        with self.lock:
            if self.closed:
                return False
            try:
                self.stream.write(text + '\n')
                self.stream.flush()
                return True
            except (OSError, ValueError):
                self.closed = True
                return False

    def event(self, name, **fields):
        return self.send({'event': name, **fields})

    def log(self, level, message):
        return self.event('log', level=level, message=str(message)[:4000])

    def reply(self, request_id, result):
        return self.send({'id': request_id, 'result': result})

    def error(self, request_id, message):
        return self.send({'id': request_id, 'error': {'message': str(message)[:3000]}})
