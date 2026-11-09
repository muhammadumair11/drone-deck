import ast
from pathlib import Path
from mavsdk.asyncio import ComponentType, Configuration, Mavsdk
from mavsdk.asyncio.plugins.telemetry import TelemetryAsync

for path in Path(__file__).parent.glob('*.py'):
    ast.parse(path.read_text(), filename=str(path))
for method in ['subscribe_position', 'subscribe_health', 'subscribe_flight_mode']:
    assert hasattr(TelemetryAsync, method), method
assert hasattr(Mavsdk, 'add_any_connection')
assert hasattr(Mavsdk, 'first_autopilot')
assert hasattr(Configuration, 'create_with_component_type')
print('Python scripts parse; installed MAVSDK exposes every adapter API.')
